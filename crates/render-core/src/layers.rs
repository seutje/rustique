#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LayerBlendMode {
    Alpha,
    Add,
    Screen,
}

/// Composites tightly packed RGBA8 layers in back-to-front order.
/// The operation is deterministic and keeps only one accumulated frame in memory.
///
/// # Panics
///
/// Panics when source and destination have different lengths.
#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
pub fn composite_rgba8(destination: &mut [u8], source: &[u8], blend: LayerBlendMode, opacity: f32) {
    assert_eq!(destination.len(), source.len());
    let opacity = opacity.clamp(0.0, 1.0);
    for (dst, src) in destination.chunks_exact_mut(4).zip(source.chunks_exact(4)) {
        let source_alpha = f32::from(src[3]) / 255.0 * opacity;
        for channel in 0..3 {
            let d = f32::from(dst[channel]) / 255.0;
            let s = f32::from(src[channel]) / 255.0;
            let value = match blend {
                LayerBlendMode::Alpha => d + (s - d) * source_alpha,
                LayerBlendMode::Add => d + s * source_alpha,
                LayerBlendMode::Screen => 1.0 - (1.0 - d) * (1.0 - s * source_alpha),
            };
            dst[channel] = (value.clamp(0.0, 1.0) * 255.0).round() as u8;
        }
        dst[3] = 255;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn alpha_and_add_blends_are_stable() {
        let mut alpha = [0, 0, 0, 255];
        composite_rgba8(&mut alpha, &[200, 100, 0, 255], LayerBlendMode::Alpha, 0.5);
        assert_eq!(alpha, [100, 50, 0, 255]);
        composite_rgba8(&mut alpha, &[200, 0, 0, 255], LayerBlendMode::Add, 1.0);
        assert_eq!(alpha, [255, 50, 0, 255]);
    }
}
