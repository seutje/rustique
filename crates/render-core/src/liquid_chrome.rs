#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    clippy::cast_sign_loss
)]

use crate::{GpuContext, OffscreenError, OffscreenRenderTarget, RgbaColor};
use bytemuck::{Pod, Zeroable};
use image::ImageReader;
use std::{
    path::{Path, PathBuf},
    time::Instant,
};
use thiserror::Error;
use wgpu::util::DeviceExt;

const SHADER: &str = include_str!("../../../shaders/liquid_chrome/liquid_chrome.wgsl");

#[derive(Clone, Debug)]
pub struct LiquidChromeConfig {
    pub environment: Option<PathBuf>,
    pub roughness: f32,
    pub reflection_intensity: f32,
    pub metallic: f32,
    pub surface_scale: f32,
}
impl Default for LiquidChromeConfig {
    fn default() -> Self {
        Self {
            environment: None,
            roughness: 0.14,
            reflection_intensity: 1.35,
            metallic: 1.0,
            surface_scale: 1.0,
        }
    }
}

#[derive(Debug, Error)]
pub enum LiquidChromeError {
    #[error("failed to load environment map {path}: {source}")]
    Environment {
        path: PathBuf,
        #[source]
        source: image::ImageError,
    },
    #[error(transparent)]
    Offscreen(#[from] OffscreenError),
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Params {
    resolution: [f32; 2],
    time: f32,
    roughness: f32,
    reflection: f32,
    metallic: f32,
    surface_scale: f32,
    _pad: f32,
}

pub struct LiquidChromeRenderer {
    pipeline: wgpu::RenderPipeline,
    group: wgpu::BindGroup,
    params: wgpu::Buffer,
    width: u32,
    height: u32,
    metallic: f32,
}

impl LiquidChromeRenderer {
    /// Creates a headless metaball renderer and loads an optional equirectangular HDRI/image.
    ///
    /// # Errors
    ///
    /// Returns a contextual error when the selected environment cannot be decoded.
    #[allow(clippy::too_many_lines)]
    pub fn new(
        ctx: &GpuContext,
        width: u32,
        height: u32,
        config: &LiquidChromeConfig,
    ) -> Result<Self, LiquidChromeError> {
        let (pixels, ew, eh) = match &config.environment {
            Some(path) => load_environment(path)?,
            None => milky_way_environment(1024, 512),
        };
        let texture = ctx.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("liquid-chrome-environment"),
            size: wgpu::Extent3d {
                width: ew,
                height: eh,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        ctx.queue.write_texture(
            texture.as_image_copy(),
            &pixels,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(ew * 4),
                rows_per_image: Some(eh),
            },
            wgpu::Extent3d {
                width: ew,
                height: eh,
                depth_or_array_layers: 1,
            },
        );
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let sampler = ctx.device.create_sampler(&wgpu::SamplerDescriptor {
            address_mode_u: wgpu::AddressMode::Repeat,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let params = ctx
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("liquid-chrome-params"),
                contents: bytemuck::bytes_of(&Params {
                    resolution: [width as f32, height as f32],
                    time: 0.0,
                    roughness: config.roughness,
                    reflection: config.reflection_intensity,
                    metallic: config.metallic,
                    surface_scale: config.surface_scale,
                    _pad: 0.0,
                }),
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            });
        let layout = ctx
            .device
            .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("liquid-chrome-layout"),
                entries: &[
                    wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            sample_type: wgpu::TextureSampleType::Float { filterable: true },
                            view_dimension: wgpu::TextureViewDimension::D2,
                            multisampled: false,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 1,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 2,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Uniform,
                            has_dynamic_offset: false,
                            min_binding_size: None,
                        },
                        count: None,
                    },
                ],
            });
        let group = ctx.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("liquid-chrome-group"),
            layout: &layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: params.as_entire_binding(),
                },
            ],
        });
        let module = ctx
            .device
            .create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("liquid-chrome-shader"),
                source: wgpu::ShaderSource::Wgsl(SHADER.into()),
            });
        let pipeline_layout = ctx
            .device
            .create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("liquid-chrome-pipeline-layout"),
                bind_group_layouts: &[&layout],
                push_constant_ranges: &[],
            });
        let pipeline = ctx
            .device
            .create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("liquid-chrome-pipeline"),
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
        Ok(Self {
            pipeline,
            group,
            params,
            width,
            height,
            metallic: config.metallic,
        })
    }

    /// Renders one deterministic material state and reads back RGBA8 pixels.
    ///
    /// # Errors
    ///
    /// Returns an error when GPU readback fails.
    #[allow(clippy::too_many_arguments)]
    pub fn render_frame(
        &self,
        ctx: &GpuContext,
        target: &OffscreenRenderTarget,
        time: f32,
        roughness: f32,
        reflection: f32,
        surface_scale: f32,
        clear: RgbaColor,
    ) -> Result<Vec<u8>, LiquidChromeError> {
        let started = Instant::now();
        ctx.queue.write_buffer(
            &self.params,
            0,
            bytemuck::bytes_of(&Params {
                resolution: [self.width as f32, self.height as f32],
                time,
                roughness: roughness.clamp(0.0, 1.0),
                reflection: reflection.max(0.0),
                metallic: self.metallic,
                surface_scale: surface_scale.max(0.05),
                _pad: 0.0,
            }),
        );
        let mut encoder = ctx
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("liquid-chrome-frame"),
            });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("liquid-chrome-pass"),
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
            "liquid chrome frame rendered in {:.2} ms",
            started.elapsed().as_secs_f64() * 1000.0
        );
        Ok(pixels)
    }
    /// Renders one frame and saves it as PNG.
    ///
    /// # Errors
    ///
    /// Returns an error when rendering, readback, or PNG encoding fails.
    #[allow(clippy::too_many_arguments)]
    pub fn save_frame_png(
        &self,
        ctx: &GpuContext,
        target: &OffscreenRenderTarget,
        time: f32,
        roughness: f32,
        reflection: f32,
        surface_scale: f32,
        clear: RgbaColor,
        path: impl AsRef<Path>,
    ) -> Result<(), LiquidChromeError> {
        let pixels = self.render_frame(
            ctx,
            target,
            time,
            roughness,
            reflection,
            surface_scale,
            clear,
        )?;
        target.save_png(&pixels, path.as_ref())?;
        Ok(())
    }
}

