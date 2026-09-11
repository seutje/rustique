use std::path::Path;

use bytemuck::{Pod, Zeroable};
use simulation::{Particle, initialize_particles};
use thiserror::Error;
use wgpu::util::DeviceExt;

use crate::{GpuContext, OffscreenError, OffscreenRenderTarget, RgbaColor};

const PARTICLE_SHADER: &str = include_str!("../../../shaders/particles/particles.wgsl");

/// Per-frame inputs shared by compute and render shaders (32 bytes).
#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
pub struct FrameUniforms {
    pub view_projection: [[f32; 4]; 4],
    pub frame_index: u32,
    pub particle_count: u32,
    pub delta_time: f32,
    pub simulation_time: f32,
}

const _: () = assert!(size_of::<FrameUniforms>() == 80);

#[derive(Debug, Error)]
pub enum ParticleRenderError {
    #[error("particle count must be greater than zero")]
    EmptyParticleSet,
    #[error(
        "particle buffers require {required} bytes, exceeding the GPU storage buffer limit of {maximum} bytes"
    )]
    BufferLimit { required: u64, maximum: u64 },
    #[error(transparent)]
    Offscreen(#[from] OffscreenError),
}

/// Reusable GPU-resident ping-pong simulation and point rendering pipeline.
pub struct ParticleRenderer {
    particle_count: u32,
    _buffers: [wgpu::Buffer; 2],
    uniform_buffer: wgpu::Buffer,
    bind_groups: [wgpu::BindGroup; 2],
    compute_pipeline: wgpu::ComputePipeline,
    render_pipeline: wgpu::RenderPipeline,
    source_index: usize,
}

