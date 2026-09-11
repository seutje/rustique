use std::{
    fs::File,
    io::BufWriter,
    path::{Path, PathBuf},
    sync::mpsc,
};

use thiserror::Error;

use crate::GpuContext;

const BYTES_PER_PIXEL: u32 = 4;

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
    texture: wgpu::Texture,
    readback_buffer: wgpu::Buffer,
}

impl OffscreenRenderTarget {
    /// Creates resources that can be reused for every render at this resolution.
    ///
    /// # Errors
    ///
    /// Returns an error for zero, unsupported, or overflowing dimensions.
    pub fn new(context: &GpuContext, width: u32, height: u32) -> Result<Self, OffscreenError> {
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

        let texture = context.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("rustique-offscreen-color"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
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
            texture,
            readback_buffer,
        })
    }

    #[must_use]
    pub const fn dimensions(&self) -> (u32, u32) {
        (self.width, self.height)
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
        let view = self
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = context
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("rustique-offscreen-clear"),
            });
        {
            let _render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("rustique-offscreen-clear-pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(color.into()),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
        }
        encoder.copy_texture_to_buffer(
            self.texture.as_image_copy(),
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
        context.queue.submit([encoder.finish()]);

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
            .write_image_data(&pixels)
            .map_err(|source| OffscreenError::EncodePng {
                path: path.to_owned(),
                source,
            })
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
