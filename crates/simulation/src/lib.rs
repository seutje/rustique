//! GPU-compatible particle simulation data.

use bytemuck::{Pod, Zeroable};

/// Particle state shared verbatim with WGSL storage buffers.
///
/// Each field is a 16-byte aligned `vec4<f32>` in WGSL, producing a 64-byte
/// structure with no implicit padding. `position_age.w` stores age and
/// `velocity_lifetime.w` stores lifetime.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Pod, Zeroable)]
pub struct Particle {
    pub position_age: [f32; 4],
    pub velocity_lifetime: [f32; 4],
    pub color: [f32; 4],
    pub params: [f32; 4],
}

const _: () = assert!(size_of::<Particle>() == 64);
const _: () = assert!(align_of::<Particle>() == 4);

/// Creates a stable prefix of particle states for a seed.
///
/// Increasing quality by increasing `count` preserves all existing particles.
#[must_use]
pub fn initialize_particles(count: u32, seed: u64) -> Vec<Particle> {
    (0..count)
        .map(|index| {
            let x = signed_unit(hash(seed, index, 0));
            let y = signed_unit(hash(seed, index, 1));
            let z = signed_unit(hash(seed, index, 2)) * 0.25;
            let velocity_scale = 0.05 + unit(hash(seed, index, 3)) * 0.15;
            Particle {
                position_age: [x * 0.85, y * 0.85, z, unit(hash(seed, index, 4)) * 5.0],
                velocity_lifetime: [-y * velocity_scale, x * velocity_scale, 0.0, 5.0],
                color: [
                    0.35 + unit(hash(seed, index, 5)) * 0.65,
                    0.45 + unit(hash(seed, index, 6)) * 0.55,
                    0.75 + unit(hash(seed, index, 7)) * 0.25,
                    1.0,
                ],
                params: [0.0; 4],
            }
        })
        .collect()
}

fn hash(seed: u64, index: u32, stream: u32) -> u32 {
    let mut value = seed ^ u64::from(index).wrapping_mul(0x9e37_79b9_7f4a_7c15);
    value ^= u64::from(stream).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    value = (value ^ (value >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    value = (value ^ (value >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    let mixed = value ^ (value >> 31);
    let bytes = mixed.to_le_bytes();
    u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])
}

fn unit(value: u32) -> f32 {
    let bytes = value.to_le_bytes();
    f32::from(u16::from_le_bytes([bytes[2], bytes[3]])) / 65_536.0
}

fn signed_unit(value: u32) -> f32 {
    unit(value).mul_add(2.0, -1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn particle_layout_is_exactly_sixty_four_bytes() {
        assert_eq!(size_of::<Particle>(), 64);
    }

    #[test]
    fn initialization_is_deterministic_and_prefix_stable() {
        let short = initialize_particles(4, 42);
        assert_eq!(short, initialize_particles(4, 42));
        assert_eq!(short, initialize_particles(8, 42)[..4]);
        assert_ne!(short, initialize_particles(4, 43));
    }
}
