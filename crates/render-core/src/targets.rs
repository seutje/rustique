//! Deterministic conversion of shapes and common creative assets to particles.
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    clippy::cast_sign_loss
)]

use std::{
    fs,
    path::{Path, PathBuf},
};

use image::{RgbaImage, imageops::FilterType};
use simulation::{Particle, initialize_particles};
use thiserror::Error;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PrimitiveTarget {
    Sphere,
    Cube,
    Ring,
}

#[derive(Debug, Error)]
pub enum TargetError {
    #[error("failed to import GLTF/GLB mesh {path}: {source}")]
    Gltf { path: PathBuf, source: gltf::Error },
    #[error("mesh {0} contains no POSITION attributes")]
    EmptyMesh(PathBuf),
    #[error("failed to read asset {path}: {source}")]
    Read {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("failed to parse SVG {path}: {message}")]
    Svg { path: PathBuf, message: String },
    #[error("failed to parse font {path}: {message}")]
    Font { path: PathBuf, message: String },
    #[error("target particle count must be greater than zero")]
    EmptyTarget,
    #[error("dissolution must be between 0 and 1")]
    InvalidDissolution,
}

#[must_use]
#[allow(clippy::cast_precision_loss)]
pub fn primitive_points(shape: PrimitiveTarget, count: u32, seed: u64) -> Vec<[f32; 3]> {
    (0..count)
        .map(|index| {
            let u = radical(index.wrapping_add(seed as u32), 2);
            let v = radical(index.wrapping_add((seed >> 32) as u32), 3);
            match shape {
                PrimitiveTarget::Sphere => {
                    let z = 1.0 - 2.0 * u;
                    let radius = (1.0 - z * z).sqrt();
                    let angle = std::f32::consts::TAU * v;
                    [radius * angle.cos(), radius * angle.sin(), z]
                }
                PrimitiveTarget::Cube => {
                    let face = index % 6;
                    let a = u * 2.0 - 1.0;
                    let b = v * 2.0 - 1.0;
                    match face {
                        0 => [1.0, a, b],
                        1 => [-1.0, a, b],
                        2 => [a, 1.0, b],
                        3 => [a, -1.0, b],
                        4 => [a, b, 1.0],
                        _ => [a, b, -1.0],
                    }
                }
                PrimitiveTarget::Ring => {
                    let angle = std::f32::consts::TAU * u;
                    [angle.cos(), angle.sin(), (v - 0.5) * 0.08]
                }
            }
        })
        .collect()
}

/// Reads vertex positions from every primitive in a GLTF or binary GLB file.
///
/// # Errors
///
/// Returns a contextual import error or reports a mesh without positions.
pub fn load_gltf_points(path: impl AsRef<Path>) -> Result<Vec<[f32; 3]>, TargetError> {
    let path = path.as_ref();
    let (document, buffers, _) = gltf::import(path).map_err(|source| TargetError::Gltf {
        path: path.to_owned(),
        source,
    })?;
    let mut points = Vec::new();
    for mesh in document.meshes() {
        for primitive in mesh.primitives() {
            let reader = primitive.reader(|buffer| Some(&buffers[buffer.index()]));
            if let Some(positions) = reader.read_positions() {
                points.extend(positions);
            }
        }
    }
    if points.is_empty() {
        Err(TargetError::EmptyMesh(path.to_owned()))
    } else {
        Ok(normalize_points(points))
    }
}

/// Rasterizes SVG content and samples its visible pixels as a 2D target.
///
/// # Errors
///
/// Returns a contextual read, parse, or raster allocation error.
pub fn load_svg_points(
    path: impl AsRef<Path>,
    resolution: u32,
) -> Result<Vec<[f32; 3]>, TargetError> {
    let path = path.as_ref();
    let data = fs::read(path).map_err(|source| TargetError::Read {
        path: path.to_owned(),
        source,
    })?;
    let tree = usvg::Tree::from_data(&data, &usvg::Options::default()).map_err(|error| {
        TargetError::Svg {
            path: path.to_owned(),
            message: error.to_string(),
        }
    })?;
    let mut pixmap =
        tiny_skia::Pixmap::new(resolution, resolution).ok_or_else(|| TargetError::Svg {
            path: path.to_owned(),
            message: "invalid raster size".into(),
        })?;
    let size = tree.size();
    let scale = (resolution as f32 / size.width()).min(resolution as f32 / size.height());
    resvg::render(
        &tree,
        tiny_skia::Transform::from_scale(scale, scale),
        &mut pixmap.as_mut(),
    );
    Ok(alpha_points(pixmap.data(), resolution, resolution))
}

