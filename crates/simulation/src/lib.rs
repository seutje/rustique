//! GPU-compatible particle simulation data.

use bytemuck::{Pod, Zeroable};
use serde::{Deserialize, Serialize};
use std::num::NonZeroU32;

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

/// Deterministic frame-zero particle distribution.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ParticleInitialization {
    #[default]
    Volume,
    GalacticDisk {
        radius: f32,
        thickness: f32,
        lifetime_seconds: f32,
        #[serde(default)]
        spawn_spread_seconds: f32,
        #[serde(default)]
        lifetime_variation: f32,
    },
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ParticleBoundary {
    #[default]
    Box,
    Unbounded,
}

/// Fixed offline simulation timing, independent of preview refresh rate.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SimulationTiming {
    project_fps: NonZeroU32,
    preview_fps: NonZeroU32,
    substeps: NonZeroU32,
}

/// Data-driven forces evaluated in list order by the GPU simulation.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Force {
    Gravity {
        acceleration: [f32; 3],
    },
    PointAttractor {
        position: [f32; 3],
        strength: f32,
        #[serde(default)]
        minimum_acceleration: f32,
        #[serde(default)]
        long_range_strength: f32,
        #[serde(default)]
        long_range_drag: f32,
    },
    /// Point attractor whose position follows a deterministic orbit in the XY plane.
    OrbitingPointAttractor {
        center: [f32; 3],
        orbit_radius: f32,
        orbit_degrees_per_second: f32,
        phase_degrees: f32,
        strength: f32,
    },
    PointRepulsor {
        position: [f32; 3],
        strength: f32,
    },
    Vortex {
        center: [f32; 3],
        strength: f32,
    },
    Drag {
        coefficient: f32,
    },
    SphereConstraint {
        center: [f32; 3],
        radius: f32,
        bounce: f32,
    },
    DirectionalNoise {
        strength: f32,
        frequency: f32,
    },
    CurlNoise {
        strength: f32,
        frequency: f32,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RespawnPolicy {
    Loop,
    Hold,
    Remove,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Emitter {
    Burst {
        count: u32,
        frame: u32,
        respawn: RespawnPolicy,
    },
    Continuous {
        particles_per_second: f32,
        respawn: RespawnPolicy,
    },
}

impl Emitter {
    /// Returns the deterministic number of particles emitted by a timeline frame.
    #[must_use]
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    pub fn emitted_count(&self, frame: u32, project_fps: NonZeroU32) -> u32 {
        match *self {
            Self::Burst {
                count,
                frame: burst_frame,
                ..
            } => {
                if frame >= burst_frame {
                    count
                } else {
                    0
                }
            }
            Self::Continuous {
                particles_per_second,
                ..
            } => {
                let seconds = f64::from(frame) / f64::from(project_fps.get());
                (f64::from(particles_per_second.max(0.0)) * seconds)
                    .floor()
                    .min(f64::from(u32::MAX)) as u32
            }
        }
    }

    #[must_use]
    pub const fn respawn_policy(&self) -> RespawnPolicy {
        match self {
            Self::Burst { respawn, .. } | Self::Continuous { respawn, .. } => *respawn,
        }
    }
}

/// Storage-buffer representation mirrored by `particles.wgsl` (48 bytes).
#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
pub struct GpuForce {
    pub kind: [u32; 4],
    pub primary: [f32; 4],
    pub secondary: [f32; 4],
}

const _: () = assert!(size_of::<GpuForce>() == 48);

impl From<&Force> for GpuForce {
    fn from(force: &Force) -> Self {
        match *force {
            Force::Gravity { acceleration } => Self::new(0, extend(acceleration, 0.0), [0.0; 4]),
            Force::PointAttractor {
                position,
                strength,
                minimum_acceleration,
                long_range_strength,
                long_range_drag,
            } => {
                Self::new(
                    1,
                    extend(position, strength),
                    [
                        minimum_acceleration,
                        long_range_strength,
                        long_range_drag,
                        0.0,
                    ],
                )
            }
            Force::PointRepulsor { position, strength } => {
                Self::new(2, extend(position, strength), [0.0; 4])
            }
            Force::Vortex { center, strength } => Self::new(3, extend(center, strength), [0.0; 4]),
            Force::Drag { coefficient } => Self::new(4, [coefficient, 0.0, 0.0, 0.0], [0.0; 4]),
            Force::SphereConstraint {
                center,
                radius,
                bounce,
            } => Self::new(5, extend(center, radius), [bounce, 0.0, 0.0, 0.0]),
            Force::DirectionalNoise {
                strength,
                frequency,
            } => Self::new(6, [strength, frequency, 0.0, 0.0], [0.0; 4]),
            Force::CurlNoise {
                strength,
                frequency,
            } => Self::new(7, [strength, frequency, 0.0, 0.0], [0.0; 4]),
            Force::OrbitingPointAttractor {
                center,
                orbit_radius,
                orbit_degrees_per_second,
                phase_degrees,
                strength,
            } => Self::new(
                8,
                extend(center, strength),
                [
                    orbit_radius,
                    orbit_degrees_per_second.to_radians(),
                    phase_degrees.to_radians(),
                    0.0,
                ],
            ),
        }
    }
}

const fn extend(value: [f32; 3], fourth: f32) -> [f32; 4] {
    [value[0], value[1], value[2], fourth]
}

impl GpuForce {
    const fn new(kind: u32, primary: [f32; 4], secondary: [f32; 4]) -> Self {
        Self {
            kind: [kind, 0, 0, 0],
            primary,
            secondary,
        }
    }
}

impl SimulationTiming {
    #[must_use]
    pub const fn new(
        project_fps: NonZeroU32,
        preview_fps: NonZeroU32,
        substeps: NonZeroU32,
    ) -> Self {
        Self {
            project_fps,
            preview_fps,
            substeps,
        }
    }

    #[must_use]
    pub const fn project_fps(self) -> u32 {
        self.project_fps.get()
    }

    #[must_use]
    pub const fn preview_fps(self) -> u32 {
        self.preview_fps.get()
    }

    #[must_use]
    pub const fn substeps(self) -> u32 {
        self.substeps.get()
    }

    #[must_use]
    #[allow(clippy::cast_precision_loss)]
    pub fn frame_time(self, frame_index: u32) -> f32 {
        frame_index as f32 / self.project_fps.get() as f32
    }

    #[must_use]
    #[allow(clippy::cast_precision_loss)]
    pub fn substep_delta(self) -> f32 {
        1.0 / (self.project_fps.get() as f32 * self.substeps.get() as f32)
    }
}

const _: () = assert!(size_of::<Particle>() == 64);
const _: () = assert!(align_of::<Particle>() == 4);

/// Creates a stable prefix of particle states for a seed.
///
/// Increasing quality by increasing `count` preserves all existing particles.
#[must_use]
pub fn initialize_particles(count: u32, seed: u64) -> Vec<Particle> {
    initialize_particles_with(count, seed, ParticleInitialization::Volume)
}

/// Creates a stable prefix using a selected deterministic distribution.
#[must_use]
pub fn initialize_particles_with(
    count: u32,
    seed: u64,
    initialization: ParticleInitialization,
) -> Vec<Particle> {
    (0..count)
        .map(|index| {
            let (position, velocity, age, lifetime, spawn_time) = match initialization {
                ParticleInitialization::Volume => {
                    let x = signed_unit(hash(seed, index, 0));
                    let y = signed_unit(hash(seed, index, 1));
                    let z = signed_unit(hash(seed, index, 2));
                    let tangential_speed = 0.05 + unit(hash(seed, index, 3)) * 0.15;
                    (
                        [x * 0.85, y * 0.85, z * 0.85],
                        [
                            -y * tangential_speed,
                            x * tangential_speed,
                            signed_unit(hash(seed, index, 8)) * tangential_speed,
                        ],
                        unit(hash(seed, index, 4)) * 5.0,
                        5.0,
                        0.0,
                    )
                }
                ParticleInitialization::GalacticDisk {
                    radius,
                    thickness,
                    lifetime_seconds,
                    spawn_spread_seconds,
                    lifetime_variation,
                } => {
                    let radial = unit(hash(seed, index, 0)).sqrt() * radius;
                    let angle = unit(hash(seed, index, 1)) * std::f32::consts::TAU;
                    let (sin, cos) = angle.sin_cos();
                    // Match the 0.1-strength central well used by the galaxy
                    // preset, with slight deterministic variation for arm texture.
                    let orbital_speed = (0.1 / radial.max(0.12)).sqrt().min(0.75);
                    let tangential_speed = orbital_speed * (0.9 + unit(hash(seed, index, 3)) * 0.2);
                    let lifetime = lifetime_seconds
                        * (1.0 + signed_unit(hash(seed, index, 9)) * lifetime_variation);
                    (
                        [
                            radial * cos,
                            radial * sin,
                            signed_unit(hash(seed, index, 2)) * thickness,
                        ],
                        [
                            -sin * tangential_speed,
                            cos * tangential_speed,
                            signed_unit(hash(seed, index, 8)) * 0.005,
                        ],
                        0.0,
                        lifetime,
                        unit(hash(seed, index, 10)) * spawn_spread_seconds,
                    )
                }
            };
            Particle {
                position_age: [position[0], position[1], position[2], age],
                velocity_lifetime: [velocity[0], velocity[1], velocity[2], lifetime],
                color: [
                    0.35 + unit(hash(seed, index, 5)) * 0.65,
                    0.45 + unit(hash(seed, index, 6)) * 0.55,
                    0.75 + unit(hash(seed, index, 7)) * 0.25,
                    1.0,
                ],
                // params.x is the absolute simulation time at which this particle appears.
                params: [spawn_time, 0.0, 0.0, 0.0],
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

    #[test]
    fn initialization_populates_and_moves_through_depth() {
        let particles = initialize_particles(128, 42);
        assert!(
            particles
                .iter()
                .any(|particle| particle.position_age[2].abs() > 0.5)
        );
        assert!(
            particles
                .iter()
                .any(|particle| particle.velocity_lifetime[2].abs() > 0.01)
        );
    }

    #[test]
    fn preview_rate_does_not_change_offline_timing() {
        let sixty = NonZeroU32::new(60).unwrap();
        let timing = SimulationTiming::new(
            sixty,
            NonZeroU32::new(30).unwrap(),
            NonZeroU32::new(2).unwrap(),
        );
        assert!((timing.frame_time(120) - 2.0).abs() < f32::EPSILON);
        assert!((timing.substep_delta() - 1.0 / 120.0).abs() < f32::EPSILON);
        assert_eq!(timing.preview_fps(), 30);
    }

    #[test]
    fn emitters_have_deterministic_schedules() {
        let fps = NonZeroU32::new(60).unwrap();
        let burst = Emitter::Burst {
            count: 500,
            frame: 10,
            respawn: RespawnPolicy::Loop,
        };
        assert_eq!(burst.emitted_count(9, fps), 0);
        assert_eq!(burst.emitted_count(10, fps), 500);
        let continuous = Emitter::Continuous {
            particles_per_second: 120.0,
            respawn: RespawnPolicy::Hold,
        };
        assert_eq!(continuous.emitted_count(90, fps), 180);
    }

    #[test]
    fn force_parameters_are_serializable_data() {
        let force = Force::PointAttractor {
            position: [1.0, 2.0, 3.0],
            strength: 0.5,
            minimum_acceleration: 0.0,
            long_range_strength: 0.0,
            long_range_drag: 0.0,
        };
        let json = serde_json::to_string(&force).unwrap();
        assert!(json.contains("point_attractor"));
        assert_eq!(serde_json::from_str::<Force>(&json).unwrap(), force);
        assert_eq!(size_of::<GpuForce>(), 48);
    }

    #[test]
    fn galactic_disk_initialization_is_flat_and_long_lived() {
        let particles = initialize_particles_with(
            128,
            42,
            ParticleInitialization::GalacticDisk {
                radius: 0.8,
                thickness: 0.04,
                lifetime_seconds: 30.0,
                spawn_spread_seconds: 3.0,
                lifetime_variation: 0.4,
            },
        );
        assert!(particles.iter().all(|particle| {
            particle.position_age[2].abs() <= 0.04
                && (18.0..=42.0).contains(&particle.velocity_lifetime[3])
                && particle.position_age[0].hypot(particle.position_age[1]) <= 0.8
                && (0.0..3.0).contains(&particle.params[0])
        }));
        assert!(particles.windows(2).any(|pair| {
            (pair[0].velocity_lifetime[3] - pair[1].velocity_lifetime[3]).abs() > f32::EPSILON
        }));
    }

    #[test]
    fn orbiting_attractor_serializes_and_encodes_orbit_parameters() {
        let force = Force::OrbitingPointAttractor {
            center: [0.0, 0.0, 0.0],
            orbit_radius: 0.35,
            orbit_degrees_per_second: 45.0,
            phase_degrees: 180.0,
            strength: 0.06,
        };
        let json = serde_json::to_string(&force).unwrap();
        assert!(json.contains("orbiting_point_attractor"));
        assert_eq!(serde_json::from_str::<Force>(&json).unwrap(), force);

        let gpu = GpuForce::from(&force);
        assert_eq!(gpu.kind[0], 8);
        assert!((gpu.secondary[1] - 45.0_f32.to_radians()).abs() < f32::EPSILON);
        assert!((gpu.secondary[2] - std::f32::consts::PI).abs() < f32::EPSILON);
    }
}
