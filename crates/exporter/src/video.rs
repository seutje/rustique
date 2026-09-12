use std::{
    ffi::OsString,
    fs,
    io::Write,
    num::NonZeroU32,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

use audio_engine::AudioAnalysis;
use project_format::{
    EnvelopeSmoother, ModulatedParameters, ModulationTarget, ProjectV1, evaluate_automation,
    evaluate_mappings,
};
use render_core::{
    BenchmarkConfig, GpuContext, OffscreenRenderTarget, ParticleRenderer, PostProcessConfig,
    RgbaColor,
};
use simulation::SimulationTiming;

use crate::{ExportError, camera_matrix};

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum VideoCodec {
    #[default]
    H264,
    Hevc,
    ProRes422Hq,
    ProRes4444,
}

#[derive(Clone, Debug)]
pub struct VideoExportConfig {
    pub ffmpeg_path: PathBuf,
    pub audio_path: PathBuf,
    pub output_path: PathBuf,
    pub width: u32,
    pub height: u32,
    pub start_frame: u32,
    pub end_frame: u32,
    pub codec: VideoCodec,
    pub post_process: PostProcessConfig,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct VideoProgress {
    pub completed_frames: u32,
    pub total_frames: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct VideoReport {
    pub frames_rendered: u32,
}

#[derive(Clone, Debug, Default)]
pub struct CancellationToken(Arc<AtomicBool>);

impl CancellationToken {
    pub fn cancel(&self) {
        self.0.store(true, Ordering::Release);
    }

    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Acquire)
    }
}

/// Renders one RGBA frame at a time and streams it directly to `FFmpeg`.
///
/// The final path is only created after `FFmpeg` exits successfully. Failures and
/// cancellation remove the adjacent partial file.
///
/// # Errors
///
/// Returns an error when configuration, GPU rendering, frame streaming,
/// encoding, cancellation, or output finalization fails.
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    clippy::cast_sign_loss,
    clippy::too_many_lines
)]
pub fn export_video(
    context: &GpuContext,
    project: &ProjectV1,
    analysis: &AudioAnalysis,
    config: &VideoExportConfig,
    cancellation: &CancellationToken,
    mut progress: impl FnMut(VideoProgress),
) -> Result<VideoReport, ExportError> {
    if config.end_frame <= config.start_frame {
        return Err(ExportError::InvalidFrameRange {
            start_frame: config.start_frame,
            end_frame: config.end_frame,
        });
    }
    if config.output_path.exists() {
        return Err(ExportError::OutputExists(config.output_path.clone()));
    }
    let partial_path = partial_path(&config.output_path);
    if partial_path.exists() {
        fs::remove_file(&partial_path).map_err(|source| ExportError::FinalizeOutput {
            from: partial_path.clone(),
            to: config.output_path.clone(),
            source,
        })?;
    }

    let fps = NonZeroU32::new(project.fps).ok_or(ExportError::InvalidTiming)?;
    let substeps =
        NonZeroU32::new(project.particle_system.substeps).ok_or(ExportError::InvalidTiming)?;
    let timing = SimulationTiming::new(fps, fps, substeps);
    let target = OffscreenRenderTarget::new_with_post_process(
        context,
        config.width,
        config.height,
        config.post_process,
    )?;
    let mut renderer = ParticleRenderer::new(context, project.particle_system.count, project.seed)?;
    renderer.set_forces(context, &project.forces)?;

    let mut command = ffmpeg_command(project, config, &partial_path);
    let mut child = command.spawn().map_err(|source| ExportError::SpawnFfmpeg {
        executable: config.ffmpeg_path.clone(),
        source,
    })?;
    let Some(mut stdin) = child.stdin.take() else {
        let _ = child.kill();
        let _ = child.wait();
        let _ = fs::remove_file(&partial_path);
        return Err(ExportError::MissingFfmpegStdin);
    };
    let total_frames = config.end_frame - config.start_frame;
    let mut smoothers = vec![EnvelopeSmoother::default(); project.modulation_mappings.len()];
    let has_burst = project
        .modulation_mappings
        .iter()
        .any(|mapping| mapping.target == ModulationTarget::BurstEmission);
    let background = project.render_defaults.background;

    for frame in 0..config.end_frame {
        if cancellation.is_cancelled() {
            drop(stdin);
            let _ = child.kill();
            let _ = child.wait();
            let _ = fs::remove_file(&partial_path);
            return Err(ExportError::Cancelled);
        }
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
        let pixels = match renderer.render_timeline_frame(
            context,
            &target,
            frame,
            timing,
            BenchmarkConfig {
                particle_size_pixels: parameters.particle_size,
                position_scale: 1.0,
                force_scale: parameters.gravity_strength,
                brightness: parameters.brightness,
                active_particle_count: has_burst
                    .then_some(parameters.burst_emission.max(0.0) as u32),
                view_projection: Some(camera_matrix(
                    project,
                    &parameters,
                    time as f32,
                    config.width,
                    config.height,
                )),
            },
            RgbaColor::new(background[0], background[1], background[2], background[3]),
        ) {
            Ok(pixels) => pixels,
            Err(error) => {
                drop(stdin);
                let _ = child.kill();
                let _ = child.wait();
                let _ = fs::remove_file(&partial_path);
                return Err(error.into());
            }
        };
        if frame < config.start_frame {
            continue;
        }
        if let Err(source) = stdin.write_all(&pixels) {
            drop(stdin);
            let process_output = child.wait_with_output();
            let _ = fs::remove_file(&partial_path);
            let (status, stderr) = process_output.map_or_else(
                |wait_error| {
                    (
                        None,
                        format!("failed to collect FFmpeg error: {wait_error}"),
                    )
                },
                |output| {
                    (
                        output.status.code(),
                        String::from_utf8_lossy(&output.stderr).trim().to_owned(),
                    )
                },
            );
            return Err(ExportError::WriteFrame {
                frame,
                status,
                stderr,
                source,
            });
        }
        progress(VideoProgress {
            completed_frames: frame - config.start_frame + 1,
            total_frames,
        });
    }

    drop(stdin);
    let output = child.wait_with_output().map_err(|error| {
        let _ = fs::remove_file(&partial_path);
        ExportError::WaitFfmpeg(error)
    })?;
    if !output.status.success() {
        let _ = fs::remove_file(&partial_path);
        return Err(ExportError::FfmpegFailed {
            status: output.status.code(),
            stderr: String::from_utf8_lossy(&output.stderr).trim().to_owned(),
        });
    }
    fs::rename(&partial_path, &config.output_path).map_err(|source| {
        ExportError::FinalizeOutput {
            from: partial_path,
            to: config.output_path.clone(),
            source,
        }
    })?;
    Ok(VideoReport {
        frames_rendered: total_frames,
    })
}

