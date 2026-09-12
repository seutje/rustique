use audio_engine::{AnalysisConfig, analyze_cached};
use exporter::{CancellationToken, VideoCodec, VideoExportConfig, export_video};
use project_format::ProjectV1;
use render_core::{GpuConfig, GpuContext, PostProcessConfig, PostProcessQuality};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::PathBuf,
    process::Command,
    sync::{Arc, Mutex, mpsc},
    thread,
    time::Instant,
};

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProductionSettings {
    pub output_path: PathBuf,
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    pub supersampling: f32,
    pub motion_blur_samples: u32,
    pub substeps: u32,
    pub codec: String,
}

#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProductionStatus {
    pub state: String,
    pub completed_frames: u32,
    pub total_frames: u32,
    pub eta_seconds: Option<f64>,
    pub output_path: Option<PathBuf>,
    pub manifest_path: Option<PathBuf>,
    pub error: Option<String>,
}

struct Job {
    project: ProjectV1,
    audio_path: PathBuf,
    settings: ProductionSettings,
}
pub struct ProductionQueue {
    sender: mpsc::Sender<Job>,
    status: Arc<Mutex<ProductionStatus>>,
}

impl ProductionQueue {
    pub fn new() -> Result<Self, String> {
        let (sender, receiver) = mpsc::channel();
        let status = Arc::new(Mutex::new(ProductionStatus {
            state: "idle".into(),
            ..ProductionStatus::default()
        }));
        let worker_status = Arc::clone(&status);
        thread::Builder::new()
            .name("rustique-production-render".into())
            .spawn(move || run(receiver, worker_status))
            .map_err(|error| format!("failed to start production render queue: {error}"))?;
        Ok(Self { sender, status })
    }
    pub fn enqueue(
        &self,
        project: ProjectV1,
        audio_path: PathBuf,
        mut settings: ProductionSettings,
    ) -> Result<(), String> {
        validate(&project, &audio_path, &mut settings)?;
        let mut status = self
            .status
            .lock()
            .map_err(|_| "production status lock is poisoned")?;
        if status.state == "queued" || status.state == "rendering" {
            return Err("a production render is already active".into());
        }
        *status = ProductionStatus {
            state: "queued".into(),
            output_path: Some(settings.output_path.clone()),
            ..ProductionStatus::default()
        };
        drop(status);
        self.sender
            .send(Job {
                project,
                audio_path,
                settings,
            })
            .map_err(|_| "production render worker stopped".into())
    }
    pub fn status(&self) -> Result<ProductionStatus, String> {
        self.status
            .lock()
            .map(|value| value.clone())
            .map_err(|_| "production status lock is poisoned".into())
    }
}

fn validate(
    project: &ProjectV1,
    audio_path: &std::path::Path,
    settings: &mut ProductionSettings,
) -> Result<(), String> {
    project.validate().map_err(|error| error.to_string())?;
    if settings.width == 0
        || settings.height == 0
        || !matches!(settings.fps, 30 | 60)
        || !settings.supersampling.is_finite()
        || !(1.0..=2.0).contains(&settings.supersampling)
        || settings.motion_blur_samples == 0
        || settings.motion_blur_samples > 16
        || settings.substeps == 0
    {
        return Err("production settings require non-zero dimensions/substeps, 30 or 60 FPS, 1-2x supersampling, and 1-16 motion-blur samples".into());
    }
    if !settings
        .output_path
        .extension()
        .and_then(|value| value.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("mp4"))
    {
        return Err("production output must use the .mp4 extension".into());
    }
    if settings
        .output_path
        .file_stem()
        .is_none_or(std::ffi::OsStr::is_empty)
    {
        return Err("production output must include a filename".into());
    }
    let parent = settings
        .output_path
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or_else(|| std::path::Path::new("."));
    let resolved_parent = parent.canonicalize().map_err(|error| {
        format!(
            "production output directory {} does not exist or cannot be accessed: {error}",
            parent.display()
        )
    })?;
    if !resolved_parent.is_dir() {
        return Err(format!(
            "production output parent {} is not a directory",
            resolved_parent.display()
        ));
    }
    let filename = settings
        .output_path
        .file_name()
        .ok_or("production output must include a filename")?;
    settings.output_path = resolved_parent.join(filename);
    if !audio_path.is_file() {
        return Err(format!(
            "production audio source {} is not a readable file",
            audio_path.display()
        ));
    }
    let mut collisions = vec![
        settings.output_path.clone(),
        settings.output_path.with_extension("project.json"),
        settings.output_path.with_extension("render.json"),
    ];
    let original_extension = settings
        .output_path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("mp4");
    collisions.push(
        settings
            .output_path
            .with_extension(format!("partial.{original_extension}")),
    );
    if let Some(existing) = collisions.into_iter().find(|path| path.exists()) {
        return Err(format!(
            "production output artifact already exists: {}",
            existing.display()
        ));
    }
    Ok(())
}

