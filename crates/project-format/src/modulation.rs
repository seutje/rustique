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
    BurstEmission,
    CameraFov,
    CameraShake,
    MaterialRoughness,
    ReflectionIntensity,
    SurfaceScale,
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
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ModulatedParameters {
    pub gravity_strength: f32,
    pub particle_size: f32,
    pub brightness: f32,
    pub burst_emission: f32,
    pub camera_fov: f32,
    pub camera_shake: f32,
    pub material_roughness: f32,
    pub reflection_intensity: f32,
    pub surface_scale: f32,
}

impl Default for ModulatedParameters {
    fn default() -> Self {
        Self {
            gravity_strength: 1.0,
            particle_size: 2.0,
            brightness: 1.0,
            burst_emission: 0.0,
            camera_fov: 0.0,
            camera_shake: 0.0,
            material_roughness: 0.14,
            reflection_intensity: 1.35,
            surface_scale: 1.0,
        }
    }
}

impl ModulatedParameters {
    pub fn apply(&mut self, active: &[ActiveModulation]) {
        for value in active {
            match value.target {
                ModulationTarget::GravityStrength => self.gravity_strength = value.output_value,
                ModulationTarget::ParticleSize => self.particle_size = value.output_value,
                ModulationTarget::Brightness => self.brightness = value.output_value,
                ModulationTarget::BurstEmission => self.burst_emission = value.output_value,
                ModulationTarget::CameraFov => self.camera_fov = value.output_value,
                ModulationTarget::CameraShake => self.camera_shake = value.output_value,
                ModulationTarget::MaterialRoughness => self.material_roughness = value.output_value,
                ModulationTarget::ReflectionIntensity => {
                    self.reflection_intensity = value.output_value;
                }
                ModulationTarget::SurfaceScale => self.surface_scale = value.output_value,
            }
        }
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
            attack_seconds: 0.0,
            release_seconds: 0.0,
        }
    }

    #[test]
    fn maps_three_visual_targets_and_clamps() {
        let mappings = [
            mapping(ModulationTarget::GravityStrength),
            mapping(ModulationTarget::ParticleSize),
            mapping(ModulationTarget::Brightness),
        ];
        let mut smoothers = [EnvelopeSmoother::default(); 3];
        let mut features = AudioFeatureFrame::default();
        features.bands.bass = 0.75;
        let active = evaluate_mappings(&mappings, &mut smoothers, features, 1.0 / 60.0);
        assert_eq!(active.len(), 3);
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
}
