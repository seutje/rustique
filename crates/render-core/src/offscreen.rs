use std::{
    fs::File,
    io::BufWriter,
    path::{Path, PathBuf},
    sync::mpsc,
};

use thiserror::Error;

use crate::{GpuContext, PostProcessConfig, post_process::PostProcessor};

const BYTES_PER_PIXEL: u32 = 4;
pub(crate) const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;

/// Linear RGBA values used when clearing an offscreen frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RgbaColor {
    pub red: f64,
    pub green: f64,
    pub blue: f64,
    pub alpha: f64,
}

impl RgbaColor {
    pub const BLACK: Self = Self::new(0.0, 0.0, 0.0, 1.0);

    #[must_use]
    pub const fn new(red: f64, green: f64, blue: f64, alpha: f64) -> Self {
        Self {
            red,
            green,
            blue,
            alpha,
        }
    }
}

impl From<RgbaColor> for wgpu::Color {
    fn from(color: RgbaColor) -> Self {
        Self {
            r: color.red,
            g: color.green,
            b: color.blue,
            a: color.alpha,
        }
    }
}

#[derive(Debug, Error)]
pub enum OffscreenError {
    #[error("render dimensions must both be greater than zero (received {width}x{height})")]
    InvalidDimensions { width: u32, height: u32 },
    #[error("render dimensions {width}x{height} exceed the GPU maximum of {maximum}")]
    DimensionsExceedLimit {
        width: u32,
        height: u32,
        maximum: u32,
    },
    #[error("render dimensions are too large to allocate a readback buffer")]
    SizeOverflow,
    #[error("GPU texture readback callback was dropped")]
    ReadbackCallbackDropped,
    #[error("GPU texture readback failed: {0}")]
    BufferMap(#[from] wgpu::BufferAsyncError),
    #[error("failed to create PNG file {path}: {source}")]
    CreatePng {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to encode PNG file {path}: {source}")]
    EncodePng {
        path: PathBuf,
        #[source]
        source: png::EncodingError,
    },
}

/// A reusable window-free color target and staging buffer.
///
/// The staging buffer uses wgpu's required 256-byte row alignment. Readback
/// removes that padding so callers receive tightly packed RGBA8 pixels.
#[derive(Debug)]
pub struct OffscreenRenderTarget {
    width: u32,
    height: u32,
    padded_bytes_per_row: u32,
    post_processor: PostProcessor,
    _depth: wgpu::Texture,
    depth_view: wgpu::TextureView,
    readback_buffer: wgpu::Buffer,
}

impl OffscreenRenderTarget {
    /// Creates resources that can be reused for every render at this resolution.
    ///
    /// # Errors
    ///
    /// Returns an error for zero, unsupported, or overflowing dimensions.
    pub fn new(context: &GpuContext, width: u32, height: u32) -> Result<Self, OffscreenError> {
        Self::new_with_post_process(context, width, height, PostProcessConfig::default())
    }

    /// Creates a reusable target with an explicit shared post-processing setup.
    ///
    /// # Errors
    ///
    /// Returns an error for zero, unsupported, or overflowing dimensions.
    pub fn new_with_post_process(
        context: &GpuContext,
        width: u32,
        height: u32,
        post_process: PostProcessConfig,
    ) -> Result<Self, OffscreenError> {
        if width == 0 || height == 0 {
            return Err(OffscreenError::InvalidDimensions { width, height });
        }
        let maximum = context.device.limits().max_texture_dimension_2d;
        if width > maximum || height > maximum {
            return Err(OffscreenError::DimensionsExceedLimit {
                width,
                height,
                maximum,
            });
        }

        let unpadded_bytes_per_row = width
            .checked_mul(BYTES_PER_PIXEL)
            .ok_or(OffscreenError::SizeOverflow)?;
        let alignment = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
        let padded_bytes_per_row = unpadded_bytes_per_row
            .checked_add(alignment - 1)
            .ok_or(OffscreenError::SizeOverflow)?
            / alignment
            * alignment;
        let buffer_size = u64::from(padded_bytes_per_row)
            .checked_mul(u64::from(height))
            .ok_or(OffscreenError::SizeOverflow)?;

        let post_processor = PostProcessor::new(context, width, height, post_process);
        let depth = context.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("rustique-scene-depth"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: DEPTH_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
        let depth_view = depth.create_view(&wgpu::TextureViewDescriptor::default());
        let readback_buffer = context.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("rustique-offscreen-readback"),
            size: buffer_size,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });

        Ok(Self {
            width,
            height,
            padded_bytes_per_row,
            post_processor,
            _depth: depth,
            depth_view,
            readback_buffer,
        })
    }

    #[must_use]
    pub const fn dimensions(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    pub(crate) fn view(&self) -> &wgpu::TextureView {
        self.post_processor.scene_view()
    }

    pub(crate) const fn depth_view(&self) -> &wgpu::TextureView {
        &self.depth_view
    }

    #[must_use]
    pub fn output_view(&self) -> wgpu::TextureView {
        self.post_processor
            .output()
            .create_view(&wgpu::TextureViewDescriptor::default())
    }

    pub(crate) fn encode_post_process(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        reset_history: bool,
    ) {
        self.post_processor.encode(encoder, reset_history);
    }

    pub(crate) fn encode_readback(&self, encoder: &mut wgpu::CommandEncoder, reset_history: bool) {
        self.post_processor.encode(encoder, reset_history);
        self.encode_copy_to_readback(encoder);
    }

    fn encode_copy_to_readback(&self, encoder: &mut wgpu::CommandEncoder) {
        encoder.copy_texture_to_buffer(
            self.post_processor.output().as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &self.readback_buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(self.padded_bytes_per_row),
                    rows_per_image: Some(self.height),
                },
            },
            wgpu::Extent3d {
                width: self.width,
                height: self.height,
                depth_or_array_layers: 1,
            },
        );
    }

