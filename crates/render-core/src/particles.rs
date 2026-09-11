use std::{path::Path, sync::mpsc, time::Instant};

use bytemuck::{Pod, Zeroable};
use simulation::{Particle, SimulationTiming, initialize_particles};
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
    pub viewport_size: [f32; 2],
    pub particle_size_pixels: f32,
    pub position_scale: f32,
    pub simulation_seed: u32,
    padding: [u32; 3],
}

const _: () = assert!(size_of::<FrameUniforms>() == 112);

#[derive(Clone, Copy, Debug)]
pub struct BenchmarkConfig {
    pub particle_size_pixels: f32,
    pub position_scale: f32,
}

impl Default for BenchmarkConfig {
    fn default() -> Self {
        Self {
            particle_size_pixels: 2.0,
            position_scale: 1.0,
        }
    }
}

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
    #[error("GPU timing readback callback was dropped")]
    TimingCallbackDropped,
    #[error("GPU timing readback failed: {0}")]
    TimingMap(#[from] wgpu::BufferAsyncError),
}

#[derive(Clone, Copy, Debug)]
pub struct FrameTiming {
    pub cpu_prepare_ms: f64,
    pub gpu_compute_ms: Option<f64>,
    pub gpu_render_ms: Option<f64>,
}

/// Reusable GPU-resident ping-pong simulation and point rendering pipeline.
pub struct ParticleRenderer {
    particle_count: u32,
    buffers: [wgpu::Buffer; 2],
    uniform_buffer: wgpu::Buffer,
    bind_groups: [wgpu::BindGroup; 2],
    compute_pipeline: wgpu::ComputePipeline,
    render_pipeline: wgpu::RenderPipeline,
    source_index: usize,
    timing: Option<TimingResources>,
    seed: u64,
    timeline_frame: u32,
}