fn run(receiver: mpsc::Receiver<Job>, status: Arc<Mutex<ProductionStatus>>) {
    let context = pollster::block_on(GpuContext::new(GpuConfig::default()))
        .map_err(|error| error.to_string());
    for job in receiver {
        let result = context
            .as_ref()
            .map_err(Clone::clone)
            .and_then(|context| render(context, &job, &status));
        if let Err(error) = result {
            set_failed(&status, error);
        }
    }
}

#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    clippy::cast_sign_loss
)]
fn render(
    context: &GpuContext,
    job: &Job,
    status: &Arc<Mutex<ProductionStatus>>,
) -> Result<(), String> {
    let mut project = job.project.clone();
    project.fps = job.settings.fps;
    project.particle_system.substeps = job.settings.substeps;
    project.validate().map_err(|error| error.to_string())?;
    let analysis = analyze_cached(&job.audio_path, AnalysisConfig::default())
        .map_err(|error| error.to_string())?;
    let seconds = f64::from(project.duration_seconds).min(analysis.duration_seconds);
    let total = (seconds * f64::from(project.fps)).floor().max(1.0) as u32;
    let render_width = (job.settings.width as f32 * job.settings.supersampling).round() as u32;
    let render_height = (job.settings.height as f32 * job.settings.supersampling).round() as u32;
    let codec = match job.settings.codec.as_str() {
        "h264" => VideoCodec::H264,
        "hevc" => VideoCodec::Hevc,
        _ => return Err(format!("unknown production codec '{}'", job.settings.codec)),
    };
    if let Ok(mut current) = status.lock() {
        current.state = "rendering".into();
        current.total_frames = total;
    }
    let started = Instant::now();
    export_video(
        context,
        &project,
        &analysis,
        &VideoExportConfig {
            ffmpeg_path: PathBuf::from("ffmpeg"),
            audio_path: job.audio_path.clone(),
            output_path: job.settings.output_path.clone(),
            width: render_width,
            height: render_height,
            output_width: job.settings.width,
            output_height: job.settings.height,
            motion_blur_samples: job.settings.motion_blur_samples,
            start_frame: 0,
            end_frame: total,
            codec,
            post_process: PostProcessConfig::for_quality(PostProcessQuality::Final),
        },
        &CancellationToken::default(),
        |progress| {
            let elapsed = started.elapsed().as_secs_f64();
            let rate = f64::from(progress.completed_frames) / elapsed.max(0.001);
            let eta = (f64::from(progress.total_frames - progress.completed_frames)
                / rate.max(0.001))
            .max(0.0);
            if let Ok(mut current) = status.lock() {
                current.completed_frames = progress.completed_frames;
                current.total_frames = progress.total_frames;
                current.eta_seconds = Some(eta);
            }
        },
    )
    .map_err(|error| error.to_string())?;
    validate_audio_mux(&job.settings.output_path)?;
    let project_path = write_project_snapshot(&project, &job.settings.output_path)?;
    let manifest_path = write_manifest(
        context,
        &project,
        &job.settings,
        &job.audio_path,
        &project_path,
        &job.settings.output_path,
    )?;
    if let Ok(mut current) = status.lock() {
        current.state = "complete".into();
        current.completed_frames = total;
        current.eta_seconds = Some(0.0);
        current.manifest_path = Some(manifest_path);
    }
    Ok(())
}

