use std::cell::Cell;

use bytemuck::{Pod, Zeroable};
use wgpu::util::DeviceExt;

use crate::{GpuContext, RgbaColor};

pub(crate) const HDR_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;
pub(crate) const OUTPUT_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;
const SHADER: &str = include_str!("../../../shaders/post/post_process.wgsl");

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum PostProcessQuality {
    Draft,
    #[default]
    Preview,
    Final,
}

#[derive(Clone, Copy, Debug, PartialEq)]
#[allow(clippy::struct_excessive_bools)]
pub struct PostProcessConfig {
    pub tone_mapping: bool,
    pub bloom: bool,
    pub trails: bool,
    pub vignette: bool,
    pub chromatic_aberration: bool,
    pub exposure: f32,
    pub gamma: f32,
    pub bloom_strength: f32,
    pub bloom_threshold: f32,
    pub trail_decay: f32,
    pub vignette_strength: f32,
    pub chromatic_aberration_pixels: f32,
}

impl PostProcessConfig {
    #[must_use]
    pub const fn for_quality(quality: PostProcessQuality) -> Self {
        match quality {
            PostProcessQuality::Draft => Self {
                tone_mapping: true,
                bloom: false,
                trails: false,
                vignette: false,
                chromatic_aberration: false,
                exposure: 1.0,
                gamma: 2.2,
                bloom_strength: 0.0,
                bloom_threshold: 1.0,
                trail_decay: 0.0,
                vignette_strength: 0.0,
                chromatic_aberration_pixels: 0.0,
            },
            PostProcessQuality::Preview => Self {
                tone_mapping: true,
                bloom: true,
                trails: false,
                vignette: false,
                chromatic_aberration: false,
                exposure: 1.0,
                gamma: 2.2,
                bloom_strength: 0.2,
                bloom_threshold: 0.8,
                trail_decay: 0.0,
                vignette_strength: 0.0,
                chromatic_aberration_pixels: 0.0,
            },
            PostProcessQuality::Final => Self {
                tone_mapping: true,
                bloom: true,
                trails: true,
                vignette: true,
                chromatic_aberration: false,
                exposure: 1.0,
                gamma: 2.2,
                bloom_strength: 0.3,
                bloom_threshold: 0.7,
                trail_decay: 0.82,
                vignette_strength: 0.2,
                chromatic_aberration_pixels: 0.0,
            },
        }
    }
}

