//! Headless frame export and encoding support.

mod video;

pub use video::{
    CancellationToken, VideoCodec, VideoExportConfig, VideoProgress, VideoReport, export_video,
};

use audio_engine::AudioAnalysis;
use project_format::{
    EnvelopeSmoother, ModulatedParameters, ModulationTarget, ProjectV1, evaluate_mappings,
};
use render_core::{
    BenchmarkConfig, GpuContext, OffscreenError, OffscreenRenderTarget, ParticleRenderError,
    ParticleRenderer, RgbaColor,
};
use simulation::SimulationTiming;
use std::{
    fs,
    num::NonZeroU32,
    path::{Path, PathBuf},
};
use thiserror::Error;

#[derive(Clone, Debug)]
pub struct PngSequenceConfig {
    pub output_directory: PathBuf,
    pub start_frame: u32,
    pub end_frame: u32,
    pub width: u32,
    pub height: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SequenceReport {
    pub frames_rendered: u32,
}

#[derive(Debug, Error)]
pub enum ExportError {
    #[error("sequence end frame {end_frame} must be greater than start frame {start_frame}")]
    InvalidFrameRange { start_frame: u32, end_frame: u32 },
    #[error("failed to create sequence directory {path}: {source}")]
    CreateDirectory {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("project timing must use non-zero FPS and substeps")]
    InvalidTiming,
    #[error(transparent)]
    Offscreen(#[from] OffscreenError),
    #[error(transparent)]
    Render(#[from] ParticleRenderError),
    #[error("output already exists: {0}")]
    OutputExists(PathBuf),
    #[error("failed to start FFmpeg executable {executable}: {source}")]
    SpawnFfmpeg {
        executable: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("FFmpeg did not provide a writable stdin pipe")]
    MissingFfmpegStdin,
    #[error("failed to stream frame {frame} to FFmpeg (exit code {status:?}): {source}; {stderr}")]
    WriteFrame {
        frame: u32,
        status: Option<i32>,
        stderr: String,
        #[source]
        source: std::io::Error,
    },
    #[error("failed while waiting for FFmpeg: {0}")]
    WaitFfmpeg(#[source] std::io::Error),
    #[error("FFmpeg failed with exit code {status:?}: {stderr}")]
    FfmpegFailed { status: Option<i32>, stderr: String },
    #[error("failed to finalize video from {from} to {to}: {source}")]
    FinalizeOutput {
        from: PathBuf,
        to: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("video export was cancelled")]
    Cancelled,
}

/// Renders a deterministic, audio-reactive PNG sequence with constant memory use.
///
/// # Errors
///
/// Returns errors for invalid ranges/timing, directory creation, GPU setup,
/// simulation, readback, or PNG encoding.
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    clippy::cast_sign_loss
)]
pub fn render_png_sequence(
    context: &GpuContext,
    project: &ProjectV1,
    analysis: &AudioAnalysis,
    config: &PngSequenceConfig,
) -> Result<SequenceReport, ExportError> {
    if config.end_frame <= config.start_frame {
        return Err(ExportError::InvalidFrameRange {
            start_frame: config.start_frame,
            end_frame: config.end_frame,
        });
    }
    fs::create_dir_all(&config.output_directory).map_err(|source| {
        ExportError::CreateDirectory {
            path: config.output_directory.clone(),
            source,
        }
    })?;
    let fps = NonZeroU32::new(project.fps).ok_or(ExportError::InvalidTiming)?;
    let substeps =
        NonZeroU32::new(project.particle_system.substeps).ok_or(ExportError::InvalidTiming)?;
    let timing = SimulationTiming::new(fps, fps, substeps);
    let target = OffscreenRenderTarget::new(context, config.width, config.height)?;
    let mut renderer = ParticleRenderer::new(context, project.particle_system.count, project.seed)?;
    renderer.set_forces(context, &project.forces)?;
    let mut smoothers = vec![EnvelopeSmoother::default(); project.modulation_mappings.len()];
    let has_burst = project
        .modulation_mappings
        .iter()
        .any(|mapping| mapping.target == ModulationTarget::BurstEmission);
    let background = project.render_defaults.background;
    for frame in 0..config.end_frame {
        let time = f64::from(frame) / f64::from(project.fps);
        let active = evaluate_mappings(
            &project.modulation_mappings,
            &mut smoothers,
            analysis.sample_at(time),
            1.0 / project.fps as f32,
        );
        let mut parameters = ModulatedParameters {
            particle_size: project.render_defaults.particle_size_pixels,
            ..ModulatedParameters::default()
        };
        parameters.apply(&active);
        let render_config = BenchmarkConfig {
            particle_size_pixels: parameters.particle_size,
            position_scale: 1.0,
            force_scale: parameters.gravity_strength,
            brightness: parameters.brightness,
            active_particle_count: has_burst.then_some(parameters.burst_emission.max(0.0) as u32),
        };
        let clear = RgbaColor::new(background[0], background[1], background[2], background[3]);
        if frame < config.start_frame {
            let _pixels = renderer.render_timeline_frame(
                context,
                &target,
                frame,
                timing,
                render_config,
                clear,
            )?;
            continue;
        }
        let path = frame_path(&config.output_directory, frame);
        renderer.save_timeline_frame_png(
            context,
            &target,
            frame,
            timing,
            render_config,
            clear,
            path,
        )?;
    }
    Ok(SequenceReport {
        frames_rendered: config.end_frame - config.start_frame,
    })
}

#[must_use]
pub fn frame_path(directory: &Path, frame: u32) -> PathBuf {
    directory.join(format!("frame-{frame:06}.png"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frame_names_sort_in_timeline_order() {
        assert_eq!(
            frame_path(Path::new("frames"), 42),
            PathBuf::from("frames/frame-000042.png")
        );
    }
}
