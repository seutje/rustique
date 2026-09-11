//! Versioned, renderer-independent Rustique project representation.

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
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ParticleSystemV1 {
    pub count: u32,
    pub substeps: u32,
    pub emitter: Emitter,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CameraV1 {
    pub position: [f32; 3],
    pub target: [f32; 3],
    pub vertical_fov_degrees: f32,
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
        let project: Self = serde_json::from_str(&json).map_err(|source| ProjectError::Parse {
            path: path.to_owned(),
            source,
        })?;
        project.validate()?;
        Ok(project)
    }

    /// Validates values that serde's structural decoding cannot constrain.
    ///
    /// # Errors
    ///
    /// Returns the first schema invariant violation found.
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
            },
            forces: vec![Force::Drag { coefficient: 0.1 }],
            camera: CameraV1 {
                position: [0.0, 0.0, 3.0],
                target: [0.0; 3],
                vertical_fov_degrees: 45.0,
            },
            render_defaults: RenderDefaultsV1 {
                width: 1920,
                height: 1080,
                particle_size_pixels: 2.0,
                background: [0.0, 0.0, 0.0, 1.0],
            },
        }
    }

    #[test]
    fn project_round_trips_without_losing_scene_state() {
        let project = sample();
        let json = serde_json::to_string(&project).unwrap();
        assert_eq!(serde_json::from_str::<ProjectV1>(&json).unwrap(), project);
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
}
