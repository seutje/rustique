#![allow(clippy::cast_possible_truncation, clippy::cast_precision_loss)]

use crate::{GpuContext, OffscreenError, OffscreenRenderTarget, RgbaColor};
use bytemuck::{Pod, Zeroable};
use std::{path::Path, time::Instant};
use thiserror::Error;
use wgpu::util::DeviceExt;

const SHADER: &str = include_str!("../../../shaders/water_droplets/water_droplets.wgsl");

#[derive(Clone, Copy, Debug)]
pub struct WaterDropletConfig {
    pub seed: u64,
    pub density: f32,
    pub size: f32,
    pub size_variation: f32,
    pub refraction_strength: f32,
    pub fresnel_strength: f32,
    pub gravity: f32,
    pub emission: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WaterDropletModulation {
    pub emission: f32,
    pub density: f32,
    pub size: f32,
    pub refraction: f32,
    pub gravity: f32,
    pub brightness: f32,
}

impl Default for WaterDropletModulation {
    fn default() -> Self {
        Self {
            emission: 0.0,
            density: 1.0,
            size: 1.0,
            refraction: 1.0,
            gravity: 1.0,
            brightness: 1.0,
        }
    }
}

#[derive(Debug, Error)]
pub enum WaterDropletError {
    #[error(transparent)]
    Offscreen(#[from] OffscreenError),
}

/// Rust/WGSL layout: four 16-byte rows. Keep in sync with `Params` in the shader.
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Params {
    resolution_time: [f32; 4],
    appearance: [f32; 4],
    motion: [f32; 4],
    background: [f32; 4],
}

pub struct WaterDropletRenderer {
    pipeline: wgpu::RenderPipeline,
    group: wgpu::BindGroup,
    params: wgpu::Buffer,
    width: u32,
    height: u32,
    config: WaterDropletConfig,
}

impl WaterDropletRenderer {
    #[must_use]
    pub fn new(ctx: &GpuContext, width: u32, height: u32, config: WaterDropletConfig) -> Self {
        let params = ctx
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("water-droplet-params"),
                contents: bytemuck::bytes_of(&Params::zeroed()),
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            });
        let layout = ctx
            .device
            .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("water-droplet-layout"),
                entries: &[wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                }],
            });
        let group = ctx.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("water-droplet-group"),
            layout: &layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: params.as_entire_binding(),
            }],
        });
        let module = ctx
            .device
            .create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("water-droplet-shader"),
                source: wgpu::ShaderSource::Wgsl(SHADER.into()),
            });
        let pipeline_layout = ctx
            .device
            .create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("water-droplet-pipeline-layout"),
                bind_group_layouts: &[&layout],
                push_constant_ranges: &[],
            });
        let pipeline = ctx
            .device
            .create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("water-droplet-pipeline"),
                layout: Some(&pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &module,
                    entry_point: Some("vs_main"),
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                    buffers: &[],
                },
                fragment: Some(wgpu::FragmentState {
                    module: &module,
                    entry_point: Some("fs_main"),
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: wgpu::TextureFormat::Rgba16Float,
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
            pipeline,
            group,
            params,
            width,
            height,
            config,
        }
    }

    /// Renders and reads back one deterministic droplet frame.
    ///
    /// # Errors
    ///
    /// Returns an error if GPU readback fails.
    pub fn render_frame(
        &self,
        ctx: &GpuContext,
        target: &OffscreenRenderTarget,
        time: f32,
        emission_modulation: f32,
        clear: RgbaColor,
    ) -> Result<Vec<u8>, WaterDropletError> {
        self.render_frame_modulated(
            ctx,
            target,
            time,
            WaterDropletModulation {
                emission: emission_modulation,
                ..WaterDropletModulation::default()
            },
            clear,
        )
    }

    /// Renders a frame with deterministic per-frame audio modulation.
    ///
    /// # Errors
    /// Returns an error if GPU readback fails.
    pub fn render_frame_modulated(
        &self,
        ctx: &GpuContext,
        target: &OffscreenRenderTarget,
        time: f32,
        modulation: WaterDropletModulation,
        clear: RgbaColor,
    ) -> Result<Vec<u8>, WaterDropletError> {
        let started = Instant::now();
        ctx.queue.write_buffer(
            &self.params,
            0,
            bytemuck::bytes_of(&Params {
                resolution_time: [self.width as f32, self.height as f32, time, 0.0],
                appearance: [
                    (self.config.density * modulation.density).clamp(0.0, 1.0),
                    (self.config.size * modulation.size).max(0.01),
                    self.config.size_variation,
                    (self.config.refraction_strength * modulation.refraction).max(0.0),
                ],
                motion: [
                    self.config.fresnel_strength,
                    (self.config.gravity * modulation.gravity).max(0.0),
                    (self.config.emission + modulation.emission).clamp(0.0, 1.0),
                    self.config.seed as f32,
                ],
                background: [
                    clear.red as f32,
                    clear.green as f32,
                    clear.blue as f32,
                    modulation.brightness.max(0.0),
                ],
            }),
        );
        let mut encoder = ctx
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("water-droplet-frame"),
            });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("water-droplet-pass"),
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
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &self.group, &[]);
            pass.draw(0..3, 0..1);
        }
        target.encode_readback(&mut encoder, true);
        ctx.queue.submit([encoder.finish()]);
        let pixels = target.read_pixels(ctx)?;
        log::debug!(
            "water droplet frame rendered in {:.2} ms",
            started.elapsed().as_secs_f64() * 1000.0
        );
        Ok(pixels)
    }

    /// Renders one deterministic droplet frame and writes it as PNG.
    ///
    /// # Errors
    ///
    /// Returns an error if GPU readback or PNG encoding fails.
    pub fn save_frame_png(
        &self,
        ctx: &GpuContext,
        target: &OffscreenRenderTarget,
        time: f32,
        emission_modulation: f32,
        clear: RgbaColor,
        path: impl AsRef<Path>,
    ) -> Result<(), WaterDropletError> {
        let pixels = self.render_frame(ctx, target, time, emission_modulation, clear)?;
        target.save_png(&pixels, path.as_ref())?;
        Ok(())
    }
}
