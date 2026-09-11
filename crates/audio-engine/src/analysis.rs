use rustfft::{FftPlanner, num_complex::Complex};
use serde::{Deserialize, Serialize};

use crate::{AudioError, DecodedAudio};

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct AnalysisConfig {
    pub window_size: usize,
    pub hop_size: usize,
    pub waveform_bucket_size: usize,
}

impl Default for AnalysisConfig {
    fn default() -> Self {
        Self {
            window_size: 2048,
            hop_size: 512,
            waveform_bucket_size: 512,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct FrequencyBands {
    pub sub: f32,
    pub bass: f32,
    pub low_mids: f32,
    pub mids: f32,
    pub high_mids: f32,
    pub highs: f32,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct AudioFeatureFrame {
    pub time_seconds: f64,
    pub rms: f32,
    pub bands: FrequencyBands,
    pub spectral_centroid: f32,
    pub spectral_flux: f32,
    pub transient_strength: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct WaveformBucket {
    pub minimum: f32,
    pub maximum: f32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AudioAnalysis {
    pub sample_rate: u32,
    pub duration_seconds: f64,
    pub hop_size: usize,
    pub waveform: Vec<WaveformBucket>,
    pub frames: Vec<AudioFeatureFrame>,
}

impl AudioAnalysis {
    /// Samples the nearest precomputed feature window at a deterministic timestamp.
    #[must_use]
    pub fn sample_at(&self, time_seconds: f64) -> AudioFeatureFrame {
        self.frames
            .iter()
            .min_by(|left, right| {
                (left.time_seconds - time_seconds)
                    .abs()
                    .total_cmp(&(right.time_seconds - time_seconds).abs())
            })
            .copied()
            .unwrap_or_default()
    }
}

/// Computes normalized offline audio features.
///
/// # Errors
///
/// Returns an error for empty audio or invalid window configuration.
#[allow(clippy::too_many_lines, clippy::cast_precision_loss)]
pub fn analyze(audio: &DecodedAudio, config: AnalysisConfig) -> Result<AudioAnalysis, AudioError> {
    if config.window_size < 2 || config.hop_size == 0 || config.waveform_bucket_size == 0 {
        return Err(AudioError::InvalidConfig(
            "window size must be at least 2 and hop/bucket sizes must be non-zero".into(),
        ));
    }
    let mono = audio.mono_samples();
    if mono.is_empty() {
        return Err(AudioError::EmptyAudio);
    }
    let waveform = mono
        .chunks(config.waveform_bucket_size)
        .map(|bucket| {
            let (minimum, maximum) = bucket
                .iter()
                .fold((f32::INFINITY, f32::NEG_INFINITY), |(min, max), &sample| {
                    (min.min(sample), max.max(sample))
                });
            WaveformBucket { minimum, maximum }
        })
        .collect();
    let mut planner = FftPlanner::<f32>::new();
    let fft = planner.plan_fft_forward(config.window_size);
    let mut input = vec![Complex::default(); config.window_size];
    let mut previous_spectrum = vec![0.0; config.window_size / 2 + 1];
    let mut frames = Vec::with_capacity(mono.len().div_ceil(config.hop_size));
    let sample_rate = audio.sample_rate as f32;
    for start in (0..mono.len()).step_by(config.hop_size) {
        let mut square_sum = 0.0;
        for (offset, value) in input.iter_mut().enumerate() {
            let sample = mono.get(start + offset).copied().unwrap_or(0.0);
            square_sum += sample * sample;
            let phase = offset as f32 / (config.window_size - 1) as f32;
            *value = Complex::new(
                sample * (0.5 - 0.5 * (std::f32::consts::TAU * phase).cos()),
                0.0,
            );
        }
        fft.process(&mut input);
        let spectrum: Vec<f32> = input[..=config.window_size / 2]
            .iter()
            .map(|value| value.norm() / config.window_size as f32)
            .collect();
        let bands = FrequencyBands {
            sub: band_energy(&spectrum, sample_rate, config.window_size, 20.0, 60.0),
            bass: band_energy(&spectrum, sample_rate, config.window_size, 60.0, 250.0),
            low_mids: band_energy(&spectrum, sample_rate, config.window_size, 250.0, 500.0),
            mids: band_energy(&spectrum, sample_rate, config.window_size, 500.0, 2_000.0),
            high_mids: band_energy(&spectrum, sample_rate, config.window_size, 2_000.0, 6_000.0),
            highs: band_energy(
                &spectrum,
                sample_rate,
                config.window_size,
                6_000.0,
                sample_rate * 0.5,
            ),
        };
        let magnitude_sum = spectrum.iter().sum::<f32>();
        let weighted_sum = spectrum
            .iter()
            .enumerate()
            .map(|(bin, magnitude)| {
                bin as f32 * sample_rate / config.window_size as f32 * magnitude
            })
            .sum::<f32>();
        let centroid = if magnitude_sum > f32::EPSILON {
            weighted_sum / magnitude_sum / (sample_rate * 0.5)
        } else {
            0.0
        };
        let flux = spectrum
            .iter()
            .zip(&previous_spectrum)
            .map(|(current, previous)| (current - previous).max(0.0))
            .sum();
        previous_spectrum.copy_from_slice(&spectrum);
        let rms = (square_sum / config.window_size as f32).sqrt();
        let previous_rms = frames
            .last()
            .map_or(0.0, |frame: &AudioFeatureFrame| frame.rms);
        frames.push(AudioFeatureFrame {
            time_seconds: start as f64 / f64::from(audio.sample_rate),
            rms,
            bands,
            spectral_centroid: centroid.clamp(0.0, 1.0),
            spectral_flux: flux,
            transient_strength: flux + (rms - previous_rms).max(0.0),
        });
    }
    normalize(&mut frames);
    Ok(AudioAnalysis {
        sample_rate: audio.sample_rate,
        duration_seconds: mono.len() as f64 / f64::from(audio.sample_rate),
        hop_size: config.hop_size,
        waveform,
        frames,
    })
}

#[allow(clippy::cast_precision_loss)]
fn band_energy(spectrum: &[f32], sample_rate: f32, window_size: usize, low: f32, high: f32) -> f32 {
    let bin_hz = sample_rate / window_size as f32;
    let mut sum = 0.0;
    let mut count = 0_u32;
    for (bin, &magnitude) in spectrum.iter().enumerate() {
        let frequency = bin as f32 * bin_hz;
        if frequency >= low && frequency < high {
            sum += magnitude;
            count += 1;
        }
    }
    if count == 0 { 0.0 } else { sum / count as f32 }
}

fn normalize(frames: &mut [AudioFeatureFrame]) {
    let maxima = frames.iter().fold([0.0_f32; 9], |mut values, frame| {
        let current = [
            frame.rms,
            frame.bands.sub,
            frame.bands.bass,
            frame.bands.low_mids,
            frame.bands.mids,
            frame.bands.high_mids,
            frame.bands.highs,
            frame.spectral_flux,
            frame.transient_strength,
        ];
        for (maximum, value) in values.iter_mut().zip(current) {
            *maximum = maximum.max(value);
        }
        values
    });
    for frame in frames {
        frame.rms = normalized(frame.rms, maxima[0]);
        frame.bands.sub = normalized(frame.bands.sub, maxima[1]);
        frame.bands.bass = normalized(frame.bands.bass, maxima[2]);
        frame.bands.low_mids = normalized(frame.bands.low_mids, maxima[3]);
        frame.bands.mids = normalized(frame.bands.mids, maxima[4]);
        frame.bands.high_mids = normalized(frame.bands.high_mids, maxima[5]);
        frame.bands.highs = normalized(frame.bands.highs, maxima[6]);
        frame.spectral_flux = normalized(frame.spectral_flux, maxima[7]);
        frame.transient_strength = normalized(frame.transient_strength, maxima[8]);
    }
}

fn normalized(value: f32, maximum: f32) -> f32 {
    if maximum > f32::EPSILON {
        (value / maximum).clamp(0.0, 1.0)
    } else {
        0.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[allow(clippy::cast_precision_loss)]
    fn sine_wave_is_analyzed_and_normalized() {
        let sample_rate = 48_000;
        let samples = (0..sample_rate)
            .map(|index| {
                (std::f32::consts::TAU * 100.0 * index as f32 / sample_rate as f32).sin() * 0.5
            })
            .collect();
        let analysis = analyze(
            &DecodedAudio {
                sample_rate,
                channels: 1,
                samples,
            },
            AnalysisConfig::default(),
        )
        .unwrap();
        assert!(!analysis.waveform.is_empty());
        assert!(!analysis.frames.is_empty());
        assert!(
            analysis
                .frames
                .iter()
                .all(|frame| (0.0..=1.0).contains(&frame.rms))
        );
        assert!(
            analysis
                .frames
                .iter()
                .any(|frame| frame.bands.bass > frame.bands.highs)
        );
    }
}
