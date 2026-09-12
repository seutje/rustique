use std::{
    fs,
    path::{Path, PathBuf},
};

use audio_engine::AudioFeatureFrame;
use serde::{Deserialize, Serialize};

use crate::{ModulationMapping, ProjectError};

pub const AUDIO_PROFILE_VERSION: u32 = 1;

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FeatureWeightsV1 {
    pub rms: f32,
    pub sub: f32,
    pub bass: f32,
    pub low_mids: f32,
    pub mids: f32,
    pub high_mids: f32,
    pub highs: f32,
    pub spectral_centroid: f32,
    pub spectral_flux: f32,
    pub transient: f32,
}

impl Default for FeatureWeightsV1 {
    fn default() -> Self {
        Self {
            rms: 1.0,
            sub: 1.0,
            bass: 1.0,
            low_mids: 1.0,
            mids: 1.0,
            high_mids: 1.0,
            highs: 1.0,
            spectral_centroid: 1.0,
            spectral_flux: 1.0,
            transient: 1.0,
        }
    }
}

impl FeatureWeightsV1 {
    pub(crate) fn is_valid(self) -> bool {
        [
            self.rms,
            self.sub,
            self.bass,
            self.low_mids,
            self.mids,
            self.high_mids,
            self.highs,
            self.spectral_centroid,
            self.spectral_flux,
            self.transient,
        ]
        .iter()
        .all(|value| value.is_finite() && *value >= 0.0)
    }

    #[must_use]
    pub fn apply(self, mut frame: AudioFeatureFrame, sensitivity: f32) -> AudioFeatureFrame {
        let weighted = |value: f32, weight: f32| (value * weight * sensitivity).clamp(0.0, 1.0);
        frame.rms = weighted(frame.rms, self.rms);
        frame.bands.sub = weighted(frame.bands.sub, self.sub);
        frame.bands.bass = weighted(frame.bands.bass, self.bass);
        frame.bands.low_mids = weighted(frame.bands.low_mids, self.low_mids);
        frame.bands.mids = weighted(frame.bands.mids, self.mids);
        frame.bands.high_mids = weighted(frame.bands.high_mids, self.high_mids);
        frame.bands.highs = weighted(frame.bands.highs, self.highs);
        frame.spectral_centroid = weighted(frame.spectral_centroid, self.spectral_centroid);
        frame.spectral_flux = weighted(frame.spectral_flux, self.spectral_flux);
        frame.transient_strength = weighted(frame.transient_strength, self.transient);
        frame
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AnalysisProfileV1 {
    pub profile_version: u32,
    pub id: String,
    pub display_name: String,
    #[serde(default)]
    pub description: String,
    pub sensitivity: f32,
    pub frequency_weights: FeatureWeightsV1,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AnalysisProfileOverridesV1 {
    pub sensitivity: Option<f32>,
    pub frequency_weights: Option<FeatureWeightsV1>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AnalysisProfileSelectionV1 {
    pub source: PathBuf,
    #[serde(default)]
    pub overrides: AnalysisProfileOverridesV1,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReactionProfileV1 {
    pub profile_version: u32,
    pub id: String,
    pub display_name: String,
    #[serde(default)]
    pub description: String,
    pub attack_seconds: f32,
    pub release_seconds: f32,
    #[serde(default)]
    pub recommended_mappings: Vec<ModulationMapping>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReactionProfileOverridesV1 {
    pub attack_seconds: Option<f32>,
    pub release_seconds: Option<f32>,
    pub mappings: Option<Vec<ModulationMapping>>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReactionProfileSelectionV1 {
    pub source: PathBuf,
    #[serde(default)]
    pub overrides: ReactionProfileOverridesV1,
}

fn load_json<T: for<'de> Deserialize<'de>>(path: &Path) -> Result<T, ProjectError> {
    let json = fs::read_to_string(path).map_err(|source| ProjectError::Read {
        path: path.to_owned(),
        source,
    })?;
    serde_json::from_str(&json).map_err(|source| ProjectError::Parse {
        path: path.to_owned(),
        source,
    })
}

impl AnalysisProfileV1 {
    /// Loads and validates an analysis profile JSON file.
    ///
    /// # Errors
    ///
    /// Returns contextual I/O, JSON decoding, or schema validation errors.
    pub fn load(path: impl AsRef<Path>) -> Result<Self, ProjectError> {
        let profile: Self = load_json(path.as_ref())?;
        profile.validate()?;
        Ok(profile)
    }

    fn validate(&self) -> Result<(), ProjectError> {
        if self.profile_version != AUDIO_PROFILE_VERSION
            || self.id.trim().is_empty()
            || !self.sensitivity.is_finite()
            || self.sensitivity < 0.0
            || !self.frequency_weights.is_valid()
        {
            return Err(ProjectError::Validation(format!(
                "invalid analysis profile '{}'",
                self.id
            )));
        }
        Ok(())
    }
}

impl ReactionProfileV1 {
    /// Loads and validates a reaction profile JSON file.
    ///
    /// # Errors
    ///
    /// Returns contextual I/O, JSON decoding, or schema validation errors.
    pub fn load(path: impl AsRef<Path>) -> Result<Self, ProjectError> {
        let profile: Self = load_json(path.as_ref())?;
        profile.validate()?;
        Ok(profile)
    }

    fn validate(&self) -> Result<(), ProjectError> {
        if self.profile_version != AUDIO_PROFILE_VERSION
            || self.id.trim().is_empty()
            || !self.attack_seconds.is_finite()
            || self.attack_seconds < 0.0
            || !self.release_seconds.is_finite()
            || self.release_seconds < 0.0
        {
            return Err(ProjectError::Validation(format!(
                "invalid reaction profile '{}'",
                self.id
            )));
        }
        Ok(())
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
    fn repository_profiles_are_editable_data_and_load() {
        for name in ["techno", "drum-and-bass", "ambient", "cinematic"] {
            AnalysisProfileV1::load(repository_path(&format!("profiles/analysis/{name}.json")))
                .unwrap();
        }
        for name in ["punchy", "fluid", "dreamy", "aggressive"] {
            let profile =
                ReactionProfileV1::load(repository_path(&format!("profiles/reaction/{name}.json")))
                    .unwrap();
            assert!(!profile.recommended_mappings.is_empty());
        }
    }

    #[test]
    fn analysis_profiles_change_the_same_feature_sample() {
        let techno =
            AnalysisProfileV1::load(repository_path("profiles/analysis/techno.json")).unwrap();
        let ambient =
            AnalysisProfileV1::load(repository_path("profiles/analysis/ambient.json")).unwrap();
        let mut input = AudioFeatureFrame::default();
        input.bands.bass = 0.5;
        input.transient_strength = 0.5;
        assert_ne!(
            techno.frequency_weights.apply(input, techno.sensitivity),
            ambient.frequency_weights.apply(input, ambient.sensitivity)
        );
    }
}