fn load_environment(path: &Path) -> Result<(Vec<u8>, u32, u32), LiquidChromeError> {
    let image = ImageReader::open(path)
        .map_err(|source| LiquidChromeError::Environment {
            path: path.to_owned(),
            source: source.into(),
        })?
        .decode()
        .map_err(|source| LiquidChromeError::Environment {
            path: path.to_owned(),
            source,
        })?;
    let rgba = image.into_rgba32f();
    let (w, h) = rgba.dimensions();
    let pixels = rgba
        .pixels()
        .flat_map(|p| {
            p.0.map(|v| ((v.max(0.0) / (1.0 + v.max(0.0))).powf(1.0 / 2.2) * 255.0).round() as u8)
        })
        .collect();
    Ok((pixels, w, h))
}

fn milky_way_environment(w: u32, h: u32) -> (Vec<u8>, u32, u32) {
    let mut out = Vec::with_capacity((w * h * 4) as usize);
    for y in 0..h {
        for x in 0..w {
            let u = x as f32 / w as f32;
            let v = y as f32 / h as f32;
            let band = (-((v - 0.52 - 0.07 * (u * 12.0).sin()) / 0.075).powi(2)).exp();
            let hash = ((x.wrapping_mul(1973) ^ y.wrapping_mul(9277) ^ 0x0001_5c55) & 1023) as f32
                / 1023.0;
            let star = if hash > 0.995 {
                (hash - 0.995) * 160.0
            } else {
                0.0
            };
            out.extend_from_slice(&[
                ((8.0 + 65.0 * band + 255.0 * star).min(255.0)) as u8,
                ((10.0 + 48.0 * band + 230.0 * star).min(255.0)) as u8,
                ((24.0 + 92.0 * band + 255.0 * star).min(255.0)) as u8,
                255,
            ]);
        }
    }
    (out, w, h)
}
