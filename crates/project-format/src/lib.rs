//! Versioned, renderer-independent Rustique project representation.

mod audio_profile;
mod automation;
mod creative;
mod modulation;
mod package;
mod preset;

pub use audio_profile::{
    AnalysisProfileOverridesV1, AnalysisProfileSelectionV1, AnalysisProfileV1, FeatureWeightsV1,
    ReactionProfileOverridesV1, ReactionProfileSelectionV1, ReactionProfileV1,
};
pub use automation::{AutomationKeyframeV1, AutomationTrackV1, SceneMarkerV1, evaluate_automation};
pub use creative::{configure_seamless_camera_loop, morph_presets, randomize_preset_macros};

pub use modulation::{
    ActiveModulation, EnvelopeSmoother, ModulatedParameters, ModulationCurve, ModulationMapping,
    ModulationPolarity, ModulationSource, ModulationTarget, evaluate_mappings,
};
pub use package::{
    AssetKind, PackageAsset, PackageCreateOptions, PackageError, RenderConfigV1, RenderPackage,
};
pub use preset::{
    MacroParameterV1, MacroTargetV1, PresetOverridesV1, PresetSelectionV1, VisualPresetV1,
};

use serde::{Deserialize, Serialize};
use simulation::{Emitter, Force};
use std::{
    fs,
    path::{Path, PathBuf},
};
use thiserror::Error;

