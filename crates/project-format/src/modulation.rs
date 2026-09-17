use audio_engine::AudioFeatureFrame;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModulationSource {
    Sub,
    Bass,
    LowMids,
    Mids,
    HighMids,
    Highs,
    Rms,
    Transient,
    SpectralCentroid,
    SpectralFlux,
}

impl ModulationSource {
    #[must_use]
    pub const fn sample(self, features: AudioFeatureFrame) -> f32 {
        match self {
            Self::Sub => features.bands.sub,
            Self::Bass => features.bands.bass,
            Self::LowMids => features.bands.low_mids,
            Self::Mids => features.bands.mids,
            Self::HighMids => features.bands.high_mids,
            Self::Highs => features.bands.highs,
            Self::Rms => features.rms,
            Self::Transient => features.transient_strength,
            Self::SpectralCentroid => features.spectral_centroid,
            Self::SpectralFlux => features.spectral_flux,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModulationTarget {
    GravityStrength,
    ParticleSize,
    Brightness,
    HueShift,
    BurstEmission,
    CameraFov,
    CameraShake,
    MaterialRoughness,
    ReflectionIntensity,
    SurfaceScale,
    VolumeDensity,
    VolumeMotion,
    DropletDensity,
    DropletSize,
    DropletRefraction,
    DropletGravity,
    FlockingSeparation,
    FlockingCohesion,
    FlockingTurbulence,
    FlockingSpeed,
    FlockingRandomness,
    FlockingImpulse,
    FireEmission,
    FireBaseWidth,
    FireHeight,
    FireSway,
    FireTurbulence,
    FireFlicker,
    FireShimmer,
    FireSparks,
    FireTemperature,
    FireBeatWave,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModulationPolarity {
    Normal,
    Inverted,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModulationCurve {
    Linear,
    Exponential,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModulationCombine {
    #[default]
    Replace,
    Multiply,
    Add,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModulationMapping {
    #[serde(default = "mapping_enabled")]
    pub enabled: bool,
    pub source: ModulationSource,
    pub target: ModulationTarget,
    pub amount: f32,
    pub offset: f32,
    pub minimum: f32,
    pub maximum: f32,
    pub polarity: ModulationPolarity,
    pub curve: ModulationCurve,
    #[serde(default)]
    pub combine: ModulationCombine,
    pub attack_seconds: f32,
    pub release_seconds: f32,
}

const fn mapping_enabled() -> bool {
    true
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ActiveModulation {
    pub target: ModulationTarget,
    pub source_value: f32,
    pub output_value: f32,
    pub combine: ModulationCombine,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ModulatedParameters {
    pub gravity_strength: f32,
    pub particle_size: f32,
    pub brightness: f32,
    pub hue_shift: f32,
    pub burst_emission: f32,
    pub camera_fov: f32,
    pub camera_shake: f32,
    pub material_roughness: f32,
    pub reflection_intensity: f32,
    pub surface_scale: f32,
    pub volume_density: f32,
    pub volume_motion: f32,
    pub droplet_density: f32,
    pub droplet_size: f32,
    pub droplet_refraction: f32,
    pub droplet_gravity: f32,
    pub flocking_separation: f32,
    pub flocking_cohesion: f32,
    pub flocking_turbulence: f32,
    pub flocking_speed: f32,
    pub flocking_randomness: f32,
    pub flocking_impulse: f32,
    pub fire_emission: f32,
    pub fire_base_width: f32,
    pub fire_height: f32,
    pub fire_sway: f32,
    pub fire_turbulence: f32,
    pub fire_flicker: f32,
    pub fire_shimmer: f32,
    pub fire_sparks: f32,
    pub fire_temperature: f32,
    pub fire_beat_wave: f32,
}

impl Default for ModulatedParameters {
    fn default() -> Self {
        Self {
            gravity_strength: 1.0,
            particle_size: 2.0,
            brightness: 1.0,
            hue_shift: 0.0,
            burst_emission: 0.0,
            camera_fov: 0.0,
            camera_shake: 0.0,
            material_roughness: 0.14,
            reflection_intensity: 1.35,
            surface_scale: 1.0,
            volume_density: 1.0,
            volume_motion: 1.0,
            droplet_density: 1.0,
            droplet_size: 1.0,
            droplet_refraction: 1.0,
            droplet_gravity: 1.0,
            flocking_separation: 1.0,
            flocking_cohesion: 1.0,
            flocking_turbulence: 1.0,
            flocking_speed: 1.0,
            flocking_randomness: 1.0,
            flocking_impulse: 0.0,
            fire_emission: 1.0,
            fire_base_width: 1.0,
            fire_height: 1.0,
            fire_sway: 1.0,
            fire_turbulence: 1.0,
            fire_flicker: 1.0,
            fire_shimmer: 1.0,
            fire_sparks: 0.0,
            fire_temperature: 0.0,
            fire_beat_wave: 0.0,
        }
    }
}

impl ModulatedParameters {
    pub fn apply(&mut self, active: &[ActiveModulation]) {
        for value in active {
            match value.target {
                ModulationTarget::GravityStrength => apply_value(&mut self.gravity_strength, value),
                ModulationTarget::ParticleSize => apply_value(&mut self.particle_size, value),
                ModulationTarget::Brightness => apply_value(&mut self.brightness, value),
                ModulationTarget::HueShift => apply_value(&mut self.hue_shift, value),
                ModulationTarget::BurstEmission => apply_value(&mut self.burst_emission, value),
                ModulationTarget::CameraFov => apply_value(&mut self.camera_fov, value),
                ModulationTarget::CameraShake => apply_value(&mut self.camera_shake, value),
                ModulationTarget::MaterialRoughness => {
                    apply_value(&mut self.material_roughness, value);
                }
                ModulationTarget::ReflectionIntensity => {
                    apply_value(&mut self.reflection_intensity, value);
                }
                ModulationTarget::SurfaceScale => apply_value(&mut self.surface_scale, value),
                ModulationTarget::VolumeDensity => apply_value(&mut self.volume_density, value),
                ModulationTarget::VolumeMotion => apply_value(&mut self.volume_motion, value),
                ModulationTarget::DropletDensity => apply_value(&mut self.droplet_density, value),
                ModulationTarget::DropletSize => apply_value(&mut self.droplet_size, value),
                ModulationTarget::DropletRefraction => {
                    apply_value(&mut self.droplet_refraction, value);
                }
                ModulationTarget::DropletGravity => apply_value(&mut self.droplet_gravity, value),
                ModulationTarget::FlockingSeparation => {
                    apply_value(&mut self.flocking_separation, value);
                }
                ModulationTarget::FlockingCohesion => {
                    apply_value(&mut self.flocking_cohesion, value);
                }
                ModulationTarget::FlockingTurbulence => {
                    apply_value(&mut self.flocking_turbulence, value);
                }
                ModulationTarget::FlockingSpeed => apply_value(&mut self.flocking_speed, value),
                ModulationTarget::FlockingRandomness => {
                    apply_value(&mut self.flocking_randomness, value);
                }
                ModulationTarget::FlockingImpulse => apply_value(&mut self.flocking_impulse, value),
                ModulationTarget::FireEmission => apply_value(&mut self.fire_emission, value),
                ModulationTarget::FireBaseWidth => apply_value(&mut self.fire_base_width, value),
                ModulationTarget::FireHeight => apply_value(&mut self.fire_height, value),
                ModulationTarget::FireSway => apply_value(&mut self.fire_sway, value),
                ModulationTarget::FireTurbulence => apply_value(&mut self.fire_turbulence, value),
                ModulationTarget::FireFlicker => apply_value(&mut self.fire_flicker, value),
                ModulationTarget::FireShimmer => apply_value(&mut self.fire_shimmer, value),
                ModulationTarget::FireSparks => apply_value(&mut self.fire_sparks, value),
                ModulationTarget::FireTemperature => apply_value(&mut self.fire_temperature, value),
                ModulationTarget::FireBeatWave => apply_value(&mut self.fire_beat_wave, value),
            }
        }
    }
}

fn apply_value(target: &mut f32, modulation: &ActiveModulation) {
    match modulation.combine {
        ModulationCombine::Replace => *target = modulation.output_value,
        ModulationCombine::Multiply => *target *= modulation.output_value,
        ModulationCombine::Add => *target += modulation.output_value,
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct EnvelopeSmoother {
    value: f32,
}

impl EnvelopeSmoother {
    #[must_use]
    pub const fn value(self) -> f32 {
        self.value
    }

    #[must_use]
    pub fn update(
        &mut self,
        input: f32,
        delta_seconds: f32,
        attack_seconds: f32,
        release_seconds: f32,
    ) -> f32 {
        let duration = if input > self.value {
            attack_seconds
        } else {
            release_seconds
        };
        self.value = if duration <= 0.0 {
            input
        } else {
            let blend = 1.0 - (-delta_seconds.max(0.0) / duration).exp();
            self.value + (input - self.value) * blend
        };
        self.value
    }
}

/// Evaluates mappings in stable list order and updates their smoothing state.
#[must_use]
pub fn evaluate_mappings(
    mappings: &[ModulationMapping],
    smoothers: &mut [EnvelopeSmoother],
    features: AudioFeatureFrame,
    delta_seconds: f32,
) -> Vec<ActiveModulation> {
    mappings
        .iter()
        .zip(smoothers)
        .filter_map(|(mapping, smoother)| {
            if !mapping.enabled {
                return None;
            }
            let raw = mapping.source.sample(features).clamp(0.0, 1.0);
            let smoothed = smoother.update(
                raw,
                delta_seconds,
                mapping.attack_seconds,
                mapping.release_seconds,
            );
            let polarized = match mapping.polarity {
                ModulationPolarity::Normal => smoothed,
                ModulationPolarity::Inverted => 1.0 - smoothed,
            };
            let curved = match mapping.curve {
                ModulationCurve::Linear => polarized,
                ModulationCurve::Exponential => polarized * polarized,
            };
            Some(ActiveModulation {
                target: mapping.target,
                source_value: smoothed,
                output_value: (mapping.offset + curved * mapping.amount)
                    .clamp(mapping.minimum, mapping.maximum),
                combine: mapping.combine,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mapping(target: ModulationTarget) -> ModulationMapping {
        ModulationMapping {
            enabled: true,
            source: ModulationSource::Bass,
            target,
            amount: 2.0,
            offset: 1.0,
            minimum: 0.0,
            maximum: 2.0,
            polarity: ModulationPolarity::Normal,
            curve: ModulationCurve::Linear,
            combine: ModulationCombine::Replace,
            attack_seconds: 0.0,
            release_seconds: 0.0,
        }
    }

    #[test]
    fn maps_visual_targets_and_clamps() {
        let mappings = [
            mapping(ModulationTarget::GravityStrength),
            mapping(ModulationTarget::ParticleSize),
            mapping(ModulationTarget::Brightness),
            mapping(ModulationTarget::HueShift),
        ];
        let mut smoothers = [EnvelopeSmoother::default(); 4];
        let mut features = AudioFeatureFrame::default();
        features.bands.bass = 0.75;
        let active = evaluate_mappings(&mappings, &mut smoothers, features, 1.0 / 60.0);
        assert_eq!(active.len(), 4);
        assert!(
            active
                .iter()
                .all(|value| (value.output_value - 2.0).abs() < f32::EPSILON)
        );
        let mut parameters = ModulatedParameters::default();
        parameters.apply(&active);
        assert!((parameters.gravity_strength - 2.0).abs() < f32::EPSILON);
        assert!((parameters.particle_size - 2.0).abs() < f32::EPSILON);
        assert!((parameters.brightness - 2.0).abs() < f32::EPSILON);
        assert!((parameters.hue_shift - 2.0).abs() < f32::EPSILON);
    }

    #[test]
    fn smoothing_curves_and_inversion_are_deterministic() {
        let mut value = mapping(ModulationTarget::BurstEmission);
        value.polarity = ModulationPolarity::Inverted;
        value.curve = ModulationCurve::Exponential;
        value.attack_seconds = 0.1;
        value.release_seconds = 0.2;
        let features = AudioFeatureFrame::default();
        let first = evaluate_mappings(
            &[value.clone()],
            &mut [EnvelopeSmoother::default()],
            features,
            1.0 / 60.0,
        );
        let second = evaluate_mappings(
            &[value],
            &mut [EnvelopeSmoother::default()],
            features,
            1.0 / 60.0,
        );
        assert_eq!(first, second);
    }

    #[test]
    fn disabled_mapping_is_not_evaluated() {
        let mut value = mapping(ModulationTarget::Brightness);
        value.enabled = false;
        let active = evaluate_mappings(
            &[value],
            &mut [EnvelopeSmoother::default()],
            AudioFeatureFrame::default(),
            1.0 / 60.0,
        );
        assert!(active.is_empty());
    }

    #[test]
    fn combine_modes_preserve_base_values() {
        let mut parameters = ModulatedParameters {
            particle_size: 4.0,
            brightness: 0.8,
            ..ModulatedParameters::default()
        };
        parameters.apply(&[
            ActiveModulation {
                target: ModulationTarget::ParticleSize,
                source_value: 0.5,
                output_value: 1.5,
                combine: ModulationCombine::Multiply,
            },
            ActiveModulation {
                target: ModulationTarget::Brightness,
                source_value: 0.5,
                output_value: 0.2,
                combine: ModulationCombine::Add,
            },
        ]);
        assert!((parameters.particle_size - 6.0).abs() < f32::EPSILON);
        assert!((parameters.brightness - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn applies_specialized_renderer_targets() {
        let targets = [
            ModulationTarget::VolumeDensity,
            ModulationTarget::VolumeMotion,
            ModulationTarget::DropletDensity,
            ModulationTarget::DropletSize,
            ModulationTarget::DropletRefraction,
            ModulationTarget::DropletGravity,
        ];
        let active: Vec<_> = targets
            .into_iter()
            .zip([2.0, 3.0, 4.0, 5.0, 6.0, 7.0])
            .map(|(target, output_value)| ActiveModulation {
                target,
                source_value: 0.5,
                output_value,
                combine: ModulationCombine::Replace,
            })
            .collect();
        let mut parameters = ModulatedParameters::default();
        parameters.apply(&active);
        let actual = [
            parameters.volume_density,
            parameters.volume_motion,
            parameters.droplet_density,
            parameters.droplet_size,
            parameters.droplet_refraction,
            parameters.droplet_gravity,
        ];
        assert!(
            actual
                .into_iter()
                .zip([2.0, 3.0, 4.0, 5.0, 6.0, 7.0])
                .all(|(left, right)| (left - right).abs() < f32::EPSILON)
        );
    }
}