fn ffmpeg_command(project: &ProjectV1, config: &VideoExportConfig, output: &Path) -> Command {
    let mut command = Command::new(&config.ffmpeg_path);
    command
        .arg("-hide_banner")
        .arg("-loglevel")
        .arg("error")
        .arg("-f")
        .arg("rawvideo")
        .arg("-pixel_format")
        .arg("rgba")
        .arg("-video_size")
        .arg(format!("{}x{}", config.width, config.height))
        .arg("-framerate")
        .arg(project.fps.to_string())
        .arg("-i")
        .arg("pipe:0")
        // Input-side seek applies only to the following audio input.
        .arg("-ss")
        .arg(format!(
            "{:.9}",
            f64::from(config.start_frame) / f64::from(project.fps)
        ))
        .arg("-i")
        .arg(&config.audio_path)
        .arg("-map")
        .arg("0:v:0")
        .arg("-map")
        .arg("1:a:0")
        .args(codec_arguments(config.codec))
        .arg("-c:a")
        .arg("aac")
        .arg("-shortest")
        .arg(output)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped());
    command
}

fn codec_arguments(codec: VideoCodec) -> &'static [&'static str] {
    match codec {
        VideoCodec::H264 => &["-c:v", "libx264", "-pix_fmt", "yuv420p", "-crf", "18"],
        VideoCodec::Hevc => &["-c:v", "libx265", "-pix_fmt", "yuv420p", "-crf", "20"],
        VideoCodec::ProRes422Hq => &[
            "-c:v",
            "prores_ks",
            "-profile:v",
            "3",
            "-pix_fmt",
            "yuv422p10le",
        ],
        VideoCodec::ProRes4444 => &[
            "-c:v",
            "prores_ks",
            "-profile:v",
            "4",
            "-pix_fmt",
            "yuva444p10le",
        ],
    }
}

fn partial_path(output: &Path) -> PathBuf {
    let mut extension = OsString::from("partial");
    if let Some(original) = output.extension() {
        extension.push(".");
        extension.push(original);
    }
    output.with_extension(extension)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn partial_file_preserves_container_extension() {
        assert_eq!(
            partial_path(Path::new("render.mp4")),
            PathBuf::from("render.partial.mp4")
        );
    }

    #[test]
    fn alpha_codec_uses_prores_4444_pixel_format() {
        let arguments = codec_arguments(VideoCodec::ProRes4444);
        assert!(arguments.contains(&"yuva444p10le"));
        assert!(arguments.contains(&"4"));
    }
}
