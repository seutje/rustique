use crate::{AnalysisConfig, AudioAnalysis, AudioError, analyze, decode_audio};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::{Path, PathBuf},
    time::UNIX_EPOCH,
};

const CACHE_VERSION: u32 = 1;

#[derive(Serialize, Deserialize)]
struct AnalysisCache {
    cache_version: u32,
    source_size: u64,
    source_modified_nanos: u128,
    config: AnalysisConfig,
    analysis: AudioAnalysis,
}

#[must_use]
pub fn cache_path_for(audio_path: impl AsRef<Path>) -> PathBuf {
    let mut path = audio_path.as_ref().as_os_str().to_owned();
    path.push(".rustique-analysis.json");
    PathBuf::from(path)
}

/// Loads a fresh cache or analyzes and writes a replacement.
///
/// # Errors
///
/// Returns contextual source, decoding, analysis, or cache I/O errors.
pub fn analyze_cached(
    audio_path: impl AsRef<Path>,
    config: AnalysisConfig,
) -> Result<AudioAnalysis, AudioError> {
    let audio_path = audio_path.as_ref();
    let metadata = fs::metadata(audio_path).map_err(|source| AudioError::Metadata {
        path: audio_path.to_owned(),
        source,
    })?;
    let modified = metadata
        .modified()
        .map_err(|source| AudioError::Metadata {
            path: audio_path.to_owned(),
            source,
        })?
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let cache_path = cache_path_for(audio_path);
    if cache_path.exists() {
        let json = fs::read_to_string(&cache_path).map_err(|source| AudioError::ReadCache {
            path: cache_path.clone(),
            source,
        })?;
        let cache: AnalysisCache =
            serde_json::from_str(&json).map_err(|source| AudioError::ParseCache {
                path: cache_path.clone(),
                source,
            })?;
        if cache.cache_version == CACHE_VERSION
            && cache.source_size == metadata.len()
            && cache.source_modified_nanos == modified
            && cache.config == config
        {
            return Ok(cache.analysis);
        }
    }
    let analysis = analyze(&decode_audio(audio_path)?, config)?;
    let cache = AnalysisCache {
        cache_version: CACHE_VERSION,
        source_size: metadata.len(),
        source_modified_nanos: modified,
        config,
        analysis: analysis.clone(),
    };
    let json = serde_json::to_string(&cache).map_err(AudioError::SerializeCache)?;
    fs::write(&cache_path, json).map_err(|source| AudioError::WriteCache {
        path: cache_path,
        source,
    })?;
    Ok(analysis)
}
