//! Deterministic offline audio decoding, analysis, and caching.

mod analysis;
mod cache;
mod decode;

pub use analysis::{
    AnalysisConfig, AudioAnalysis, AudioFeatureFrame, FrequencyBands, WaveformBucket, analyze,
};
pub use cache::{analyze_cached, cache_path_for};
pub use decode::{DecodedAudio, decode_audio};

use std::path::PathBuf;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum AudioError {
    #[error("failed to open audio file {path}: {source}")]
    Open {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to probe audio file {path}: {message}")]
    Probe { path: PathBuf, message: String },
    #[error("audio file {path} has no decodable default track")]
    NoTrack { path: PathBuf },
    #[error("audio track in {path} does not declare a sample rate")]
    NoSampleRate { path: PathBuf },
    #[error("failed to create decoder for {path}: {message}")]
    Decoder { path: PathBuf, message: String },
    #[error("failed while decoding {path}: {message}")]
    Decode { path: PathBuf, message: String },
    #[error("analysis requires at least one decoded sample")]
    EmptyAudio,
    #[error("invalid analysis configuration: {0}")]
    InvalidConfig(String),
    #[error("failed to inspect audio source {path}: {source}")]
    Metadata {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to read analysis cache {path}: {source}")]
    ReadCache {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to parse analysis cache {path}: {source}")]
    ParseCache {
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },
    #[error("failed to serialize analysis cache: {0}")]
    SerializeCache(#[source] serde_json::Error),
    #[error("failed to write analysis cache {path}: {source}")]
    WriteCache {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn wav_bytes() -> Vec<u8> {
        let sample_rate = 8_000_u32;
        let samples: Vec<i16> = (0..sample_rate)
            .map(|index| if index % 800 < 400 { 12_000 } else { -12_000 })
            .collect();
        let data_size = u32::try_from(samples.len() * 2).unwrap();
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"RIFF");
        bytes.extend_from_slice(&(36 + data_size).to_le_bytes());
        bytes.extend_from_slice(b"WAVEfmt ");
        bytes.extend_from_slice(&16_u32.to_le_bytes());
        bytes.extend_from_slice(&1_u16.to_le_bytes());
        bytes.extend_from_slice(&1_u16.to_le_bytes());
        bytes.extend_from_slice(&sample_rate.to_le_bytes());
        bytes.extend_from_slice(&(sample_rate * 2).to_le_bytes());
        bytes.extend_from_slice(&2_u16.to_le_bytes());
        bytes.extend_from_slice(&16_u16.to_le_bytes());
        bytes.extend_from_slice(b"data");
        bytes.extend_from_slice(&data_size.to_le_bytes());
        for sample in samples {
            bytes.extend_from_slice(&sample.to_le_bytes());
        }
        bytes
    }

    #[test]
    fn wav_decodes_analyzes_and_reuses_cache() {
        let path =
            std::env::temp_dir().join(format!("rustique-audio-test-{}.wav", std::process::id()));
        let cache = cache_path_for(&path);
        fs::write(&path, wav_bytes()).unwrap();
        let decoded = decode_audio(&path).unwrap();
        assert_eq!(decoded.sample_rate, 8_000);
        assert_eq!(decoded.channels, 1);
        assert_eq!(decoded.samples.len(), 8_000);
        let first = analyze_cached(&path, AnalysisConfig::default()).unwrap();
        assert!(cache.exists());
        let second = analyze_cached(&path, AnalysisConfig::default()).unwrap();
        assert_eq!(first, second);
        fs::remove_file(cache).unwrap();
        fs::remove_file(path).unwrap();
    }
}