impl Default for PostProcessConfig {
    fn default() -> Self {
        Self::for_quality(PostProcessQuality::Preview)
    }
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct PostUniforms {
    dimensions: [f32; 2],
    exposure: f32,
    inverse_gamma: f32,
    bloom_strength: f32,
    bloom_threshold: f32,
    trail_decay: f32,
    vignette_strength: f32,
    chromatic_pixels: f32,
    tone_mapping: f32,
    padding: [f32; 2],
}

const _: () = assert!(size_of::<PostUniforms>() == 48);

#[derive(Debug)]
pub(crate) struct PostProcessor {
    _scene: wgpu::Texture,
    scene_view: wgpu::TextureView,
    output: wgpu::Texture,
    _history: [wgpu::Texture; 2],
    history_views: [wgpu::TextureView; 2],
    accumulation_pipeline: wgpu::RenderPipeline,
    output_pipeline: wgpu::RenderPipeline,
    accumulation_bind_groups: [wgpu::BindGroup; 2],
    output_bind_groups: [wgpu::BindGroup; 2],
    history_index: Cell<usize>,
    history_valid: Cell<bool>,
}

impl PostProcessor {
    #[allow(clippy::cast_precision_loss, clippy::too_many_lines)]
    pub(crate) fn new(
        context: &GpuContext,
        width: u32,
        height: u32,
        config: PostProcessConfig,
    ) -> Self {
        let size = wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        };
        let hdr_texture = |label| {
            context.device.create_texture(&wgpu::TextureDescriptor {
                label: Some(label),
                size,
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: HDR_FORMAT,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                    | wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            })
        };
        let scene = hdr_texture("rustique-hdr-scene");
        let scene_view = scene.create_view(&wgpu::TextureViewDescriptor::default());
        let history = [
            hdr_texture("rustique-trail-history-a"),
            hdr_texture("rustique-trail-history-b"),
        ];
        let history_views = history
            .each_ref()
            .map(|texture| texture.create_view(&wgpu::TextureViewDescriptor::default()));
        let output = context.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("rustique-post-output"),
            size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: OUTPUT_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let uniforms = PostUniforms {
            dimensions: [width as f32, height as f32],
            exposure: config.exposure.max(0.0),
            inverse_gamma: if config.gamma > 0.0 {
                1.0 / config.gamma
            } else {
                1.0
            },
            bloom_strength: if config.bloom {
                config.bloom_strength.max(0.0)
            } else {
                0.0
            },
            bloom_threshold: config.bloom_threshold.max(0.0),
            trail_decay: if config.trails {
                config.trail_decay.clamp(0.0, 0.999)
            } else {
                0.0
            },
            vignette_strength: if config.vignette {
                config.vignette_strength.clamp(0.0, 1.0)
            } else {
                0.0
            },
            chromatic_pixels: if config.chromatic_aberration {
                config.chromatic_aberration_pixels.max(0.0)
            } else {
                0.0
            },
            tone_mapping: f32::from(config.tone_mapping),
            padding: [0.0; 2],
        };
        let uniform_buffer = context
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("rustique-post-uniforms"),
                contents: bytemuck::bytes_of(&uniforms),
                usage: wgpu::BufferUsages::UNIFORM,
            });
        let layout = context
            .device
            .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("rustique-post-layout"),
                entries: &[
                    texture_entry(0),
                    texture_entry(1),
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
        let shader = context
            .device
            .create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("rustique-post-shader"),
                source: wgpu::ShaderSource::Wgsl(SHADER.into()),
            });
        let pipeline_layout =
            context
                .device
                .create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                    label: Some("rustique-post-pipeline-layout"),
                    bind_group_layouts: &[&layout],
                    push_constant_ranges: &[],
                });
        let accumulation_pipeline = pipeline(
            &context.device,
            &pipeline_layout,
            &shader,
            "accumulate",
            HDR_FORMAT,
            "rustique-trail-pipeline",
        );
        let output_pipeline = pipeline(
            &context.device,
            &pipeline_layout,
            &shader,
            "finish",
            OUTPUT_FORMAT,
            "rustique-output-pipeline",
        );
        let bind_group = |label, first: &wgpu::TextureView, second: &wgpu::TextureView| {
            context
                .device
                .create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some(label),
                    layout: &layout,
                    entries: &[
                        wgpu::BindGroupEntry {
                            binding: 0,
                            resource: wgpu::BindingResource::TextureView(first),
                        },
                        wgpu::BindGroupEntry {
                            binding: 1,
                            resource: wgpu::BindingResource::TextureView(second),
                        },
                        wgpu::BindGroupEntry {
                            binding: 2,
                            resource: uniform_buffer.as_entire_binding(),
                        },
                    ],
                })
        };
        let accumulation_bind_groups = [
            bind_group("rustique-accumulate-to-a", &scene_view, &history_views[1]),
            bind_group("rustique-accumulate-to-b", &scene_view, &history_views[0]),
        ];
        let output_bind_groups = [
            bind_group("rustique-output-from-a", &scene_view, &history_views[0]),
            bind_group("rustique-output-from-b", &scene_view, &history_views[1]),
        ];
        Self {
            _scene: scene,
            scene_view,
            output,
            _history: history,
            history_views,
            accumulation_pipeline,
            output_pipeline,
            accumulation_bind_groups,
            output_bind_groups,
            history_index: Cell::new(0),
            history_valid: Cell::new(false),
        }
    }

    pub(crate) fn scene_view(&self) -> &wgpu::TextureView {
        &self.scene_view
    }

    pub(crate) fn output(&self) -> &wgpu::Texture {
        &self.output
    }

    pub(crate) fn encode(&self, encoder: &mut wgpu::CommandEncoder, reset: bool) {
        if reset || !self.history_valid.get() {
            for view in &self.history_views {
                clear_view(encoder, view);
            }
            self.history_index.set(0);
            self.history_valid.set(true);
        }
        let index = self.history_index.get();
        draw_fullscreen(
            encoder,
            &self.history_views[index],
            &self.accumulation_pipeline,
            &self.accumulation_bind_groups[index],
            "rustique-trail-pass",
        );
        let output_view = self
            .output
            .create_view(&wgpu::TextureViewDescriptor::default());
        draw_fullscreen(
            encoder,
            &output_view,
            &self.output_pipeline,
            &self.output_bind_groups[index],
            "rustique-output-pass",
        );
        self.history_index.set(index ^ 1);
    }

    pub(crate) fn allocated_texture_bytes(width: u32, height: u32) -> u64 {
        u64::from(width) * u64::from(height) * (8 * 3 + 4)
    }
}

