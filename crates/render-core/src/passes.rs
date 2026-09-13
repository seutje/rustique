//! Headless auxiliary render-pass encoding and floating-point EXR output.
#![allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]

use std::{
    fs,
    path::{Path, PathBuf},
};

use image::{ImageBuffer, ImageFormat, Rgba};
use thiserror::Error;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RenderPass {
    Beauty,
    Alpha,
    Depth,
    Normals,
    MotionVectors,
    Emission,
}

impl RenderPass {
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Beauty => "beauty",
            Self::Alpha => "alpha",
            Self::Depth => "depth",
            Self::Normals => "normals",
            Self::MotionVectors => "motion-vectors",
            Self::Emission => "emission",
        }
    }
}

#[derive(Debug, Error)]
pub enum PassError {
    #[error("RGBA byte count {actual} does not match {width}x{height}")]
    InvalidPixels {
        width: u32,
        height: u32,
        actual: usize,
    },
    #[error("failed to create render-pass directory {path}: {source}")]
    CreateDirectory {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("failed to encode {path}: {source}")]
    Encode {
        path: PathBuf,
        source: image::ImageError,
    },
}

/// Saves selected image-space passes beside a beauty frame. Depth and normals
/// are reconstructed from the visible particle field; motion compares the
/// current and previous beauty buffers. This keeps the experimental path
/// renderer-independent while producing deterministic compositing assets.
///
/// # Errors
///
/// Returns an error for mismatched buffers or failed directory/image writes.
pub fn save_render_passes(
    pixels: &[u8],
    previous: Option<&[u8]>,
    width: u32,
    height: u32,
    directory: impl AsRef<Path>,
    stem: &str,
    passes: &[RenderPass],
) -> Result<Vec<PathBuf>, PassError> {
    validate(pixels, width, height)?;
    if let Some(previous) = previous {
        validate(previous, width, height)?;
    }
    let directory = directory.as_ref();
    fs::create_dir_all(directory).map_err(|source| PassError::CreateDirectory {
        path: directory.to_owned(),
        source,
    })?;
    let mut outputs = Vec::new();
    for pass in passes {
        let bytes = derive_pass(*pass, pixels, previous, width, height);
        let path = directory.join(format!("{stem}.{}.png", pass.name()));
        image::save_buffer_with_format(
            &path,
            &bytes,
            width,
            height,
            image::ColorType::Rgba8,
            ImageFormat::Png,
        )
        .map_err(|source| PassError::Encode {
            path: path.clone(),
            source,
        })?;
        outputs.push(path);
    }
    Ok(outputs)
}

/// Writes linear floating-point RGBA data to an `OpenEXR` image.
///
/// # Errors
///
/// Returns an error for a mismatched buffer or failed image encoding.
pub fn save_exr(
    pixels: &[u8],
    width: u32,
    height: u32,
    path: impl AsRef<Path>,
) -> Result<(), PassError> {
    validate(pixels, width, height)?;
    let path = path.as_ref();
    let floats: Vec<f32> = pixels
        .chunks_exact(4)
        .flat_map(|p| p.iter().map(|v| f32::from(*v) / 255.0))
        .collect();
    let image: ImageBuffer<Rgba<f32>, Vec<f32>> = ImageBuffer::from_raw(width, height, floats)
        .ok_or(PassError::InvalidPixels {
            width,
            height,
            actual: pixels.len(),
        })?;
    image
        .save_with_format(path, ImageFormat::OpenExr)
        .map_err(|source| PassError::Encode {
            path: path.to_owned(),
            source,
        })
}

fn validate(pixels: &[u8], width: u32, height: u32) -> Result<(), PassError> {
    let expected = width as usize * height as usize * 4;
    if pixels.len() == expected {
        Ok(())
    } else {
        Err(PassError::InvalidPixels {
            width,
            height,
            actual: pixels.len(),
        })
    }
}
fn derive_pass(
    pass: RenderPass,
    pixels: &[u8],
    previous: Option<&[u8]>,
    width: u32,
    height: u32,
) -> Vec<u8> {
    let mut out = vec![0; pixels.len()];
    for y in 0..height {
        for x in 0..width {
            let i = ((y * width + x) * 4) as usize;
            let pixel = &pixels[i..i + 4];
            let luminance = luma(pixel);
            let rgba = match pass {
                RenderPass::Beauty => [pixel[0], pixel[1], pixel[2], pixel[3]],
                RenderPass::Alpha => [pixel[3], pixel[3], pixel[3], 255],
                RenderPass::Depth => {
                    let value = ((1.0 - luminance) * 255.0) as u8;
                    [value, value, value, pixel[3]]
                }
                RenderPass::Emission => {
                    let value = ((luminance - 0.6).max(0.0) * 2.5 * 255.0).min(255.0) as u8;
                    [value, value, value, pixel[3]]
                }
                RenderPass::Normals => {
                    let nx = (luma_at(pixels, width, height, x.saturating_sub(1), y)
                        - luma_at(pixels, width, height, (x + 1).min(width - 1), y))
                        * 0.5;
                    let ny = (luma_at(pixels, width, height, x, y.saturating_sub(1))
                        - luma_at(pixels, width, height, x, (y + 1).min(height - 1)))
                        * 0.5;
                    let inverse = (nx * nx + ny * ny + 1.0).sqrt().recip();
                    [
                        ((nx * inverse * 0.5 + 0.5) * 255.0) as u8,
                        ((ny * inverse * 0.5 + 0.5) * 255.0) as u8,
                        ((inverse * 0.5 + 0.5) * 255.0) as u8,
                        pixel[3],
                    ]
                }
                RenderPass::MotionVectors => {
                    let old = previous.map_or(luminance, |value| luma(&value[i..i + 4]));
                    let delta = (luminance - old).clamp(-1.0, 1.0);
                    [((delta * 0.5 + 0.5) * 255.0) as u8, 127, 0, pixel[3]]
                }
            };
            out[i..i + 4].copy_from_slice(&rgba);
        }
    }
    out
}
fn luma(pixel: &[u8]) -> f32 {
    (0.2126 * f32::from(pixel[0]) + 0.7152 * f32::from(pixel[1]) + 0.0722 * f32::from(pixel[2]))
        / 255.0
}
fn luma_at(pixels: &[u8], width: u32, _height: u32, x: u32, y: u32) -> f32 {
    let i = ((y * width + x) * 4) as usize;
    luma(&pixels[i..i + 4])
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn all_passes_have_rgba_shape() {
        let pixels = [255, 128, 0, 64, 0, 0, 0, 0];
        for pass in [
            RenderPass::Beauty,
            RenderPass::Alpha,
            RenderPass::Depth,
            RenderPass::Normals,
            RenderPass::MotionVectors,
            RenderPass::Emission,
        ] {
            assert_eq!(derive_pass(pass, &pixels, None, 2, 1).len(), pixels.len());
        }
    }
}