pub const PROJECT_VERSION: u32 = 1;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectV1 {
    pub project_version: u32,
    pub engine_version: String,
    pub seed: u64,
    pub fps: u32,
    pub duration_seconds: f32,
    pub particle_system: ParticleSystemV1,
    #[serde(default)]
    pub forces: Vec<Force>,
    pub camera: CameraV1,
    pub render_defaults: RenderDefaultsV1,
    #[serde(default)]
    pub render_mode: RenderModeV1,
    #[serde(default)]
    pub liquid_chrome: LiquidChromeV1,
    #[serde(default)]
    pub water_droplets: WaterDropletsV1,
    /// Optional independently configured systems, composited by ascending depth
    /// and then declaration order. Empty preserves the version-1 single-system path.
    #[serde(default)]
    pub layers: Vec<SceneLayerV1>,
    #[serde(default)]
    pub modulation_mappings: Vec<ModulationMapping>,
    #[serde(default)]
    pub automation_tracks: Vec<AutomationTrackV1>,
    #[serde(default)]
    pub scene_markers: Vec<SceneMarkerV1>,
    #[serde(default)]
    pub visual_preset: Option<PresetSelectionV1>,
    #[serde(default)]
    pub analysis_profile: Option<AnalysisProfileSelectionV1>,
    #[serde(default)]
    pub reaction_profile: Option<ReactionProfileSelectionV1>,
    #[serde(skip)]
    pub resolved_audio_profile: Option<ResolvedAudioProfile>,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RenderModeV1 {
    #[default]
    Particles,
    Volumetric,
    LiquidChrome,
    WaterDroplets,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LayerBlendModeV1 {
    #[default]
    Alpha,
    Add,
    Screen,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SceneLayerV1 {
    pub name: String,
    #[serde(default = "layer_visible")]
    pub visible: bool,
    #[serde(default)]
    pub depth: i32,
    #[serde(default)]
    pub blend: LayerBlendModeV1,
    #[serde(default = "layer_opacity")]
    pub opacity: f32,
    /// Scales particle count without changing the deterministic seed prefix.
    #[serde(default = "layer_quality_scale")]
    pub quality_scale: f32,
    pub render_mode: RenderModeV1,
    pub particle_system: ParticleSystemV1,
    #[serde(default)]
    pub forces: Vec<Force>,
    #[serde(default)]
    pub liquid_chrome: LiquidChromeV1,
    #[serde(default)]
    pub water_droplets: WaterDropletsV1,
    #[serde(default)]
    pub modulation_mappings: Vec<ModulationMapping>,
}

const fn layer_visible() -> bool {
    true
}
const fn layer_opacity() -> f32 {
    1.0
}
const fn layer_quality_scale() -> f32 {
    1.0
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WaterDropletsV1 {
    #[serde(default = "default_droplet_density")]
    pub density: f32,
    #[serde(default = "default_droplet_size")]
    pub size: f32,
    #[serde(default = "default_size_variation")]
    pub size_variation: f32,
    #[serde(default = "default_refraction_strength")]
    pub refraction_strength: f32,
    #[serde(default = "default_fresnel_strength")]
    pub fresnel_strength: f32,
    #[serde(default = "default_droplet_gravity")]
    pub gravity: f32,
    #[serde(default = "default_emission")]
    pub emission: f32,
}

impl Default for WaterDropletsV1 {
    fn default() -> Self {
        Self {
            density: default_droplet_density(),
            size: default_droplet_size(),
            size_variation: default_size_variation(),
            refraction_strength: default_refraction_strength(),
            fresnel_strength: default_fresnel_strength(),
            gravity: default_droplet_gravity(),
            emission: default_emission(),
        }
    }
}

const fn default_droplet_density() -> f32 {
    0.72
}
const fn default_droplet_size() -> f32 {
    0.34
}
const fn default_size_variation() -> f32 {
    0.65
}
const fn default_refraction_strength() -> f32 {
    0.08
}
const fn default_fresnel_strength() -> f32 {
    1.0
}
const fn default_droplet_gravity() -> f32 {
    0.12
}
const fn default_emission() -> f32 {
    0.45
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LiquidChromeV1 {
    /// Optional equirectangular PNG/JPEG/HDR environment, relative to the project.
    #[serde(default)]
    pub environment: Option<PathBuf>,
    #[serde(default = "default_environment_preset")]
    pub environment_preset: String,
    #[serde(default = "default_roughness")]
    pub roughness: f32,
    #[serde(default = "default_reflection_intensity")]
    pub reflection_intensity: f32,
    #[serde(default = "default_metallic")]
    pub metallic: f32,
    #[serde(default = "default_surface_scale")]
    pub surface_scale: f32,
}

impl Default for LiquidChromeV1 {
    fn default() -> Self {
        Self {
            environment: None,
            environment_preset: default_environment_preset(),
            roughness: default_roughness(),
            reflection_intensity: default_reflection_intensity(),
            metallic: default_metallic(),
            surface_scale: default_surface_scale(),
        }
    }
}
fn default_environment_preset() -> String {
    "milky_way".into()
}
const fn default_roughness() -> f32 {
    0.14
}
const fn default_reflection_intensity() -> f32 {
    1.35
}
const fn default_metallic() -> f32 {
    1.0
}
const fn default_surface_scale() -> f32 {
    1.0
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ResolvedAudioProfile {
    pub sensitivity: f32,
    pub frequency_weights: FeatureWeightsV1,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ParticleSystemV1 {
    pub count: u32,
    pub substeps: u32,
    pub emitter: Emitter,
    #[serde(default)]
    pub initialization: simulation::ParticleInitialization,
    #[serde(default)]
    pub boundary: simulation::ParticleBoundary,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CameraV1 {
    pub position: [f32; 3],
    pub target: [f32; 3],
    pub vertical_fov_degrees: f32,
    #[serde(default = "default_up")]
    pub up: [f32; 3],
    #[serde(default = "default_near")]
    pub near_plane: f32,
    #[serde(default = "default_far")]
    pub far_plane: f32,
    #[serde(default)]
    pub mode: CameraModeV1,
    #[serde(default)]
    pub orbit_degrees_per_second: f32,
    #[serde(default)]
    pub dolly_units_per_second: f32,
    #[serde(default)]
    pub drift_amplitude: [f32; 3],
    #[serde(default = "default_drift_frequency")]
    pub drift_frequency_hz: f32,
    #[serde(default)]
    pub fov_modulation_degrees: f32,
    #[serde(default)]
    pub shake_amplitude: f32,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CameraModeV1 {
    #[default]
    Static,
    Orbit,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CameraSample {
    pub position: [f32; 3],
    pub target: [f32; 3],
    pub up: [f32; 3],
    pub vertical_fov_degrees: f32,
    pub near_plane: f32,
    pub far_plane: f32,
}

impl CameraV1 {
    #[must_use]
    pub fn sample(
        &self,
        time_seconds: f32,
        fov_modulation: f32,
        shake_modulation: f32,
        seed: u64,
    ) -> CameraSample {
        let mut offset = subtract(self.position, self.target);
        if self.mode == CameraModeV1::Orbit {
            let angle = self.orbit_degrees_per_second.to_radians() * time_seconds;
            let (sin, cos) = angle.sin_cos();
            offset = [
                offset[0] * cos + offset[2] * sin,
                offset[1],
                -offset[0] * sin + offset[2] * cos,
            ];
        }
        let direction = normalize(offset);
        let distance = (length(offset) + self.dolly_units_per_second * time_seconds).max(0.01);
        let seed_bytes = seed.to_le_bytes();
        let phase = f32::from(u16::from_le_bytes([seed_bytes[0], seed_bytes[1]])) * 0.000_1;
        let drift_phase = time_seconds * self.drift_frequency_hz * std::f32::consts::TAU;
        let drift = [
            self.drift_amplitude[0] * (drift_phase + phase).sin(),
            self.drift_amplitude[1] * (drift_phase * 0.83 + phase * 1.7).sin(),
            self.drift_amplitude[2] * (drift_phase * 1.13 + phase * 2.3).sin(),
        ];
        let shake = self.shake_amplitude * shake_modulation.max(0.0);
        let shake_offset = [
            shake * (time_seconds * 37.0 + phase).sin(),
            shake * (time_seconds * 43.0 + phase * 2.0).sin(),
            shake * (time_seconds * 53.0 + phase * 3.0).sin(),
        ];
        CameraSample {
            position: add(
                add(add(self.target, scale(direction, distance)), drift),
                shake_offset,
            ),
            target: add(self.target, scale(shake_offset, 0.35)),
            up: self.up,
            vertical_fov_degrees: (self.vertical_fov_degrees
                + self.fov_modulation_degrees * fov_modulation)
                .clamp(1.0, 179.0),
            near_plane: self.near_plane,
            far_plane: self.far_plane,
        }
    }
}

impl Default for CameraV1 {
    fn default() -> Self {
        Self {
            position: [0.0, 0.0, 3.0],
            target: [0.0; 3],
            vertical_fov_degrees: 45.0,
            up: default_up(),
            near_plane: default_near(),
            far_plane: default_far(),
            mode: CameraModeV1::Static,
            orbit_degrees_per_second: 0.0,
            dolly_units_per_second: 0.0,
            drift_amplitude: [0.0; 3],
            drift_frequency_hz: default_drift_frequency(),
            fov_modulation_degrees: 0.0,
            shake_amplitude: 0.0,
        }
    }
}

const fn default_up() -> [f32; 3] {
    [0.0, 1.0, 0.0]
}
const fn default_near() -> f32 {
    0.01
}
const fn default_far() -> f32 {
    1_000.0
}
const fn default_drift_frequency() -> f32 {
    0.1
}

fn add(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}
fn subtract(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn scale(value: [f32; 3], factor: f32) -> [f32; 3] {
    [value[0] * factor, value[1] * factor, value[2] * factor]
}
fn length(value: [f32; 3]) -> f32 {
    (value[0] * value[0] + value[1] * value[1] + value[2] * value[2]).sqrt()
}
fn normalize(value: [f32; 3]) -> [f32; 3] {
    let magnitude = length(value);
    if magnitude > f32::EPSILON {
        scale(value, 1.0 / magnitude)
    } else {
        [0.0, 0.0, 1.0]
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RenderDefaultsV1 {
    pub width: u32,
    pub height: u32,
    pub particle_size_pixels: f32,
    pub background: [f64; 4],
}

#[derive(Debug, Error)]
pub enum ProjectError {
    #[error("failed to read project {path}: {source}")]
    Read {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to parse project {path}: {source}")]
    Parse {
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },
    #[error("project validation failed: {0}")]
    Validation(String),
    #[error("failed to serialize project: {0}")]
    Serialize(#[source] serde_json::Error),
    #[error("failed to write project {path}: {source}")]
    Write {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
}

impl ProjectV1 {
    /// Loads and validates a version-1 JSON project.
    ///
    /// # Errors
    ///
    /// Returns contextual I/O, JSON decoding, or validation errors.
    pub fn load(path: impl AsRef<Path>) -> Result<Self, ProjectError> {
        let path = path.as_ref();
        let json = fs::read_to_string(path).map_err(|source| ProjectError::Read {
            path: path.to_owned(),
            source,
        })?;
        let mut project: Self =
            serde_json::from_str(&json).map_err(|source| ProjectError::Parse {
                path: path.to_owned(),
                source,
            })?;
        if let Some(selection) = project.visual_preset.clone() {
            let preset_path = path
                .parent()
                .unwrap_or_else(|| Path::new("."))
                .join(&selection.source);
            let preset = VisualPresetV1::load(&preset_path)?;
            preset.apply(&mut project, &selection.overrides)?;
        }
        if let Some(selection) = project.analysis_profile.clone() {
            let profile_path = path
                .parent()
                .unwrap_or_else(|| Path::new("."))
                .join(&selection.source);
            let profile = AnalysisProfileV1::load(profile_path)?;
            let sensitivity = selection
                .overrides
                .sensitivity
                .unwrap_or(profile.sensitivity);
            let frequency_weights = selection
                .overrides
                .frequency_weights
                .unwrap_or(profile.frequency_weights);
            if !sensitivity.is_finite() || sensitivity < 0.0 || !frequency_weights.is_valid() {
                return Err(ProjectError::Validation(
                    "analysis profile overrides must be finite and non-negative".into(),
                ));
            }
            project.resolved_audio_profile = Some(ResolvedAudioProfile {
                sensitivity,
                frequency_weights,
            });
        }
        if let Some(selection) = project.reaction_profile.clone() {
            let profile_path = path
                .parent()
                .unwrap_or_else(|| Path::new("."))
                .join(&selection.source);
            let profile = ReactionProfileV1::load(profile_path)?;
            let attack = selection
                .overrides
                .attack_seconds
                .unwrap_or(profile.attack_seconds);
            let release = selection
                .overrides
                .release_seconds
                .unwrap_or(profile.release_seconds);
            if !attack.is_finite() || attack < 0.0 || !release.is_finite() || release < 0.0 {
                return Err(ProjectError::Validation(
                    "reaction profile smoothing overrides must be finite and non-negative".into(),
                ));
            }
            let mut mappings = selection
                .overrides
                .mappings
                .unwrap_or(profile.recommended_mappings);
            for mapping in &mut mappings {
                mapping.attack_seconds = attack;
                mapping.release_seconds = release;
            }
            project.modulation_mappings = mappings;
        }
        project.validate()?;
        Ok(project)
    }

    #[must_use]
    pub fn apply_analysis_profile(
        &self,
        features: audio_engine::AudioFeatureFrame,
    ) -> audio_engine::AudioFeatureFrame {
        self.resolved_audio_profile.map_or(features, |profile| {
            profile
                .frequency_weights
                .apply(features, profile.sensitivity)
        })
    }

    /// Validates values that serde's structural decoding cannot constrain.
    ///
    /// # Errors
    ///
    /// Returns the first schema invariant violation found.
    #[allow(clippy::too_many_lines)]
    pub fn validate(&self) -> Result<(), ProjectError> {
        if self.project_version != PROJECT_VERSION {
            return Err(ProjectError::Validation(format!(
                "unsupported project_version {}; expected {PROJECT_VERSION}",
                self.project_version
            )));
        }
        if self.engine_version.trim().is_empty() {
            return Err(ProjectError::Validation(
                "engine_version must not be empty".into(),
            ));
        }
        if self.fps == 0 {
            return Err(ProjectError::Validation(
                "fps must be greater than zero".into(),
            ));
        }
        if !self.duration_seconds.is_finite() || self.duration_seconds <= 0.0 {
            return Err(ProjectError::Validation(
                "duration_seconds must be positive and finite".into(),
            ));
        }
        if self.particle_system.count == 0 || self.particle_system.substeps == 0 {
            return Err(ProjectError::Validation(
                "particle count and substeps must be greater than zero".into(),
            ));
        }
        if let simulation::ParticleInitialization::GalacticDisk {
            radius,
            thickness,
            lifetime_seconds,
            spawn_spread_seconds,
            lifetime_variation,
        } = self.particle_system.initialization
            && (!radius.is_finite()
                || radius <= 0.0
                || !thickness.is_finite()
                || thickness < 0.0
                || !lifetime_seconds.is_finite()
                || lifetime_seconds <= 5.0
                || !spawn_spread_seconds.is_finite()
                || spawn_spread_seconds < 0.0
                || !lifetime_variation.is_finite()
                || !(0.0..1.0).contains(&lifetime_variation))
        {
            return Err(ProjectError::Validation(
                "galactic disk requires positive radius, non-negative thickness/spawn spread, lifetime greater than five seconds, and lifetime variation in [0, 1)".into(),
            ));
        }
        if self.render_defaults.width == 0 || self.render_defaults.height == 0 {
            return Err(ProjectError::Validation(
                "render dimensions must be greater than zero".into(),
            ));
        }
        if !self.render_defaults.particle_size_pixels.is_finite()
            || self.render_defaults.particle_size_pixels <= 0.0
        {
            return Err(ProjectError::Validation(
                "particle size must be positive and finite".into(),
            ));
        }
        if !self.camera.vertical_fov_degrees.is_finite()
            || !(1.0..179.0).contains(&self.camera.vertical_fov_degrees)
        {
            return Err(ProjectError::Validation(
                "camera vertical FOV must be between 1 and 179 degrees".into(),
            ));
        }
        if !self.camera.near_plane.is_finite()
            || !self.camera.far_plane.is_finite()
            || self.camera.near_plane <= 0.0
            || self.camera.far_plane <= self.camera.near_plane
        {
            return Err(ProjectError::Validation(
                "camera planes must be finite, positive, and ordered".into(),
            ));
        }
        let camera_values = [
            self.camera.orbit_degrees_per_second,
            self.camera.dolly_units_per_second,
            self.camera.drift_frequency_hz,
            self.camera.fov_modulation_degrees,
            self.camera.shake_amplitude,
        ];
        if !camera_values.iter().all(|value| value.is_finite())
            || !self.camera.position.iter().all(|value| value.is_finite())
            || !self.camera.target.iter().all(|value| value.is_finite())
            || !self.camera.up.iter().all(|value| value.is_finite())
            || !self
                .camera
                .drift_amplitude
                .iter()
                .all(|value| value.is_finite())
        {
            return Err(ProjectError::Validation(
                "camera parameters must be finite".into(),
            ));
        }
        if length(self.camera.up) <= f32::EPSILON
            || length(subtract(self.camera.position, self.camera.target)) <= f32::EPSILON
            || self.camera.drift_frequency_hz < 0.0
            || self.camera.shake_amplitude < 0.0
        {
            return Err(ProjectError::Validation(
                "camera direction vectors must be non-zero and procedural rates/amplitudes non-negative"
                    .into(),
            ));
        }
        if !self
            .render_defaults
            .background
            .iter()
            .all(|value| value.is_finite() && (0.0..=1.0).contains(value))
        {
            return Err(ProjectError::Validation(
                "background channels must be finite values from 0 to 1".into(),
            ));
        }
        let material = &self.liquid_chrome;
        if !material.roughness.is_finite()
            || !(0.0..=1.0).contains(&material.roughness)
            || !material.metallic.is_finite()
            || !(0.0..=1.0).contains(&material.metallic)
            || !material.reflection_intensity.is_finite()
            || material.reflection_intensity < 0.0
            || !material.surface_scale.is_finite()
            || material.surface_scale <= 0.0
            || (material.environment.is_none() && material.environment_preset != "milky_way")
        {
            return Err(ProjectError::Validation(
                "liquid chrome material values or environment preset are invalid".into(),
            ));
        }
        let droplets = &self.water_droplets;
        if ![
            droplets.density,
            droplets.size,
            droplets.size_variation,
            droplets.refraction_strength,
            droplets.fresnel_strength,
            droplets.gravity,
            droplets.emission,
        ]
        .iter()
        .all(|value| value.is_finite())
            || droplets.density < 0.0
            || droplets.size <= 0.0
            || !(0.0..=1.0).contains(&droplets.size_variation)
            || droplets.refraction_strength < 0.0
            || droplets.fresnel_strength < 0.0
            || droplets.gravity < 0.0
            || !(0.0..=1.0).contains(&droplets.emission)
        {
            return Err(ProjectError::Validation(
                "water droplet properties must be finite and inside their documented ranges".into(),
            ));
        }
        for layer in &self.layers {
            if layer.name.trim().is_empty()
                || !layer.opacity.is_finite()
                || !(0.0..=1.0).contains(&layer.opacity)
                || !layer.quality_scale.is_finite()
                || layer.quality_scale <= 0.0
                || layer.particle_system.count == 0
                || layer.particle_system.substeps == 0
            {
                return Err(ProjectError::Validation(
                    "scene layers require a name, opacity in 0..=1, positive quality scale, particle count, and substeps".into(),
                ));
            }
            for mapping in &layer.modulation_mappings {
                let values = [
                    mapping.amount,
                    mapping.offset,
                    mapping.minimum,
                    mapping.maximum,
                    mapping.attack_seconds,
                    mapping.release_seconds,
                ];
                if !values.iter().all(|value| value.is_finite())
                    || mapping.minimum > mapping.maximum
                    || mapping.attack_seconds < 0.0
                    || mapping.release_seconds < 0.0
                {
                    return Err(ProjectError::Validation(format!(
                        "layer '{}' has invalid modulation values",
                        layer.name
                    )));
                }
            }
        }
        for mapping in &self.modulation_mappings {
            let values = [
                mapping.amount,
                mapping.offset,
                mapping.minimum,
                mapping.maximum,
                mapping.attack_seconds,
                mapping.release_seconds,
            ];
            if !values.iter().all(|value| value.is_finite())
                || mapping.minimum > mapping.maximum
                || mapping.attack_seconds < 0.0
                || mapping.release_seconds < 0.0
            {
                return Err(ProjectError::Validation(
                    "modulation values must be finite, min must not exceed max, and attack/release must be non-negative".into(),
                ));
            }
        }
        for track in &self.automation_tracks {
            if track.keyframes.iter().any(|keyframe| {
                !keyframe.time_seconds.is_finite()
                    || !keyframe.value.is_finite()
                    || keyframe.time_seconds < 0.0
                    || keyframe.time_seconds > self.duration_seconds
            }) || track
                .keyframes
                .windows(2)
                .any(|pair| pair[0].time_seconds >= pair[1].time_seconds)
            {
                return Err(ProjectError::Validation("automation keyframes must be finite, ordered, unique, and inside the project duration".into()));
            }
        }
        if self.scene_markers.iter().any(|marker| {
            marker.label.trim().is_empty()
                || !marker.time_seconds.is_finite()
                || marker.time_seconds < 0.0
                || marker.time_seconds > self.duration_seconds
        }) {
            return Err(ProjectError::Validation(
                "scene markers must have labels and times inside the project duration".into(),
            ));
        }
        Ok(())
    }

    /// Validates and saves pretty-printed JSON.
    ///
    /// # Errors
    ///
    /// Returns validation, serialization, or contextual file-write errors.
    pub fn save(&self, path: impl AsRef<Path>) -> Result<(), ProjectError> {
        self.validate()?;
        let json = serde_json::to_string_pretty(self).map_err(ProjectError::Serialize)?;
        let path = path.as_ref();
        fs::write(path, format!("{json}\n")).map_err(|source| ProjectError::Write {
            path: path.to_owned(),
            source,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use simulation::RespawnPolicy;

    fn sample() -> ProjectV1 {
        ProjectV1 {
            project_version: PROJECT_VERSION,
            engine_version: "0.1.0".into(),
            seed: 42,
            fps: 60,
            duration_seconds: 10.0,
            particle_system: ParticleSystemV1 {
                count: 1_000,
                substeps: 2,
                emitter: Emitter::Burst {
                    count: 1_000,
                    frame: 0,
                    respawn: RespawnPolicy::Loop,
                },
                initialization: simulation::ParticleInitialization::default(),
                boundary: simulation::ParticleBoundary::default(),
            },
            forces: vec![Force::Drag { coefficient: 0.1 }],
            camera: CameraV1 {
                position: [0.0, 0.0, 3.0],
                target: [0.0; 3],
                vertical_fov_degrees: 45.0,
                ..CameraV1::default()
            },
            render_defaults: RenderDefaultsV1 {
                width: 1920,
                height: 1080,
                particle_size_pixels: 2.0,
                background: [0.0, 0.0, 0.0, 1.0],
            },
            render_mode: RenderModeV1::Particles,
            liquid_chrome: LiquidChromeV1::default(),
            water_droplets: WaterDropletsV1::default(),
            layers: Vec::new(),
            modulation_mappings: Vec::new(),
            automation_tracks: Vec::new(),
            scene_markers: Vec::new(),
            visual_preset: None,
            analysis_profile: None,
            reaction_profile: None,
            resolved_audio_profile: None,
        }
    }

    #[test]
    fn project_round_trips_without_losing_scene_state() {
        let project = sample();
        let json = serde_json::to_string(&project).unwrap();
        assert_eq!(serde_json::from_str::<ProjectV1>(&json).unwrap(), project);
    }

    #[test]
    fn layered_scene_round_trips_and_validates() {
        let mut project = sample();
        project.layers.push(SceneLayerV1 {
            name: "stars".into(),
            visible: true,
            depth: 2,
            blend: LayerBlendModeV1::Add,
            opacity: 0.8,
            quality_scale: 0.5,
            render_mode: RenderModeV1::Particles,
            particle_system: project.particle_system.clone(),
            forces: Vec::new(),
            liquid_chrome: LiquidChromeV1::default(),
            water_droplets: WaterDropletsV1::default(),
            modulation_mappings: Vec::new(),
        });
        project.validate().unwrap();
        let json = serde_json::to_string(&project).unwrap();
        assert_eq!(serde_json::from_str::<ProjectV1>(&json).unwrap(), project);
    }

    #[test]
    fn repository_layered_example_is_valid() {
        let project: ProjectV1 =
            serde_json::from_str(include_str!("../../../examples/multi-layer.rustique.json"))
                .unwrap();
        project.validate().unwrap();
        assert!(project.layers.len() >= 3);
    }

    #[test]
    fn rejects_unknown_versions_and_invalid_values() {
        let mut project = sample();
        project.project_version = 2;
        assert!(
            project
                .validate()
                .unwrap_err()
                .to_string()
                .contains("unsupported project_version")
        );
        project.project_version = PROJECT_VERSION;
        project.fps = 0;
        assert!(project.validate().unwrap_err().to_string().contains("fps"));
    }

    #[test]
    fn repository_example_is_valid() {
        let project: ProjectV1 =
            serde_json::from_str(include_str!("../../../examples/star-orbit.rustique.json"))
                .unwrap();
        project.validate().unwrap();
    }

    #[test]
    fn procedural_camera_is_deterministic_and_modulatable() {
        let camera = CameraV1 {
            mode: CameraModeV1::Orbit,
            orbit_degrees_per_second: 30.0,
            fov_modulation_degrees: 10.0,
            shake_amplitude: 0.1,
            ..CameraV1::default()
        };
        let first = camera.sample(1.0, 0.5, 0.75, 42);
        let repeated = camera.sample(1.0, 0.5, 0.75, 42);
        assert_eq!(first, repeated);
        assert!((first.vertical_fov_degrees - 50.0).abs() < f32::EPSILON);
        assert!(
            first
                .position
                .iter()
                .zip(camera.position)
                .any(|(actual, original)| (actual - original).abs() > f32::EPSILON)
        );
    }
}
