use bytemuck::{Pod, Zeroable};
use simulation::initialize_particles;
use wgpu::util::DeviceExt;

use crate::{
    GpuContext, OffscreenError, OffscreenRenderTarget, RgbaColor, post_process::HDR_FORMAT,
};
use std::path::Path;

const SHADER: &str = include_str!("../../../shaders/volumetrics/volume.wgsl");

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum VolumetricQuality {
    Draft,
    #[default]
    Preview,
    Final,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct VolumetricConfig {
    pub grid_size: u32,
    pub ray_steps: u32,
    pub half_resolution: bool,
    pub density_scale: f32,
    pub absorption: f32,
    pub emission: f32,
}

impl VolumetricConfig {
    #[must_use]
    pub const fn for_quality(quality: VolumetricQuality) -> Self {
        match quality {
            VolumetricQuality::Draft => Self {
                grid_size: 32,
                ray_steps: 24,
                half_resolution: true,
                density_scale: 1.2,
                absorption: 8.0,
                emission: 1.2,
            },
            VolumetricQuality::Preview => Self {
                grid_size: 48,
                ray_steps: 48,
                half_resolution: true,
                density_scale: 0.9,
                absorption: 8.0,
                emission: 1.35,
            },
            VolumetricQuality::Final => Self {
                grid_size: 96,
                ray_steps: 128,
                half_resolution: false,
                density_scale: 0.6,
                absorption: 8.0,
                emission: 1.5,
            },
        }
    }
}

impl Default for VolumetricConfig {
    fn default() -> Self {
        Self::for_quality(VolumetricQuality::Preview)
    }
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Uniforms {
    grid_size: u32,
    particle_count: u32,
    ray_steps: u32,
    pixel_scale: u32,
    time: f32,
    density_scale: f32,
    absorption: f32,
    emission: f32,
    dimensions: [f32; 2],
    padding: [u32; 2],
}

pub struct VolumetricRenderer {
    particle_count: u32,
    uniforms: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
    clear_pipeline: wgpu::ComputePipeline,
    splat_pipeline: wgpu::ComputePipeline,
    raymarch_pipeline: wgpu::RenderPipeline,
    config: VolumetricConfig,
}

impl VolumetricRenderer {
    #[must_use]
    #[allow(clippy::too_many_lines)]
    pub fn new(
        context: &GpuContext,
        particle_count: u32,
        seed: u64,
        config: VolumetricConfig,
    ) -> Self {
        let particles = initialize_particles(particle_count, seed);
        let particle_buffer =
            context
                .device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("volume-particles"),
                    contents: bytemuck::cast_slice(&particles),
                    usage: wgpu::BufferUsages::STORAGE,
                });
        let voxel_count = u64::from(config.grid_size).pow(3);
        let density = context.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("volume-density-grid"),
            size: voxel_count * 4,
            usage: wgpu::BufferUsages::STORAGE,
            mapped_at_creation: false,
        });
        let uniforms = context.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("volume-uniforms"),
            size: size_of::<Uniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let layout = context
            .device
            .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("volume-layout"),
                entries: &[
                    storage_entry(0, true),
                    storage_entry(1, false),
                    wgpu::BindGroupLayoutEntry {
                        binding: 2,
                        visibility: wgpu::ShaderStages::COMPUTE | wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Uniform,
                            has_dynamic_offset: false,
                            min_binding_size: None,
                        },
                        count: None,
                    },
                ],
            });
        let bind_group = context
            .device
            .create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("volume-bind-group"),
                layout: &layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: particle_buffer.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: density.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: uniforms.as_entire_binding(),
                    },
                ],
            });
        let shader = context
            .device
            .create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("volume-shader"),
                source: wgpu::ShaderSource::Wgsl(SHADER.into()),
            });
        let pipeline_layout =
            context
                .device
                .create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                    label: Some("volume-pipeline-layout"),
                    bind_group_layouts: &[&layout],
                    push_constant_ranges: &[],
                });
        let compute = |entry| {
            context
                .device
                .create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                    label: Some(entry),
                    layout: Some(&pipeline_layout),
                    module: &shader,
                    entry_point: Some(entry),
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                    cache: None,
                })
        };
        let clear_pipeline = compute("clear_density");
        let splat_pipeline = compute("splat_particles");
        let raymarch_pipeline =
            context
                .device
                .create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                    label: Some("volume-raymarch"),
                    layout: Some(&pipeline_layout),
                    vertex: wgpu::VertexState {
                        module: &shader,
                        entry_point: Some("fullscreen"),
                        compilation_options: wgpu::PipelineCompilationOptions::default(),
                        buffers: &[],
                    },
                    fragment: Some(wgpu::FragmentState {
                        module: &shader,
                        entry_point: Some("raymarch"),
                        compilation_options: wgpu::PipelineCompilationOptions::default(),
                        targets: &[Some(wgpu::ColorTargetState {
                            format: HDR_FORMAT,
                            blend: None,
                            write_mask: wgpu::ColorWrites::ALL,
                        })],
                    }),
                    primitive: wgpu::PrimitiveState::default(),
                    depth_stencil: None,
                    multisample: wgpu::MultisampleState::default(),
                    multiview: None,
                    cache: None,
                });
        Self {
            particle_count,
            uniforms,
            bind_group,
            clear_pipeline,
            splat_pipeline,
            raymarch_pipeline,
            config,
        }
    }

    /// Splats the deterministic particle set and raymarches one frame.
    ///
    /// # Errors
    /// Returns an error if GPU readback fails.
    #[allow(clippy::cast_precision_loss)]
    pub fn render_frame(
        &self,
        context: &GpuContext,
        target: &OffscreenRenderTarget,
        frame: u32,
        fps: u32,
        clear: RgbaColor,
    ) -> Result<Vec<u8>, OffscreenError> {
        let dimensions = target.dimensions();
        let values = Uniforms {
            grid_size: self.config.grid_size,
            particle_count: self.particle_count,
            ray_steps: self.config.ray_steps,
            pixel_scale: if self.config.half_resolution { 2 } else { 1 },
            time: frame as f32 / fps as f32,
            density_scale: self.config.density_scale,
            absorption: self.config.absorption,
            emission: self.config.emission,
            dimensions: [dimensions.0 as f32, dimensions.1 as f32],
            padding: [0; 2],
        };
        context
            .queue
            .write_buffer(&self.uniforms, 0, bytemuck::bytes_of(&values));
        let mut encoder = context
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("volume-frame"),
            });
        for (pipeline, count) in [
            (&self.clear_pipeline, self.config.grid_size.pow(3)),
            (&self.splat_pipeline, self.particle_count),
        ] {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("volume-compute"),
                timestamp_writes: None,
            });
            pass.set_pipeline(pipeline);
            pass.set_bind_group(0, &self.bind_group, &[]);
            pass.dispatch_workgroups(count.div_ceil(256), 1, 1);
        }
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("volume-raymarch-pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: target.view(),
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
            pass.set_pipeline(&self.raymarch_pipeline);
            pass.set_bind_group(0, &self.bind_group, &[]);
            pass.draw(0..3, 0..1);
        }
        target.encode_readback(&mut encoder, frame == 0);
        context.queue.submit([encoder.finish()]);
        target.read_pixels(context)
    }

    /// Renders and saves one volumetric frame.
    ///
    /// # Errors
    /// Returns an error if GPU readback or PNG encoding fails.
    pub fn save_frame_png(
        &self,
        context: &GpuContext,
        target: &OffscreenRenderTarget,
        frame: u32,
        fps: u32,
        clear: RgbaColor,
        path: impl AsRef<Path>,
    ) -> Result<(), OffscreenError> {
        let pixels = self.render_frame(context, target, frame, fps, clear)?;
        target.save_png(&pixels, path.as_ref())
    }
}

fn storage_entry(binding: u32, read_only: bool) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::COMPUTE | wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Storage { read_only },
            has_dynamic_offset: false,
            min_binding_size: None,
        },
        count: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn quality_scales_independently() {
        let preview = VolumetricConfig::for_quality(VolumetricQuality::Preview);
        let final_quality = VolumetricConfig::for_quality(VolumetricQuality::Final);
        assert!(preview.half_resolution);
        assert!(!final_quality.half_resolution);
        assert!(final_quality.ray_steps > preview.ray_steps);
        assert!(final_quality.grid_size > preview.grid_size);
    }
}