    pub(crate) fn read_output(&self, context: &GpuContext) -> Result<Vec<u8>, OffscreenError> {
        let mut encoder = context
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("rustique-output-readback"),
            });
        self.encode_copy_to_readback(&mut encoder);
        context.queue.submit([encoder.finish()]);
        self.read_pixels(context)
    }

    /// Returns the bytes allocated by reusable color textures at this resolution.
    #[must_use]
    pub fn allocated_texture_bytes(&self) -> u64 {
        PostProcessor::allocated_texture_bytes(self.width, self.height)
            + u64::from(self.width) * u64::from(self.height) * 4
    }

    pub(crate) fn read_pixels(&self, context: &GpuContext) -> Result<Vec<u8>, OffscreenError> {
        let slice = self.readback_buffer.slice(..);
        let (sender, receiver) = mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |result| {
            let _ = sender.send(result);
        });
        let _ = context.device.poll(wgpu::Maintain::Wait);
        receiver
            .recv()
            .map_err(|_| OffscreenError::ReadbackCallbackDropped)??;

        let mapped = slice.get_mapped_range();
        let unpadded_bytes_per_row = (self.width * BYTES_PER_PIXEL) as usize;
        let mut pixels = Vec::with_capacity(unpadded_bytes_per_row * self.height as usize);
        for row in mapped.chunks_exact(self.padded_bytes_per_row as usize) {
            pixels.extend_from_slice(&row[..unpadded_bytes_per_row]);
        }
        drop(mapped);
        self.readback_buffer.unmap();
        Ok(pixels)
    }

    /// Saves a tightly packed RGBA8 frame.
    ///
    /// # Errors
    ///
    /// Returns contextual file creation or PNG encoding errors.
    pub fn save_png(&self, pixels: &[u8], path: &Path) -> Result<(), OffscreenError> {
        let file = File::create(path).map_err(|source| OffscreenError::CreatePng {
            path: path.to_owned(),
            source,
        })?;
        let mut encoder = png::Encoder::new(BufWriter::new(file), self.width, self.height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder
            .write_header()
            .map_err(|source| OffscreenError::EncodePng {
                path: path.to_owned(),
                source,
            })?;
        writer
            .write_image_data(pixels)
            .map_err(|source| OffscreenError::EncodePng {
                path: path.to_owned(),
                source,
            })
    }

    /// Clears the target and returns tightly packed RGBA8 pixels.
    ///
    /// # Errors
    ///
    /// Returns an error when the GPU cannot map the staging buffer.
    pub fn render_clear(
        &self,
        context: &GpuContext,
        color: RgbaColor,
    ) -> Result<Vec<u8>, OffscreenError> {
        let view = self.view();
        let mut encoder = context
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("rustique-offscreen-clear"),
            });
        {
            let _render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("rustique-offscreen-clear-pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(color.into()),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: self.depth_view(),
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(1.0),
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
            });
        }
        self.encode_readback(&mut encoder, true);
        context.queue.submit([encoder.finish()]);
        self.read_pixels(context)
    }

    /// Renders a clear color and saves it as an RGBA PNG.
    ///
    /// # Errors
    ///
    /// Returns an error when readback, file creation, or PNG encoding fails.
    pub fn save_clear_png(
        &self,
        context: &GpuContext,
        color: RgbaColor,
        path: impl AsRef<Path>,
    ) -> Result<(), OffscreenError> {
        let pixels = self.render_clear(context, color)?;
        let path = path.as_ref();
        self.save_png(&pixels, path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn color_converts_to_wgpu_color() {
        let color = wgpu::Color::from(RgbaColor::new(0.1, 0.2, 0.3, 0.4));
        assert!((color.r - 0.1).abs() < f64::EPSILON);
        assert!((color.g - 0.2).abs() < f64::EPSILON);
        assert!((color.b - 0.3).abs() < f64::EPSILON);
        assert!((color.a - 0.4).abs() < f64::EPSILON);
    }
}
