use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};
use simulation::Force;

use crate::{
    CameraV1, ModulationMapping, ParticleSystemV1, ProjectError, ProjectV1, RenderDefaultsV1,
    RenderModeV1,
};

pub const PRESET_VERSION: u32 = 1;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VisualPresetV1 {
    pub preset_version: u32,
    pub id: String,
    pub display_name: String,
    #[serde(default)]
    pub description: String,
    pub particle_system: ParticleSystemV1,
    #[serde(default)]
    pub forces: Vec<Force>,
    pub camera: CameraV1,
    pub render_defaults: RenderDefaultsV1,
    #[serde(default)]
    pub render_mode: RenderModeV1,
    #[serde(default)]
    pub liquid_chrome: crate::LiquidChromeV1,
    #[serde(default)]
    pub water_droplets: crate::WaterDropletsV1,
    #[serde(default)]
    pub macros: Vec<MacroParameterV1>,
    #[serde(default)]
    pub recommended_mappings: Vec<ModulationMapping>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MacroParameterV1 {
    pub id: String,
    pub label: String,
    pub target: MacroTargetV1,
    pub minimum: f32,
    pub maximum: f32,
    pub default: f32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MacroTargetV1 {
    ParticleCount,
    ParticleSize,
    ForceStrengthScale,
    OrbitSpeed,
    DollySpeed,
    CameraFov,
    CameraShake,
    MaterialRoughness,
    SurfaceDeformation,
    FireBaseRadius,
    FireHeight,
    FireBuoyancy,
    FireTurbulence,
    FireFlicker,
    FireSparkAmount,
    FireSparkVelocity,
    FireSparkLifetime,
    FireTemperature,
    FireAudioReactivity,
    FireBeatWave,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PresetOverridesV1 {
    #[serde(default)]
    pub macros: BTreeMap<String, f32>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PresetSelectionV1 {
    pub source: PathBuf,
    #[serde(default)]
    pub overrides: PresetOverridesV1,
}

impl VisualPresetV1 {
    /// Loads and validates a visual preset JSON file.
    ///
    /// # Errors
    ///
    /// Returns contextual I/O, JSON, or schema validation errors.
    pub fn load(path: impl AsRef<Path>) -> Result<Self, ProjectError> {
        let path = path.as_ref();
        let json = fs::read_to_string(path).map_err(|source| ProjectError::Read {
            path: path.to_owned(),
            source,
        })?;
        let preset: Self = serde_json::from_str(&json).map_err(|source| ProjectError::Parse {
            path: path.to_owned(),
            source,
        })?;
        preset.validate()?;
        Ok(preset)
    }

    /// Applies preset defaults and validated project-local macro overrides.
    ///
    /// # Errors
    ///
    /// Returns an error for unknown, non-finite, or out-of-range overrides.
    pub fn apply(
        &self,
        project: &mut ProjectV1,
        overrides: &PresetOverridesV1,
    ) -> Result<(), ProjectError> {
        self.validate()?;
        for id in overrides.macros.keys() {
            if !self.macros.iter().any(|parameter| parameter.id == *id) {
                return Err(ProjectError::Validation(format!(
                    "preset '{}' has no macro named '{id}'",
                    self.id
                )));
            }
        }
        project.particle_system = self.particle_system.clone();
        project.forces.clone_from(&self.forces);
        project.camera = self.camera.clone();
        project.render_defaults = self.render_defaults.clone();
        project.render_mode = self.render_mode;
        project.liquid_chrome.clone_from(&self.liquid_chrome);
        project.water_droplets.clone_from(&self.water_droplets);
        project
            .modulation_mappings
            .clone_from(&self.recommended_mappings);
        for parameter in &self.macros {
            let value = overrides
                .macros
                .get(&parameter.id)
                .copied()
                .unwrap_or(parameter.default);
            if !value.is_finite() || !(parameter.minimum..=parameter.maximum).contains(&value) {
                return Err(ProjectError::Validation(format!(
                    "preset macro '{}' must be between {} and {}",
                    parameter.id, parameter.minimum, parameter.maximum
                )));
            }
            apply_macro(project, parameter.target, value);
        }
        Ok(())
    }

    fn validate(&self) -> Result<(), ProjectError> {
        if self.preset_version != PRESET_VERSION || self.id.trim().is_empty() {
            return Err(ProjectError::Validation(format!(
                "invalid visual preset version or id for '{}'",
                self.display_name
            )));
        }
        for parameter in &self.macros {
            if parameter.id.trim().is_empty()
                || !parameter.minimum.is_finite()
                || !parameter.maximum.is_finite()
                || !parameter.default.is_finite()
                || parameter.minimum > parameter.maximum
                || !(parameter.minimum..=parameter.maximum).contains(&parameter.default)
            {
                return Err(ProjectError::Validation(format!(
                    "invalid macro schema in preset '{}'",
                    self.id
                )));
            }
        }
        Ok(())
    }
}

#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn apply_macro(project: &mut ProjectV1, target: MacroTargetV1, value: f32) {
    match target {
        MacroTargetV1::ParticleCount => project.particle_system.count = value.round() as u32,
        MacroTargetV1::ParticleSize => project.render_defaults.particle_size_pixels = value,
        MacroTargetV1::ForceStrengthScale => scale_forces(&mut project.forces, value),
        MacroTargetV1::OrbitSpeed => project.camera.orbit_degrees_per_second = value,
        MacroTargetV1::DollySpeed => project.camera.dolly_units_per_second = value,
        MacroTargetV1::CameraFov => project.camera.vertical_fov_degrees = value,
        MacroTargetV1::CameraShake => project.camera.shake_amplitude = value,
        MacroTargetV1::MaterialRoughness => project.liquid_chrome.roughness = value,
        MacroTargetV1::SurfaceDeformation => project.liquid_chrome.surface_deformation = value,
        MacroTargetV1::FireBaseRadius => set_fire_parameter(
            &mut project.particle_system.initialization,
            FireParameter::BaseRadius,
            value,
        ),
        MacroTargetV1::FireHeight => set_fire_parameter(
            &mut project.particle_system.initialization,
            FireParameter::Height,
            value,
        ),
        MacroTargetV1::FireBuoyancy => set_fire_parameter(
            &mut project.particle_system.initialization,
            FireParameter::Buoyancy,
            value,
        ),
        MacroTargetV1::FireTurbulence => set_fire_parameter(
            &mut project.particle_system.initialization,
            FireParameter::Turbulence,
            value,
        ),
        MacroTargetV1::FireFlicker => set_fire_parameter(
            &mut project.particle_system.initialization,
            FireParameter::Flicker,
            value,
        ),
        MacroTargetV1::FireSparkAmount => set_fire_parameter(
            &mut project.particle_system.initialization,
            FireParameter::SparkAmount,
            value,
        ),
        MacroTargetV1::FireSparkVelocity => set_fire_parameter(
            &mut project.particle_system.initialization,
            FireParameter::SparkVelocity,
            value,
        ),
        MacroTargetV1::FireSparkLifetime => set_fire_parameter(
            &mut project.particle_system.initialization,
            FireParameter::SparkLifetime,
            value,
        ),
        MacroTargetV1::FireTemperature => set_fire_parameter(
            &mut project.particle_system.initialization,
            FireParameter::Temperature,
            value,
        ),
        MacroTargetV1::FireAudioReactivity => set_fire_parameter(
            &mut project.particle_system.initialization,
            FireParameter::AudioReactivity,
            value,
        ),
        MacroTargetV1::FireBeatWave => set_fire_parameter(
            &mut project.particle_system.initialization,
            FireParameter::BeatWave,
            value,
        ),
    }
}

#[derive(Clone, Copy)]
enum FireParameter {
    BaseRadius,
    Height,
    Buoyancy,
    Turbulence,
    Flicker,
    SparkAmount,
    SparkVelocity,
    SparkLifetime,
    Temperature,
    AudioReactivity,
    BeatWave,
}

fn set_fire_parameter(
    initialization: &mut simulation::ParticleInitialization,
    parameter: FireParameter,
    value: f32,
) {
    let simulation::ParticleInitialization::Fire {
        base_radius,
        flame_height,
        buoyancy,
        turbulence,
        flicker,
        spark_ratio,
        spark_velocity,
        spark_lifetime,
        temperature,
        audio_reactivity,
        beat_wave_strength,
        ..
    } = initialization
    else {
        return;
    };
    match parameter {
        FireParameter::BaseRadius => *base_radius = value,
        FireParameter::Height => *flame_height = value,
        FireParameter::Buoyancy => *buoyancy = value,
        FireParameter::Turbulence => *turbulence = value,
        FireParameter::Flicker => *flicker = value,
        FireParameter::SparkAmount => *spark_ratio = value,
        FireParameter::SparkVelocity => *spark_velocity = value,
        FireParameter::SparkLifetime => *spark_lifetime = value,
        FireParameter::Temperature => *temperature = value,
        FireParameter::AudioReactivity => *audio_reactivity = value,
        FireParameter::BeatWave => *beat_wave_strength = value,
    }
}

fn scale_forces(forces: &mut [Force], scale: f32) {
    for force in forces {
        match force {
            Force::Gravity { acceleration } => acceleration.iter_mut().for_each(|v| *v *= scale),
            Force::PointAttractor { strength, .. }
            | Force::OrbitingPointAttractor { strength, .. }
            | Force::PointRepulsor { strength, .. }
            | Force::Vortex { strength, .. }
            | Force::DirectionalNoise { strength, .. }
            | Force::CurlNoise { strength, .. } => *strength *= scale,
            Force::Drag { coefficient } => *coefficient *= scale,
            Force::SphereConstraint { .. } => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn repository_path(relative: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .join(relative)
    }

    #[test]
    fn repository_presets_load_and_are_distinct() {
        let names = [
            "star-system",
            "nebula",
            "liquid-chrome",
            "green-slime",
            "water-droplets",
            "murmuration",
            "fire",
        ];
        let presets: Vec<_> = names
            .iter()
            .map(|name| {
                VisualPresetV1::load(repository_path(&format!("presets/{name}.json"))).unwrap()
            })
            .collect();
        assert_eq!(presets.len(), 7);
        assert_ne!(presets[0].forces, presets[1].forces);
        assert_ne!(presets[1].render_defaults, presets[2].render_defaults);
        assert!(
            presets
                .iter()
                .all(|preset| preset.recommended_mappings.len() >= 3)
        );
    }

    #[test]
    fn reflective_material_presets_react_to_bass_and_transients() {
        for name in ["liquid-chrome", "green-slime"] {
            let preset =
                VisualPresetV1::load(repository_path(&format!("presets/{name}.json"))).unwrap();
            assert!(preset.camera.fov_modulation_degrees.abs() >= 8.0);
            assert!(preset.recommended_mappings.iter().any(|mapping| {
                mapping.source == crate::ModulationSource::Bass
                    && mapping.target == crate::ModulationTarget::CameraFov
            }));
            assert!(preset.recommended_mappings.iter().any(|mapping| {
                mapping.source == crate::ModulationSource::Transient
                    && mapping.target == crate::ModulationTarget::ReflectionIntensity
            }));
            assert!(preset.recommended_mappings.iter().any(|mapping| {
                mapping.source == crate::ModulationSource::Transient
                    && mapping.target == crate::ModulationTarget::MaterialRoughness
            }));
        }
    }

    #[test]
    fn project_macro_override_is_applied() {
        let preset = VisualPresetV1::load(repository_path("presets/star-system.json")).unwrap();
        let mut project =
            ProjectV1::load(repository_path("examples/star-orbit.rustique.json")).unwrap();
        let overrides = PresetOverridesV1 {
            macros: BTreeMap::from([("density".to_owned(), 250_000.0)]),
        };
        preset.apply(&mut project, &overrides).unwrap();
        assert_eq!(project.particle_system.count, 250_000);
    }

    #[test]
    fn star_system_has_three_wells_and_sub_reactivity() {
        let project =
            ProjectV1::load(repository_path("examples/star-orbit.rustique.json")).unwrap();
        assert_eq!(
            project.particle_system.boundary,
            simulation::ParticleBoundary::Unbounded
        );
        assert_eq!(
            project
                .forces
                .iter()
                .filter(|force| matches!(force, Force::PointAttractor { .. }))
                .count(),
            1
        );
        assert_eq!(
            project
                .forces
                .iter()
                .filter(|force| matches!(force, Force::OrbitingPointAttractor { .. }))
                .count(),
            2
        );
        assert!(project.modulation_mappings.iter().any(|mapping| {
            mapping.source == crate::ModulationSource::Sub
                && mapping.target == crate::ModulationTarget::GravityStrength
                && mapping.minimum > 0.0
        }));
        assert!(project.modulation_mappings.iter().any(|mapping| {
            mapping.source == crate::ModulationSource::Sub
                && mapping.target == crate::ModulationTarget::ParticleSize
        }));
        assert!(project.modulation_mappings.iter().any(|mapping| {
            mapping.source == crate::ModulationSource::Bass
                && mapping.target == crate::ModulationTarget::HueShift
        }));
        assert!(project.render_defaults.particle_depth_size_strength > 0.0);
        assert!(project.render_defaults.particle_depth_brightness_strength > 0.0);
    }

    #[test]
    fn murmuration_uses_field_flocking_and_audio_motion_targets() {
        let preset = VisualPresetV1::load(repository_path("presets/murmuration.json")).unwrap();
        let flocking = preset.particle_system.flocking.as_ref().unwrap();
        assert!(flocking.enabled && flocking.murmuration.enabled);
        assert!(flocking.attractors.len() > 1);
        assert!(preset.render_defaults.particle_depth_size_strength >= 0.8);
        assert!(preset.render_defaults.particle_depth_brightness_strength > 0.0);
        for target in [
            crate::ModulationTarget::FlockingSeparation,
            crate::ModulationTarget::FlockingCohesion,
            crate::ModulationTarget::FlockingTurbulence,
            crate::ModulationTarget::FlockingSpeed,
            crate::ModulationTarget::FlockingRandomness,
            crate::ModulationTarget::FlockingImpulse,
        ] {
            assert!(
                preset
                    .recommended_mappings
                    .iter()
                    .any(|mapping| mapping.target == target)
            );
        }
    }

    #[test]
    fn fire_uses_scale_appropriate_audio_mappings_and_safe_silence_defaults() {
        let preset = VisualPresetV1::load(repository_path("presets/fire.json")).unwrap();
        assert!(matches!(
            preset.particle_system.initialization,
            simulation::ParticleInitialization::Fire {
                audio_reactivity,
                spark_ratio,
                ..
            } if audio_reactivity > 0.0 && audio_reactivity <= 0.5 && spark_ratio > 0.0
        ));
        for (source, target) in [
            (
                crate::ModulationSource::Sub,
                crate::ModulationTarget::FireBaseWidth,
            ),
            (
                crate::ModulationSource::Bass,
                crate::ModulationTarget::FireHeight,
            ),
            (
                crate::ModulationSource::LowMids,
                crate::ModulationTarget::FireSway,
            ),
            (
                crate::ModulationSource::Mids,
                crate::ModulationTarget::FireTurbulence,
            ),
            (
                crate::ModulationSource::HighMids,
                crate::ModulationTarget::FireFlicker,
            ),
            (
                crate::ModulationSource::Highs,
                crate::ModulationTarget::FireShimmer,
            ),
            (
                crate::ModulationSource::Transient,
                crate::ModulationTarget::FireSparks,
            ),
            (
                crate::ModulationSource::SpectralCentroid,
                crate::ModulationTarget::FireTemperature,
            ),
        ] {
            assert!(
                preset
                    .recommended_mappings
                    .iter()
                    .any(|mapping| { mapping.source == source && mapping.target == target })
            );
        }
        let bass = preset
            .recommended_mappings
            .iter()
            .find(|mapping| mapping.target == crate::ModulationTarget::FireHeight)
            .unwrap();
        assert!((0.02..=0.06).contains(&bass.attack_seconds));
        assert!((0.15..=0.4).contains(&bass.release_seconds));
    }

    #[test]
    fn switching_between_fire_and_existing_preset_replaces_specialized_state() {
        let fire = VisualPresetV1::load(repository_path("presets/fire.json")).unwrap();
        let stars = VisualPresetV1::load(repository_path("presets/star-system.json")).unwrap();
        let mut project =
            ProjectV1::load(repository_path("examples/star-orbit.rustique.json")).unwrap();
        fire.apply(&mut project, &PresetOverridesV1::default())
            .unwrap();
        assert!(matches!(
            project.particle_system.initialization,
            simulation::ParticleInitialization::Fire { .. }
        ));
        stars
            .apply(&mut project, &PresetOverridesV1::default())
            .unwrap();
        assert!(!matches!(
            project.particle_system.initialization,
            simulation::ParticleInitialization::Fire { .. }
        ));
    }
}
