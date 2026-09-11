use std::{fs::File, path::Path};

use symphonia::core::{
    audio::SampleBuffer,
    codecs::DecoderOptions,
    errors::Error as SymphoniaError,
    formats::FormatOptions,
    io::{MediaSourceStream, MediaSourceStreamOptions},
    meta::MetadataOptions,
    probe::Hint,
};

use crate::AudioError;

#[derive(Clone, Debug)]
pub struct DecodedAudio {
    pub sample_rate: u32,
    pub channels: usize,
    /// Interleaved, normalized `f32` samples.
    pub samples: Vec<f32>,
}

impl DecodedAudio {
    #[must_use]
    #[allow(clippy::cast_precision_loss)]
    pub fn mono_samples(&self) -> Vec<f32> {
        if self.channels == 1 {
            return self.samples.clone();
        }
        self.samples
            .chunks_exact(self.channels)
            .map(|frame| frame.iter().sum::<f32>() / self.channels as f32)
            .collect()
    }
}

/// Decodes a supported audio file completely for deterministic offline analysis.
///
/// # Errors
///
/// Returns contextual file, probe, codec, or packet decoding errors.
#[allow(clippy::too_many_lines)]
pub fn decode_audio(path: impl AsRef<Path>) -> Result<DecodedAudio, AudioError> {
    let path = path.as_ref();
    let file = File::open(path).map_err(|source| AudioError::Open {
        path: path.to_owned(),
        source,
    })?;
    let stream = MediaSourceStream::new(Box::new(file), MediaSourceStreamOptions::default());
    let mut hint = Hint::new();
    if let Some(extension) = path.extension().and_then(|value| value.to_str()) {
        hint.with_extension(extension);
    }
    let probed = symphonia::default::get_probe()
        .format(
            &hint,
            stream,
            &FormatOptions::default(),
            &MetadataOptions::default(),
        )
        .map_err(|error| AudioError::Probe {
            path: path.to_owned(),
            message: error.to_string(),
        })?;
    let mut format = probed.format;
    let track = format.default_track().ok_or_else(|| AudioError::NoTrack {
        path: path.to_owned(),
    })?;
    let track_id = track.id;
    let sample_rate = track
        .codec_params
        .sample_rate
        .ok_or_else(|| AudioError::NoSampleRate {
            path: path.to_owned(),
        })?;
    let mut codec_decoder = symphonia::default::get_codecs()
        .make(&track.codec_params, &DecoderOptions::default())
        .map_err(|error| AudioError::Decoder {
            path: path.to_owned(),
            message: error.to_string(),
        })?;
    let mut samples = Vec::new();
    let mut channels = None;
    loop {
        let packet = match format.next_packet() {
            Ok(packet) => packet,
            Err(SymphoniaError::IoError(error))
                if error.kind() == std::io::ErrorKind::UnexpectedEof =>
            {
                break;
            }
            Err(error) => {
                return Err(AudioError::Decode {
                    path: path.to_owned(),
                    message: error.to_string(),
                });
            }
        };
        if packet.track_id() != track_id {
            continue;
        }
        let audio_buffer = match codec_decoder.decode(&packet) {
            Ok(audio_buffer) => audio_buffer,
            Err(SymphoniaError::DecodeError(_)) => continue,
            Err(error) => {
                return Err(AudioError::Decode {
                    path: path.to_owned(),
                    message: error.to_string(),
                });
            }
        };
        let channel_count = audio_buffer.spec().channels.count();
        channels.get_or_insert(channel_count);
        let mut converted =
            SampleBuffer::<f32>::new(audio_buffer.capacity() as u64, *audio_buffer.spec());
        converted.copy_interleaved_ref(audio_buffer);
        samples.extend_from_slice(converted.samples());
    }
    Ok(DecodedAudio {
        sample_rate,
        channels: channels.unwrap_or(1),
        samples,
    })
}