/// Rasterizes UTF-8 text with a user-supplied TTF/OTF font.
///
/// # Errors
///
/// Returns a contextual font read or parse error.
pub fn load_text_points(
    text: &str,
    font_path: impl AsRef<Path>,
    pixel_size: f32,
) -> Result<Vec<[f32; 3]>, TargetError> {
    let path = font_path.as_ref();
    let data = fs::read(path).map_err(|source| TargetError::Read {
        path: path.to_owned(),
        source,
    })?;
    let font =
        fontdue::Font::from_bytes(data, fontdue::FontSettings::default()).map_err(|message| {
            TargetError::Font {
                path: path.to_owned(),
                message: message.to_owned(),
            }
        })?;
    let glyphs: Vec<_> = text
        .chars()
        .map(|character| font.rasterize(character, pixel_size))
        .collect();
    let width = glyphs
        .iter()
        .map(|(metrics, _)| metrics.advance_width.ceil() as usize)
        .sum::<usize>()
        .max(1);
    let height = pixel_size.ceil() as usize;
    let mut image = RgbaImage::new(width as u32, height as u32);
    let mut x = 0usize;
    for (metrics, bitmap) in glyphs {
        for row in 0..metrics.height {
            for column in 0..metrics.width {
                let alpha = bitmap[row * metrics.width + column];
                if alpha > 32 {
                    image.put_pixel(
                        (x + column) as u32,
                        row as u32,
                        image::Rgba([255, 255, 255, alpha]),
                    );
                }
            }
        }
        x += metrics.advance_width.ceil() as usize;
    }
    let resized = image::imageops::resize(&image, 512, 512, FilterType::Triangle);
    Ok(alpha_points(resized.as_raw(), 512, 512))
}

/// Maps a target point cloud into stable particle state. `dissolution` blends
/// toward the default seeded cloud, providing deterministic mesh dissolution.
///
/// # Errors
///
/// Returns an error for empty input or a dissolution outside `0..=1`.
pub fn particles_from_target(
    points: &[[f32; 3]],
    count: u32,
    seed: u64,
    dissolution: f32,
) -> Result<Vec<Particle>, TargetError> {
    if count == 0 || points.is_empty() {
        return Err(TargetError::EmptyTarget);
    }
    if !dissolution.is_finite() || !(0.0..=1.0).contains(&dissolution) {
        return Err(TargetError::InvalidDissolution);
    }
    let mut particles = initialize_particles(count, seed);
    if dissolution >= 1.0 {
        return Ok(particles);
    }
    for (index, particle) in particles.iter_mut().enumerate() {
        let target = points[index % points.len()];
        for (axis, value) in target.iter().enumerate() {
            particle.position_age[axis] =
                value + (particle.position_age[axis] - value) * dissolution;
        }
    }
    Ok(particles)
}

fn alpha_points(bytes: &[u8], width: u32, height: u32) -> Vec<[f32; 3]> {
    bytes
        .chunks_exact(4)
        .enumerate()
        .filter(|(_, pixel)| pixel[3] > 32)
        .map(|(index, _)| {
            let x = (index as u32) % width;
            let y = (index as u32) / width;
            [
                (x as f32 / width as f32 - 0.5) * 2.0,
                (0.5 - y as f32 / height as f32) * 2.0,
                0.0,
            ]
        })
        .collect()
}
fn normalize_points(mut points: Vec<[f32; 3]>) -> Vec<[f32; 3]> {
    let mut min = [f32::MAX; 3];
    let mut max = [f32::MIN; 3];
    for p in &points {
        for a in 0..3 {
            min[a] = min[a].min(p[a]);
            max[a] = max[a].max(p[a]);
        }
    }
    let center: [f32; 3] = std::array::from_fn(|a| f32::midpoint(min[a], max[a]));
    let scale = (0..3)
        .map(|a| max[a] - min[a])
        .fold(0.0, f32::max)
        .max(f32::EPSILON);
    for p in &mut points {
        for a in 0..3 {
            p[a] = (p[a] - center[a]) * 2.0 / scale;
        }
    }
    points
}
fn radical(mut value: u32, base: u32) -> f32 {
    let mut result = 0.0;
    let mut factor = 1.0 / base as f32;
    while value > 0 {
        result += (value % base) as f32 * factor;
        value /= base;
        factor /= base as f32;
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn targets_are_deterministic_and_dissolve() {
        let points = primitive_points(PrimitiveTarget::Sphere, 16, 4);
        assert_eq!(points, primitive_points(PrimitiveTarget::Sphere, 16, 4));
        let solid = particles_from_target(&points, 16, 9, 0.0).unwrap();
        let gone = particles_from_target(&points, 16, 9, 1.0).unwrap();
        assert_ne!(solid, gone);
        assert_eq!(gone, initialize_particles(16, 9));
    }
}