struct TimingResources {
    queries: wgpu::QuerySet,
    resolve: wgpu::Buffer,
    readback: wgpu::Buffer,
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
                        topology: wgpu::PrimitiveTopology::TriangleList,
                        ..Default::default()
                    },
                    depth_stencil: None,
                    multisample: wgpu::MultisampleState::default(),
                    multiview: None,
                    cache: None,
                });
        Ok(Self {
            particle_count,
            buffers,
            uniform_buffer,
            bind_groups,
            compute_pipeline,
            render_pipeline,
            source_index: 0,
            timing: create_timing_resources(context),
            seed,
            timeline_frame: 0,
        })
    }

    #[must_use]
    pub const fn particle_count(&self) -> u32 {
        self.particle_count
    }

    #[must_use]
    pub fn particle_memory_bytes(&self) -> u64 {
        u64::from(self.particle_count) * size_of::<Particle>() as u64 * 2
    }

    /// Restores both ping-pong buffers to their deterministic frame-zero state.
    pub fn reset(&mut self, context: &GpuContext) {
        let particles = initialize_particles(self.particle_count, self.seed);
        for buffer in &self.buffers {
            context
                .queue
                .write_buffer(buffer, 0, bytemuck::cast_slice(&particles));
        }
        self.source_index = 0;
        self.timeline_frame = 0;
    }

    /// Deterministically seeks to and renders an offline timeline frame.
    ///
    /// Forward seeks replay fixed simulation steps. Backward seeks reset to the
    /// stored seed and replay from frame zero. Only the final color texture is
    /// read back; particle buffers remain GPU-resident.
    ///
    /// # Errors
    ///
    /// Returns an error if final texture readback fails.
    #[allow(clippy::too_many_lines, clippy::cast_precision_loss)]
    pub fn render_timeline_frame(
        &mut self,
        context: &GpuContext,
        target: &OffscreenRenderTarget,
        frame_index: u32,
        timing: SimulationTiming,
        clear: RgbaColor,
    ) -> Result<Vec<u8>, ParticleRenderError> {
        if frame_index < self.timeline_frame {
            self.reset(context);
        }
        for frame in (self.timeline_frame + 1)..=frame_index {
            for substep in 0..timing.substeps() {
                let uniforms = FrameUniforms {
                    view_projection: aspect_matrix(target.dimensions()),
                    frame_index: frame,
                    particle_count: self.particle_count,
                    delta_time: timing.substep_delta(),
                    simulation_time: timing.frame_time(frame)
                        + substep as f32 * timing.substep_delta(),
                    viewport_size: [target.dimensions().0 as f32, target.dimensions().1 as f32],
                    particle_size_pixels: 2.0,
                    position_scale: 1.0,
                    simulation_seed: seed_u32(self.seed),
                    padding: [0; 3],
                };
                context
                    .queue
                    .write_buffer(&self.uniform_buffer, 0, bytemuck::bytes_of(&uniforms));
                let mut encoder =
                    context
                        .device
                        .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                            label: Some("particle-fixed-step"),
                        });
                {
                    let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                        label: Some("particle-fixed-step-update"),
                        timestamp_writes: None,
                    });
                    pass.set_pipeline(&self.compute_pipeline);
                    pass.set_bind_group(0, &self.bind_groups[self.source_index], &[]);
                    let (groups_x, groups_y) = dispatch_dimensions(self.particle_count);
                    pass.dispatch_workgroups(groups_x, groups_y, 1);
                }
                context.queue.submit([encoder.finish()]);
                self.source_index ^= 1;
            }
        }
        self.timeline_frame = frame_index;

        let uniforms = FrameUniforms {
            view_projection: aspect_matrix(target.dimensions()),
            frame_index,
            particle_count: self.particle_count,
            delta_time: timing.substep_delta(),
            simulation_time: timing.frame_time(frame_index),
            viewport_size: [target.dimensions().0 as f32, target.dimensions().1 as f32],
            particle_size_pixels: 2.0,
            position_scale: 1.0,
            simulation_seed: seed_u32(self.seed),
            padding: [0; 3],
        };
        context
            .queue
            .write_buffer(&self.uniform_buffer, 0, bytemuck::bytes_of(&uniforms));
        let view = target.view();
        let mut encoder = context
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("particle-timeline-render"),
            });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("particle-timeline-render-pass"),
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
            pass.set_bind_group(0, &self.bind_groups[self.source_index], &[]);
            pass.draw(0..self.particle_count.saturating_mul(6), 0..1);
        }
        target.encode_readback(&mut encoder);
        context.queue.submit([encoder.finish()]);
        Ok(target.read_pixels(context)?)
    }

    /// Deterministically seeks to a timeline frame and saves it as PNG.
    ///
    /// # Errors
    ///
    /// Returns an error if rendering, readback, or PNG encoding fails.
    pub fn save_timeline_frame_png(
        &mut self,
        context: &GpuContext,
        target: &OffscreenRenderTarget,
        frame_index: u32,
        timing: SimulationTiming,
        clear: RgbaColor,
        path: impl AsRef<Path>,
    ) -> Result<(), ParticleRenderError> {
        let pixels = self.render_timeline_frame(context, target, frame_index, timing, clear)?;
        Ok(target.save_png(&pixels, path.as_ref())?)
    }

    /// Measures CPU frame preparation and GPU compute/render work without image readback.
    ///
    /// # Errors
    ///
    /// Returns an error if timestamp results cannot be mapped. GPU fields are
    /// `None` on adapters without timestamp-query support.
    #[allow(clippy::cast_precision_loss)]
    pub fn benchmark_frame(
        &mut self,
        context: &GpuContext,
        target: &OffscreenRenderTarget,
        frame_index: u32,
        fps: f32,
        config: BenchmarkConfig,
    ) -> Result<FrameTiming, ParticleRenderError> {
        let started = Instant::now();
        let uniforms = FrameUniforms {
            view_projection: aspect_matrix(target.dimensions()),
            frame_index,
            particle_count: self.particle_count,
            delta_time: 1.0 / fps,
            simulation_time: frame_index as f32 / fps,
            viewport_size: [target.dimensions().0 as f32, target.dimensions().1 as f32],
            particle_size_pixels: config.particle_size_pixels,
            position_scale: config.position_scale,
            simulation_seed: seed_u32(self.seed),
            padding: [0; 3],
        };
        context
            .queue
            .write_buffer(&self.uniform_buffer, 0, bytemuck::bytes_of(&uniforms));
        let view = target.view();
        let mut encoder = context
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("particle-benchmark-frame"),
            });
        {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("particle-benchmark-compute"),
                timestamp_writes: self.timing.as_ref().map(|timing| {
                    wgpu::ComputePassTimestampWrites {
                        query_set: &timing.queries,
                        beginning_of_pass_write_index: Some(0),
                        end_of_pass_write_index: Some(1),
                    }
                }),
            });
            pass.set_pipeline(&self.compute_pipeline);
            pass.set_bind_group(0, &self.bind_groups[self.source_index], &[]);
            let (groups_x, groups_y) = dispatch_dimensions(self.particle_count);
            pass.dispatch_workgroups(groups_x, groups_y, 1);
        }
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("particle-benchmark-render"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: self.timing.as_ref().map(|timing| {
                    wgpu::RenderPassTimestampWrites {
                        query_set: &timing.queries,
                        beginning_of_pass_write_index: Some(2),
                        end_of_pass_write_index: Some(3),
                    }
                }),
                occlusion_query_set: None,
            });
            pass.set_pipeline(&self.render_pipeline);
            pass.set_bind_group(0, &self.bind_groups[self.source_index ^ 1], &[]);
            pass.draw(0..self.particle_count.saturating_mul(6), 0..1);
        }
        if let Some(timing) = &self.timing {
            encoder.resolve_query_set(&timing.queries, 0..4, &timing.resolve, 0);
            encoder.copy_buffer_to_buffer(&timing.resolve, 0, &timing.readback, 0, 32);
        }
        let cpu_prepare_ms = started.elapsed().as_secs_f64() * 1000.0;
        context.queue.submit([encoder.finish()]);
        self.source_index ^= 1;
        let (gpu_compute_ms, gpu_render_ms) = if let Some(timing) = &self.timing {
            let values = read_timestamps(context, &timing.readback)?;
            let period_ms = f64::from(context.queue.get_timestamp_period()) / 1_000_000.0;
            (
                (values[1] - values[0]) as f64 * period_ms,
                (values[3] - values[2]) as f64 * period_ms,
            )
        } else {
            (0.0, 0.0)
        };
        Ok(FrameTiming {
            cpu_prepare_ms,
            gpu_compute_ms: self.timing.as_ref().map(|_| gpu_compute_ms),
            gpu_render_ms: self.timing.as_ref().map(|_| gpu_render_ms),
        })
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
            viewport_size: [target.dimensions().0 as f32, target.dimensions().1 as f32],
            particle_size_pixels: 2.0,
            position_scale: 1.0,
            simulation_seed: seed_u32(self.seed),
            padding: [0; 3],
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
            let (groups_x, groups_y) = dispatch_dimensions(self.particle_count);
            pass.dispatch_workgroups(groups_x, groups_y, 1);
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
            pass.draw(0..self.particle_count.saturating_mul(6), 0..1);
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