impl ParticleRenderer {
    /// Allocates and deterministically initializes GPU particle state.
    ///
    /// # Errors
    ///
    /// Returns an error for an empty particle set or a storage allocation that
    /// exceeds the selected device's binding limit.
    #[allow(clippy::too_many_lines)]
    pub fn new(
        context: &GpuContext,
        particle_count: u32,
        seed: u64,
    ) -> Result<Self, ParticleRenderError> {
        if particle_count == 0 {
            return Err(ParticleRenderError::EmptyParticleSet);
        }
        let required = u64::from(particle_count) * size_of::<Particle>() as u64;
        let maximum = u64::from(context.device.limits().max_storage_buffer_binding_size);
        if required > maximum {
            return Err(ParticleRenderError::BufferLimit { required, maximum });
        }
        let particles = initialize_particles(particle_count, seed);
        let make_buffer = |label| {
            context
                .device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some(label),
                    contents: bytemuck::cast_slice(&particles),
                    usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
                })
        };
        let buffers = [
            make_buffer("particle-buffer-a"),
            make_buffer("particle-buffer-b"),
        ];
        let uniform_buffer = context.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("particle-frame-uniforms"),
            size: size_of::<FrameUniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let layout = context
            .device
            .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("particle-bind-layout"),
                entries: &[
                    storage_entry(
                        0,
                        true,
                        wgpu::ShaderStages::COMPUTE | wgpu::ShaderStages::VERTEX,
                    ),
                    storage_entry(1, false, wgpu::ShaderStages::COMPUTE),
                    wgpu::BindGroupLayoutEntry {
                        binding: 2,
                        visibility: wgpu::ShaderStages::COMPUTE | wgpu::ShaderStages::VERTEX,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Uniform,
                            has_dynamic_offset: false,
                            min_binding_size: None,
                        },
                        count: None,
                    },
                ],
            });
        let bind_groups = [
            make_bind_group(
                &context.device,
                &layout,
                &buffers[0],
                &buffers[1],
                &uniform_buffer,
                "particle-bind-a-b",
            ),
            make_bind_group(
                &context.device,
                &layout,
                &buffers[1],
                &buffers[0],
                &uniform_buffer,
                "particle-bind-b-a",
            ),
        ];
        let shader = context
            .device
            .create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("particle-shader"),
                source: wgpu::ShaderSource::Wgsl(PARTICLE_SHADER.into()),
            });
        let pipeline_layout =
            context
                .device
                .create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                    label: Some("particle-pipeline-layout"),
                    bind_group_layouts: &[&layout],
                    push_constant_ranges: &[],
                });
        let compute_pipeline =
            context
                .device
                .create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                    label: Some("particle-compute-pipeline"),
                    layout: Some(&pipeline_layout),
                    module: &shader,
                    entry_point: Some("update"),
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                    cache: None,
                });
        let render_pipeline =
            context
                .device
                .create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                    label: Some("particle-render-pipeline"),
                    layout: Some(&pipeline_layout),
                    vertex: wgpu::VertexState {
                        module: &shader,
                        entry_point: Some("vertex"),
                        compilation_options: wgpu::PipelineCompilationOptions::default(),
                        buffers: &[],
                    },
                    fragment: Some(wgpu::FragmentState {
                        module: &shader,
                        entry_point: Some("fragment"),
                        compilation_options: wgpu::PipelineCompilationOptions::default(),
                        targets: &[Some(wgpu::ColorTargetState {
                            format: wgpu::TextureFormat::Rgba8UnormSrgb,
                            blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                            write_mask: wgpu::ColorWrites::ALL,
                        })],
                    }),
                    primitive: wgpu::PrimitiveState {
                        topology: wgpu::PrimitiveTopology::PointList,
                        ..Default::default()
                    },
                    depth_stencil: None,
                    multisample: wgpu::MultisampleState::default(),
                    multiview: None,
                    cache: None,
                });
        Ok(Self {
            particle_count,
            _buffers: buffers,
            uniform_buffer,
            bind_groups,
            compute_pipeline,
            render_pipeline,
            source_index: 0,
        })
    }

    #[must_use]
    pub const fn particle_count(&self) -> u32 {
        self.particle_count
    }

    /// Advances one fixed frame and renders the resulting GPU buffer.
    ///
    /// # Errors
    ///
    /// Returns an error if the completed texture cannot be read back.
    #[allow(clippy::cast_precision_loss)]
    pub fn render_frame(
        &mut self,
        context: &GpuContext,
        target: &OffscreenRenderTarget,
        frame_index: u32,
        fps: f32,
        clear: RgbaColor,
    ) -> Result<Vec<u8>, ParticleRenderError> {
        let uniforms = FrameUniforms {
            view_projection: aspect_matrix(target.dimensions()),
            frame_index,
            particle_count: self.particle_count,
            delta_time: 1.0 / fps,
            simulation_time: frame_index as f32 / fps,
        };
        context
            .queue
            .write_buffer(&self.uniform_buffer, 0, bytemuck::bytes_of(&uniforms));
        let view = target.view();
        let mut encoder = context
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("particle-frame"),
            });
        {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("particle-update"),
                timestamp_writes: None,
            });
            pass.set_pipeline(&self.compute_pipeline);
            pass.set_bind_group(0, &self.bind_groups[self.source_index], &[]);
            pass.dispatch_workgroups(self.particle_count.div_ceil(256), 1, 1);
        }
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("particle-render"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(clear.into()),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            pass.set_pipeline(&self.render_pipeline);
            pass.set_bind_group(0, &self.bind_groups[self.source_index ^ 1], &[]);
            pass.draw(0..self.particle_count, 0..1);
        }
        target.encode_readback(&mut encoder);
        context.queue.submit([encoder.finish()]);
        self.source_index ^= 1;
        Ok(target.read_pixels(context)?)
    }

    /// Advances a frame and encodes its final pixels as PNG.
    ///
    /// # Errors
    ///
    /// Returns an error if rendering, readback, file creation, or PNG encoding fails.
    pub fn save_frame_png(
        &mut self,
        context: &GpuContext,
        target: &OffscreenRenderTarget,
        frame_index: u32,
        fps: f32,
        clear: RgbaColor,
        path: impl AsRef<Path>,
    ) -> Result<(), ParticleRenderError> {
        let pixels = self.render_frame(context, target, frame_index, fps, clear)?;
        Ok(target.save_png(&pixels, path.as_ref())?)
    }
}

fn storage_entry(
    binding: u32,
    read_only: bool,
    visibility: wgpu::ShaderStages,
) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility,
        ty: wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Storage { read_only },
            has_dynamic_offset: false,
            min_binding_size: None,
        },
        count: None,
    }
}

fn make_bind_group(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    source: &wgpu::Buffer,
    destination: &wgpu::Buffer,
    uniforms: &wgpu::Buffer,
    label: &str,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some(label),
        layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: source.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: destination.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: uniforms.as_entire_binding(),
            },
        ],
    })
}

#[allow(clippy::cast_precision_loss)]
fn aspect_matrix((width, height): (u32, u32)) -> [[f32; 4]; 4] {
    let aspect = width as f32 / height as f32;
    let x_scale = if aspect > 1.0 { 1.0 / aspect } else { 1.0 };
    let y_scale = if aspect < 1.0 { aspect } else { 1.0 };
    [
        [x_scale, 0.0, 0.0, 0.0],
        [0.0, y_scale, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
        [0.0, 0.0, 0.0, 1.0],
    ]
}