fn texture_entry(binding: u32) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Texture {
            sample_type: wgpu::TextureSampleType::Float { filterable: false },
            view_dimension: wgpu::TextureViewDimension::D2,
            multisampled: false,
        },
        count: None,
    }
}

fn pipeline(
    device: &wgpu::Device,
    layout: &wgpu::PipelineLayout,
    shader: &wgpu::ShaderModule,
    entry: &str,
    format: wgpu::TextureFormat,
    label: &str,
) -> wgpu::RenderPipeline {
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some(label),
        layout: Some(layout),
        vertex: wgpu::VertexState {
            module: shader,
            entry_point: Some("fullscreen"),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            buffers: &[],
        },
        fragment: Some(wgpu::FragmentState {
            module: shader,
            entry_point: Some(entry),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            targets: &[Some(format.into())],
        }),
        primitive: wgpu::PrimitiveState::default(),
        depth_stencil: None,
        multisample: wgpu::MultisampleState::default(),
        multiview: None,
        cache: None,
    })
}

fn draw_fullscreen(
    encoder: &mut wgpu::CommandEncoder,
    target: &wgpu::TextureView,
    pipeline: &wgpu::RenderPipeline,
    bind_group: &wgpu::BindGroup,
    label: &str,
) {
    let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some(label),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view: target,
            resolve_target: None,
            ops: wgpu::Operations {
                load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                store: wgpu::StoreOp::Store,
            },
        })],
        depth_stencil_attachment: None,
        timestamp_writes: None,
        occlusion_query_set: None,
    });
    pass.set_pipeline(pipeline);
    pass.set_bind_group(0, bind_group, &[]);
    pass.draw(0..3, 0..1);
}

fn clear_view(encoder: &mut wgpu::CommandEncoder, view: &wgpu::TextureView) {
    let _pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("rustique-clear-history"),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view,
            resolve_target: None,
            ops: wgpu::Operations {
                load: wgpu::LoadOp::Clear(RgbaColor::BLACK.into()),
                store: wgpu::StoreOp::Store,
            },
        })],
        depth_stencil_attachment: None,
        timestamp_writes: None,
        occlusion_query_set: None,
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quality_presets_scale_effects() {
        let draft = PostProcessConfig::for_quality(PostProcessQuality::Draft);
        let final_quality = PostProcessConfig::for_quality(PostProcessQuality::Final);
        assert!(!draft.bloom);
        assert!(final_quality.bloom && final_quality.trails && final_quality.vignette);
    }

    #[test]
    fn effects_are_independently_toggleable() {
        let config = PostProcessConfig {
            bloom: false,
            trails: true,
            vignette: true,
            chromatic_aberration: true,
            ..PostProcessConfig::default()
        };
        assert!(!config.bloom);
        assert!(config.trails && config.vignette && config.chromatic_aberration);
    }
}