fn validate_audio_mux(path: &std::path::Path) -> Result<(), String> {
    let output = Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-select_streams",
            "a:0",
            "-show_entries",
            "stream=codec_type",
            "-of",
            "default=noprint_wrappers=1:nokey=1",
        ])
        .arg(path)
        .output()
        .map_err(|error| format!("failed to run ffprobe for final audio validation: {error}"))?;
    if !output.status.success() || String::from_utf8_lossy(&output.stdout).trim() != "audio" {
        return Err(format!(
            "final audio mux validation failed for {}: {}",
            path.display(),
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(())
}

#[derive(Serialize)]
struct Manifest<'a> {
    project_version: u32,
    engine_version: &'a str,
    seed: u64,
    gpu: String,
    project_snapshot: &'a std::path::Path,
    audio_source: &'a std::path::Path,
    render_settings: &'a ProductionSettings,
}
fn write_project_snapshot(
    project: &ProjectV1,
    output: &std::path::Path,
) -> Result<PathBuf, String> {
    let path = output.with_extension("project.json");
    let mut snapshot = project.clone();
    snapshot.visual_preset = None;
    snapshot.reaction_profile = None;
    snapshot
        .save(&path)
        .map_err(|error| format!("failed to save render project snapshot: {error}"))?;
    Ok(path)
}
fn write_manifest(
    context: &GpuContext,
    project: &ProjectV1,
    settings: &ProductionSettings,
    audio_source: &std::path::Path,
    project_snapshot: &std::path::Path,
    output: &std::path::Path,
) -> Result<PathBuf, String> {
    let path = output.with_extension("render.json");
    let manifest = Manifest {
        project_version: project.project_version,
        engine_version: &project.engine_version,
        seed: project.seed,
        gpu: context.info().adapter.name,
        project_snapshot,
        audio_source,
        render_settings: settings,
    };
    let json = serde_json::to_string_pretty(&manifest)
        .map_err(|error| format!("failed to serialize render manifest: {error}"))?;
    fs::write(&path, format!("{json}\n")).map_err(|error| {
        format!(
            "failed to write render manifest {}: {error}",
            path.display()
        )
    })?;
    Ok(path)
}
fn set_failed(status: &Arc<Mutex<ProductionStatus>>, error: String) {
    if let Ok(mut current) = status.lock() {
        current.state = "failed".into();
        current.error = Some(error);
        current.eta_seconds = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn export_path_is_resolved_and_collisions_are_rejected() {
        let project = ProjectV1::load(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../../examples/star-orbit.rustique.json"),
        )
        .unwrap();
        let directory = std::env::temp_dir().join(format!(
            "rustique-production-path-test-{}",
            std::process::id()
        ));
        fs::create_dir_all(&directory).unwrap();
        let audio = directory.join("audio.wav");
        fs::write(&audio, []).unwrap();
        let mut settings = ProductionSettings {
            output_path: directory.join("render.MP4"),
            width: 3840,
            height: 2160,
            fps: 60,
            supersampling: 1.0,
            motion_blur_samples: 1,
            substeps: 2,
            codec: "h264".into(),
        };
        validate(&project, &audio, &mut settings).unwrap();
        assert!(settings.output_path.is_absolute());
        fs::write(&settings.output_path, []).unwrap();
        assert!(validate(&project, &audio, &mut settings).is_err());
        fs::remove_dir_all(directory).unwrap();
    }
}
