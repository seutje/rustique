//! Headless frame export and encoding support.

mod video;

pub use video::{
    CancellationToken, VideoCodec, VideoExportConfig, VideoProgress, VideoReport, export_video,
};

use audio_engine::AudioAnalysis;
use project_format::RenderModeV1;
use project_format::{
    EnvelopeSmoother, ModulatedParameters, ModulationTarget, ProjectV1, evaluate_automation,
    evaluate_mappings,
};
use render_core::{
    BenchmarkConfig, GpuContext, OffscreenError, OffscreenRenderTarget, ParticleRenderError,
    ParticleRenderer, PerspectiveCamera, RgbaColor, VolumetricConfig, VolumetricQuality,
    VolumetricRenderer,
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
    pub post_process: render_core::PostProcessConfig,
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
    #[error("renderer was not initialized for the selected project render mode")]
    MissingRenderer,
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
    clippy::cast_sign_loss,
    clippy::too_many_lines
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
    let mut post_process = config.post_process;
    if project.render_mode == RenderModeV1::Volumetric {
        post_process.trails = true;
        post_process.trail_decay = if config.post_process.trails {
            0.82
        } else {
            0.65
        };
    }
    let target = OffscreenRenderTarget::new_with_post_process(
        context,
        config.width,
        config.height,
        post_process,
    )?;
    let mut renderer = (project.render_mode == RenderModeV1::Particles)
        .then(|| ParticleRenderer::new(context, project.particle_system.count, project.seed))
        .transpose()?;
    if let Some(renderer) = &mut renderer {
        renderer.set_forces(context, &project.forces)?;
    }
    let volume_quality = if config.post_process.trails {
        VolumetricQuality::Final
    } else if config.post_process.bloom {
        VolumetricQuality::Preview
    } else {
        VolumetricQuality::Draft
    };
    let volume = (project.render_mode == RenderModeV1::Volumetric).then(|| {
        VolumetricRenderer::new(
            context,
            project.particle_system.count,
            project.seed,
            VolumetricConfig::for_quality(volume_quality),
        )
    });
    let mut smoothers = vec![EnvelopeSmoother::default(); project.modulation_mappings.len()];
    let has_burst = project
        .modulation_mappings
        .iter()
        .any(|mapping| mapping.target == ModulationTarget::BurstEmission);
    let background = project.render_defaults.background;
    for frame in 0..config.end_frame {
        let time = f64::from(frame) / f64::from(project.fps);
        let mut active = evaluate_mappings(
            &project.modulation_mappings,
            &mut smoothers,
            project.apply_analysis_profile(analysis.sample_at(time)),
            1.0 / project.fps as f32,
        );
        active.extend(evaluate_automation(&project.automation_tracks, time as f32));
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
            view_projection: Some(camera_matrix(
                project,
                &parameters,
                time as f32,
                config.width,
                config.height,
            )),
        };
        let clear = RgbaColor::new(background[0], background[1], background[2], background[3]);
        if frame < config.start_frame {
            if let Some(volume) = &volume {
                let _pixels = volume.render_frame(context, &target, frame, project.fps, clear)?;
            } else if let Some(renderer) = &mut renderer {
                let _pixels = renderer.render_timeline_frame(
                    context,
                    &target,
                    frame,
                    timing,
                    render_config,
                    clear,
                )?;
            }
            continue;
        }
        let path = frame_path(&config.output_directory, frame);
        if let Some(volume) = &volume {
            volume.save_frame_png(context, &target, frame, project.fps, clear, path)?;
        } else if let Some(renderer) = &mut renderer {
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
    }
    Ok(SequenceReport {
        frames_rendered: config.end_frame - config.start_frame,
    })
}

#[allow(clippy::cast_precision_loss)]
pub(crate) fn camera_matrix(
    project: &ProjectV1,
    parameters: &ModulatedParameters,
    time_seconds: f32,
    width: u32,
    height: u32,
) -> [[f32; 4]; 4] {
    let camera = project.camera.sample(
        time_seconds,
        parameters.camera_fov,
        parameters.camera_shake,
        project.seed,
    );
    PerspectiveCamera {
        position: camera.position,
        target: camera.target,
        up: camera.up,
        vertical_fov_degrees: camera.vertical_fov_degrees,
        near_plane: camera.near_plane,
        far_plane: camera.far_plane,
    }
    .view_projection(width as f32 / height as f32)
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