fn dispatch_dimensions(particle_count: u32) -> (u32, u32) {
    let groups = particle_count.div_ceil(256);
    let groups_x = groups.min(65_535);
    (groups_x, groups.div_ceil(groups_x))
}

fn seed_u32(seed: u64) -> u32 {
    let bytes = seed.to_le_bytes();
    u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])
}

fn create_timing_resources(context: &GpuContext) -> Option<TimingResources> {
    if !context
        .device
        .features()
        .contains(wgpu::Features::TIMESTAMP_QUERY)
    {
        return None;
    }
    let queries = context.device.create_query_set(&wgpu::QuerySetDescriptor {
        label: Some("particle-timing-queries"),
        ty: wgpu::QueryType::Timestamp,
        count: 4,
    });
    let resolve = context.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("particle-timing-resolve"),
        size: 32,
        usage: wgpu::BufferUsages::QUERY_RESOLVE | wgpu::BufferUsages::COPY_SRC,
        mapped_at_creation: false,
    });
    let readback = context.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("particle-timing-readback"),
        size: 32,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    Some(TimingResources {
        queries,
        resolve,
        readback,
    })
}

fn read_timestamps(
    context: &GpuContext,
    buffer: &wgpu::Buffer,
) -> Result<[u64; 4], ParticleRenderError> {
    let slice = buffer.slice(..);
    let (sender, receiver) = mpsc::channel();
    slice.map_async(wgpu::MapMode::Read, move |result| {
        let _ = sender.send(result);
    });
    let _ = context.device.poll(wgpu::Maintain::Wait);
    receiver
        .recv()
        .map_err(|_| ParticleRenderError::TimingCallbackDropped)??;
    let mapped = slice.get_mapped_range();
    let mut values = [0_u64; 4];
    values.copy_from_slice(bytemuck::cast_slice(&mapped));
    drop(mapped);
    buffer.unmap();
    Ok(values)
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
