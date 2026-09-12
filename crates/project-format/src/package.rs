//! Portable, directory-based render packages.

use std::{
    collections::HashSet,
    fs,
    io::{self, Read},
    path::{Component, Path, PathBuf},
};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;

use crate::{PROJECT_VERSION, ProjectV1};

pub const PACKAGE_VERSION: u32 = 1;
const MANIFEST_NAME: &str = "manifest.json";
const PROJECT_NAME: &str = "project.json";
const RENDER_NAME: &str = "render.json";

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AssetKind {
    Audio,
    Texture,
    Hdri,
    Mesh,
    ProjectDependency,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PackageAsset {
    pub kind: AssetKind,
    pub source: PathBuf,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RenderConfigV1 {
    pub render_version: u32,
    pub audio: PathBuf,
    pub width: u32,
    pub height: u32,
}

#[derive(Clone, Debug)]
pub struct PackageCreateOptions {
    pub project: PathBuf,
    pub audio: PathBuf,
    pub output: PathBuf,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub assets: Vec<PackageAsset>,
}

#[derive(Clone, Debug)]
pub struct RenderPackage {
    root: PathBuf,
    pub project: ProjectV1,
    pub render: RenderConfigV1,
}

impl RenderPackage {
    /// Creates a self-contained directory whose name conventionally ends in
    /// `.rustiqueproject`.
    ///
    /// # Errors
    ///
    /// Returns an error when the source project or an asset is invalid, the
    /// destination is non-empty, or package files cannot be written.
    pub fn create(options: &PackageCreateOptions) -> Result<Self, PackageError> {
        ensure_empty_destination(&options.output)?;
        let raw_project = ProjectV1::load(&options.project)?;
        fs::create_dir_all(options.output.join("assets")).map_err(|source| PackageError::Io {
            path: options.output.clone(),
            source,
        })?;

        let mut project: ProjectV1 =
            serde_json::from_slice(&fs::read(&options.project).map_err(|source| {
                PackageError::Io {
                    path: options.project.clone(),
                    source,
                }
            })?)
            .map_err(|source| PackageError::Json {
                path: options.project.clone(),
                source,
            })?;
        let project_dir = options.project.parent().unwrap_or_else(|| Path::new("."));
        if let Some(selection) = &mut project.visual_preset {
            selection.source = copy_asset(
                project_dir.join(&selection.source),
                &options.output,
                AssetKind::ProjectDependency,
            )?;
        }
        if let Some(selection) = &mut project.analysis_profile {
            selection.source = copy_asset(
                project_dir.join(&selection.source),
                &options.output,
                AssetKind::ProjectDependency,
            )?;
        }
        if let Some(selection) = &mut project.reaction_profile {
            selection.source = copy_asset(
                project_dir.join(&selection.source),
                &options.output,
                AssetKind::ProjectDependency,
            )?;
        }
        let audio = copy_asset(&options.audio, &options.output, AssetKind::Audio)?;
        for asset in &options.assets {
            copy_asset(&asset.source, &options.output, asset.kind)?;
        }
        project.save(options.output.join(PROJECT_NAME))?;
        let render = RenderConfigV1 {
            render_version: 1,
            audio,
            width: options.width.unwrap_or(raw_project.render_defaults.width),
            height: options.height.unwrap_or(raw_project.render_defaults.height),
        };
        if render.width == 0 || render.height == 0 {
            return Err(PackageError::Invalid(
                "render dimensions must be greater than zero".into(),
            ));
        }
        write_json(options.output.join(RENDER_NAME), &render)?;
        write_manifest(&options.output)?;
        Self::load(&options.output)
    }

    /// Loads a package and verifies its layout, relative paths, and fingerprints.
    ///
    /// # Errors
    ///
    /// Returns an error when package files are missing, malformed, unsafe, or
    /// do not match their recorded SHA-256 fingerprints.
    pub fn load(root: impl AsRef<Path>) -> Result<Self, PackageError> {
        let root = root.as_ref();
        let manifest: Manifest = read_json(&root.join(MANIFEST_NAME))?;
        if manifest.package_version != PACKAGE_VERSION {
            return Err(PackageError::Invalid(format!(
                "unsupported package_version {}",
                manifest.package_version
            )));
        }
        let mut seen = HashSet::new();
        for file in &manifest.files {
            validate_relative(&file.path)?;
            if !seen.insert(file.path.clone()) {
                return Err(PackageError::Invalid(format!(
                    "duplicate manifest path {}",
                    file.path.display()
                )));
            }
            let actual = fingerprint(&root.join(&file.path))?;
            if actual != file.sha256 {
                return Err(PackageError::Invalid(format!(
                    "checksum mismatch for {}",
                    file.path.display()
                )));
            }
        }
        for required in [Path::new(PROJECT_NAME), Path::new(RENDER_NAME)] {
            if !seen.contains(required) {
                return Err(PackageError::Invalid(format!(
                    "manifest is missing {}",
                    required.display()
                )));
            }
        }
        let mut actual_paths = Vec::new();
        collect_files(root, root, &mut actual_paths)?;
        actual_paths.retain(|path| path != Path::new(MANIFEST_NAME));
        if actual_paths.iter().any(|path| !seen.contains(path)) {
            return Err(PackageError::Invalid(
                "package contains a file that is not fingerprinted in the manifest".into(),
            ));
        }
        let render: RenderConfigV1 = read_json(&root.join(RENDER_NAME))?;
        if render.render_version != 1 || render.width == 0 || render.height == 0 {
            return Err(PackageError::Invalid("invalid render config".into()));
        }
        validate_relative(&render.audio)?;
        if !seen.contains(&render.audio) {
            return Err(PackageError::Invalid(
                "render audio is not fingerprinted in the manifest".into(),
            ));
        }
        let raw_project: ProjectV1 = read_json(&root.join(PROJECT_NAME))?;
        for source in [
            raw_project
                .visual_preset
                .as_ref()
                .map(|selection| &selection.source),
            raw_project
                .analysis_profile
                .as_ref()
                .map(|selection| &selection.source),
            raw_project
                .reaction_profile
                .as_ref()
                .map(|selection| &selection.source),
        ]
        .into_iter()
        .flatten()
        {
            validate_relative(source)?;
            if !seen.contains(source) {
                return Err(PackageError::Invalid(format!(
                    "project dependency {} is not fingerprinted in the manifest",
                    source.display()
                )));
            }
        }
        let project = ProjectV1::load(root.join(PROJECT_NAME))?;
        if project.project_version != PROJECT_VERSION {
            return Err(PackageError::Invalid(
                "package project version is unsupported".into(),
            ));
        }
        Ok(Self {
            root: root.to_owned(),
            project,
            render,
        })
    }

    #[must_use]
    pub fn project_path(&self) -> PathBuf {
        self.root.join(PROJECT_NAME)
    }

    #[must_use]
    pub fn audio_path(&self) -> PathBuf {
        self.root.join(&self.render.audio)
    }
}

#[derive(Debug, Error)]
pub enum PackageError {
    #[error(transparent)]
    Project(#[from] crate::ProjectError),
    #[error("package I/O failed for {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("invalid JSON in {path}: {source}")]
    Json {
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },
    #[error("invalid render package: {0}")]
    Invalid(String),
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    package_version: u32,
    files: Vec<ManifestFile>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ManifestFile {
    path: PathBuf,
    sha256: String,
}

fn ensure_empty_destination(path: &Path) -> Result<(), PackageError> {
    if path.exists()
        && fs::read_dir(path)
            .map_err(|source| PackageError::Io {
                path: path.to_owned(),
                source,
            })?
            .next()
            .is_some()
    {
        return Err(PackageError::Invalid(format!(
            "destination {} is not empty",
            path.display()
        )));
    }
    Ok(())
}

fn copy_asset(
    source: impl AsRef<Path>,
    root: &Path,
    kind: AssetKind,
) -> Result<PathBuf, PackageError> {
    let source = source.as_ref();
    if !source.is_file() {
        return Err(PackageError::Invalid(format!(
            "asset {} is not a file",
            source.display()
        )));
    }
    let name = source.file_name().ok_or_else(|| {
        PackageError::Invalid(format!("asset {} has no file name", source.display()))
    })?;
    let category = match kind {
        AssetKind::Audio => "audio",
        AssetKind::Texture => "textures",
        AssetKind::Hdri => "hdris",
        AssetKind::Mesh => "meshes",
        AssetKind::ProjectDependency => "project",
    };
    let directory = root.join("assets").join(category);
    fs::create_dir_all(&directory).map_err(|source| PackageError::Io {
        path: directory.clone(),
        source,
    })?;
    let destination = unique_destination(&directory, name);
    fs::copy(source, &destination).map_err(|error| PackageError::Io {
        path: source.to_owned(),
        source: error,
    })?;
    destination
        .strip_prefix(root)
        .map(portable_path)
        .map_err(|_| PackageError::Invalid("asset escaped package root".into()))
}

fn unique_destination(directory: &Path, name: &std::ffi::OsStr) -> PathBuf {
    let first = directory.join(name);
    if !first.exists() {
        return first;
    }
    let path = Path::new(name);
    let stem = path.file_stem().unwrap_or(name).to_string_lossy();
    let extension = path
        .extension()
        .map(|value| format!(".{}", value.to_string_lossy()))
        .unwrap_or_default();
    let mut index = 2_u64;
    loop {
        let candidate = directory.join(format!("{stem}-{index}{extension}"));
        if !candidate.exists() {
            return candidate;
        }
        index = index.saturating_add(1);
    }
}

fn write_manifest(root: &Path) -> Result<(), PackageError> {
    let mut paths = Vec::new();
    collect_files(root, root, &mut paths)?;
    paths.retain(|path| path != Path::new(MANIFEST_NAME));
    paths.sort();
    let files = paths
        .into_iter()
        .map(|path| {
            let sha256 = fingerprint(&root.join(&path))?;
            Ok(ManifestFile { path, sha256 })
        })
        .collect::<Result<Vec<_>, PackageError>>()?;
    write_json(
        root.join(MANIFEST_NAME),
        &Manifest {
            package_version: PACKAGE_VERSION,
            files,
        },
    )
}

fn collect_files(
    root: &Path,
    directory: &Path,
    output: &mut Vec<PathBuf>,
) -> Result<(), PackageError> {
    for entry in fs::read_dir(directory).map_err(|source| PackageError::Io {
        path: directory.to_owned(),
        source,
    })? {
        let entry = entry.map_err(|source| PackageError::Io {
            path: directory.to_owned(),
            source,
        })?;
        let path = entry.path();
        if path.is_dir() {
            collect_files(root, &path, output)?;
        } else if path.is_file() {
            output.push(portable_path(
                path.strip_prefix(root).expect("walk remains below root"),
            ));
        }
    }
    Ok(())
}

fn portable_path(path: &Path) -> PathBuf {
    PathBuf::from(
        path.components()
            .filter_map(|component| match component {
                Component::Normal(value) => Some(value.to_string_lossy()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("/"),
    )
}

fn fingerprint(path: &Path) -> Result<String, PackageError> {
    let mut file = fs::File::open(path).map_err(|source| PackageError::Io {
        path: path.to_owned(),
        source,
    })?;
    let mut digest = Sha256::new();
    let mut buffer = vec![0_u8; 64 * 1024];
    loop {
        let count = file.read(&mut buffer).map_err(|source| PackageError::Io {
            path: path.to_owned(),
            source,
        })?;
        if count == 0 {
            break;
        }
        digest.update(&buffer[..count]);
    }
    Ok(format!("{:x}", digest.finalize()))
}

fn validate_relative(path: &Path) -> Result<(), PackageError> {
    if path.as_os_str().is_empty()
        || path.is_absolute()
        || path
            .components()
            .any(|part| !matches!(part, Component::Normal(_)))
    {
        return Err(PackageError::Invalid(format!(
            "path must be package-relative without traversal: {}",
            path.display()
        )));
    }
    Ok(())
}

fn read_json<T: for<'de> Deserialize<'de>>(path: &Path) -> Result<T, PackageError> {
    let bytes = fs::read(path).map_err(|source| PackageError::Io {
        path: path.to_owned(),
        source,
    })?;
    serde_json::from_slice(&bytes).map_err(|source| PackageError::Json {
        path: path.to_owned(),
        source,
    })
}

fn write_json(path: PathBuf, value: &impl Serialize) -> Result<(), PackageError> {
    let json = serde_json::to_vec_pretty(value).map_err(|source| PackageError::Json {
        path: path.clone(),
        source,
    })?;
    fs::write(&path, json).map_err(|source| PackageError::Io { path, source })
}

#[cfg(test)]
mod tests {
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::*;

    fn test_directory(name: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("rustique-{name}-{unique}"))
    }

    #[test]
    fn package_survives_move_without_original_assets() {
        let source = test_directory("source");
        let destination = test_directory("package").with_extension("rustiqueproject");
        let moved = test_directory("moved").with_extension("rustiqueproject");
        fs::create_dir_all(source.join("profiles")).unwrap();
        let mut project: ProjectV1 =
            serde_json::from_str(include_str!("../../../examples/star-orbit.rustique.json"))
                .unwrap();
        project.visual_preset = None;
        project.analysis_profile = None;
        project.reaction_profile = None;
        project.save(source.join("project.json")).unwrap();
        fs::write(source.join("audio.wav"), b"test audio bytes").unwrap();
        fs::write(source.join("texture.png"), b"test texture bytes").unwrap();

        RenderPackage::create(&PackageCreateOptions {
            project: source.join("project.json"),
            audio: source.join("audio.wav"),
            output: destination.clone(),
            width: Some(640),
            height: Some(360),
            assets: vec![PackageAsset {
                kind: AssetKind::Texture,
                source: source.join("texture.png"),
            }],
        })
        .unwrap();
        fs::remove_dir_all(&source).unwrap();
        fs::rename(&destination, &moved).unwrap();

        let package = RenderPackage::load(&moved).unwrap();
        assert_eq!((package.render.width, package.render.height), (640, 360));
        assert!(package.render.audio.to_string_lossy().contains('/'));
        assert!(package.audio_path().is_file());
        fs::remove_dir_all(moved).unwrap();
    }

    #[test]
    fn validation_detects_modified_asset() {
        let source = test_directory("checksum-source");
        let destination = test_directory("checksum-package").with_extension("rustiqueproject");
        fs::create_dir_all(&source).unwrap();
        let mut project: ProjectV1 =
            serde_json::from_str(include_str!("../../../examples/star-orbit.rustique.json"))
                .unwrap();
        project.visual_preset = None;
        project.analysis_profile = None;
        project.reaction_profile = None;
        project.save(source.join("project.json")).unwrap();
        fs::write(source.join("audio.wav"), b"original").unwrap();
        let package = RenderPackage::create(&PackageCreateOptions {
            project: source.join("project.json"),
            audio: source.join("audio.wav"),
            output: destination.clone(),
            width: None,
            height: None,
            assets: Vec::new(),
        })
        .unwrap();
        fs::write(package.audio_path(), b"modified").unwrap();
        let error = RenderPackage::load(&destination).unwrap_err();
        assert!(error.to_string().contains("checksum mismatch"));
        fs::remove_dir_all(source).unwrap();
        fs::remove_dir_all(destination).unwrap();
    }

    #[test]
    fn selected_profiles_are_rewritten_into_the_package() {
        let source = test_directory("profile-source");
        let destination = test_directory("profile-package").with_extension("rustiqueproject");
        fs::create_dir_all(&source).unwrap();
        fs::write(source.join("audio.wav"), b"audio").unwrap();
        let repository = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
        let package = RenderPackage::create(&PackageCreateOptions {
            project: repository.join("examples/star-orbit.rustique.json"),
            audio: source.join("audio.wav"),
            output: destination.clone(),
            width: None,
            height: None,
            assets: Vec::new(),
        })
        .unwrap();
        let preset = package.project.visual_preset.unwrap().source;
        assert!(preset.to_string_lossy().starts_with("assets/project/"));
        assert!(destination.join(preset).is_file());
        fs::remove_dir_all(source).unwrap();
        fs::remove_dir_all(destination).unwrap();
    }
}
