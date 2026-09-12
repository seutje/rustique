use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
        mpsc,
    },
    thread,
};

use audio_engine::{AnalysisConfig, analyze_cached};
use exporter::{
    CancellationToken, PngSequenceConfig, VideoCodec, VideoExportConfig, export_video, frame_path,
    render_png_sequence,
};
use project_format::ProjectV1;
use render_core::{GpuConfig, GpuContext, PostProcessConfig, PostProcessQuality};
use serde::Serialize;

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PreviewJobStatus {
    pub id: u64,
    pub kind: String,
    pub state: String,
    pub progress: f32,
    pub output_path: Option<PathBuf>,
    pub error: Option<String>,
}

struct PreviewJob {
    id: u64,
    kind: PreviewKind,
    project: ProjectV1,
    audio_path: PathBuf,
    start_frame: u32,
    end_frame: u32,
    use_final_settings: bool,
}
enum PreviewKind {
    Still,
    Slice,
}

pub struct PreviewQueue {
    sender: mpsc::Sender<PreviewJob>,
    statuses: Arc<Mutex<Vec<PreviewJobStatus>>>,
    output_root: PathBuf,
    next_id: AtomicU64,
}

impl PreviewQueue {
    pub fn new() -> Result<Self, String> {
        let output_root =
            std::env::temp_dir().join(format!("rustique-previews-{}", std::process::id()));
        fs::create_dir_all(&output_root).map_err(|error| {
            format!(
                "failed to create preview directory {}: {error}",
                output_root.display()
            )
        })?;
        let (sender, receiver) = mpsc::channel();
        let statuses = Arc::new(Mutex::new(Vec::new()));
        let worker_statuses = Arc::clone(&statuses);
        let worker_root = output_root.clone();
        thread::Builder::new()
            .name("rustique-preview-queue".into())
            .spawn(move || run_queue(receiver, worker_statuses, worker_root))
            .map_err(|error| format!("failed to start preview queue: {error}"))?;
        Ok(Self {
            sender,
            statuses,
            output_root,
            next_id: AtomicU64::new(1),
        })
    }
    pub fn enqueue(
        &self,
        kind: &str,
        project: ProjectV1,
        audio_path: PathBuf,
        start_frame: u32,
        end_frame: u32,
        use_final_settings: bool,
    ) -> Result<u64, String> {
        project.validate().map_err(|error| error.to_string())?;
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let kind = match kind {
            "still" => PreviewKind::Still,
            "slice" => PreviewKind::Slice,
            _ => return Err(format!("unknown preview kind '{kind}'")),
        };
        if matches!(kind, PreviewKind::Slice) && end_frame <= start_frame {
            return Err("preview slice end must be after start".into());
        }
        self.statuses
            .lock()
            .map_err(|_| "preview queue lock is poisoned")?
            .push(PreviewJobStatus {
                id,
                kind: match kind {
                    PreviewKind::Still => "still",
                    PreviewKind::Slice => "slice",
                }
                .into(),
                state: "queued".into(),
                progress: 0.0,
                output_path: None,
                error: None,
            });
        self.sender
            .send(PreviewJob {
                id,
                kind,
                project,
                audio_path,
                start_frame,
                end_frame,
                use_final_settings,
            })
            .map_err(|_| "preview queue worker stopped".to_owned())?;
        Ok(id)
    }
    pub fn statuses(&self) -> Result<Vec<PreviewJobStatus>, String> {
        self.statuses
            .lock()
            .map(|value| value.clone())
            .map_err(|_| "preview queue lock is poisoned".into())
    }
    pub fn clear(&self) -> Result<(), String> {
        let mut statuses = self
            .statuses
            .lock()
            .map_err(|_| "preview queue lock is poisoned")?;
        if statuses
            .iter()
            .any(|job| job.state == "rendering" || job.state == "queued")
        {
            return Err("cannot clear previews while jobs are active".into());
        }
        if self.output_root.exists() {
            fs::remove_dir_all(&self.output_root).map_err(|error| {
                format!(
                    "failed to clear preview directory {}: {error}",
                    self.output_root.display()
                )
            })?;
            fs::create_dir_all(&self.output_root)
                .map_err(|error| format!("failed to recreate preview directory: {error}"))?;
        }
        statuses.clear();
        Ok(())
    }

    pub fn open(&self, path: &Path) -> Result<(), String> {
        let resolved = path
            .canonicalize()
            .map_err(|error| format!("failed to resolve preview {}: {error}", path.display()))?;
        let root = self.output_root.canonicalize().map_err(|error| {
            format!(
                "failed to resolve preview directory {}: {error}",
                self.output_root.display()
            )
        })?;
        if !resolved.starts_with(&root) {
            return Err("refusing to open a file outside the managed preview directory".into());
        }
        Command::new("explorer.exe")
            .arg(&resolved)
            .spawn()
            .map_err(|error| {
                format!(
                    "failed to open preview {} with its default application: {error}",
                    resolved.display()
                )
            })?;
        Ok(())
    }
}

fn run_queue(
    receiver: mpsc::Receiver<PreviewJob>,
    statuses: Arc<Mutex<Vec<PreviewJobStatus>>>,
    root: PathBuf,
) {
    let context = pollster::block_on(GpuContext::new(GpuConfig::default()))
        .map_err(|error| error.to_string());
    for job in receiver {
        update(&statuses, job.id, "rendering", 0.0, None, None);
        let result = context
            .as_ref()
            .map_err(Clone::clone)
            .and_then(|context| render_job(context, &job, &root, &statuses));
        match result {
            Ok(path) => update(&statuses, job.id, "complete", 1.0, Some(path), None),
            Err(error) => update(&statuses, job.id, "failed", 0.0, None, Some(error)),
        }
    }
}

#[allow(clippy::cast_precision_loss)]
fn render_job(
    context: &GpuContext,
    job: &PreviewJob,
    root: &Path,
    statuses: &Arc<Mutex<Vec<PreviewJobStatus>>>,
) -> Result<PathBuf, String> {
    let analysis = analyze_cached(&job.audio_path, AnalysisConfig::default())
        .map_err(|error| error.to_string())?;
    match job.kind {
        PreviewKind::Still => {
            let directory = root.join(format!("still-{}", job.id));
            let frame = job.start_frame;
            render_png_sequence(
                context,
                &job.project,
                &analysis,
                &PngSequenceConfig {
                    output_directory: directory.clone(),
                    start_frame: frame,
                    end_frame: frame.saturating_add(1),
                    width: 3840,
                    height: 2160,
                    post_process: PostProcessConfig::for_quality(PostProcessQuality::Final),
                },
            )
            .map_err(|error| error.to_string())?;
            Ok(frame_path(&directory, frame))
        }
        PreviewKind::Slice => {
            let output = root.join(format!("slice-{}.mp4", job.id));
            let (width, height, quality) = if job.use_final_settings {
                (
                    job.project.render_defaults.width,
                    job.project.render_defaults.height,
                    PostProcessQuality::Final,
                )
            } else {
                (1920, 1080, PostProcessQuality::Preview)
            };
            let total = job.end_frame - job.start_frame;
            export_video(
                context,
                &job.project,
                &analysis,
                &VideoExportConfig {
                    ffmpeg_path: PathBuf::from("ffmpeg"),
                    audio_path: job.audio_path.clone(),
                    output_path: output.clone(),
                    width,
                    height,
                    output_width: width,
                    output_height: height,
                    motion_blur_samples: 1,
                    start_frame: job.start_frame,
                    end_frame: job.end_frame,
                    codec: VideoCodec::H264,
                    post_process: PostProcessConfig::for_quality(quality),
                },
                &CancellationToken::default(),
                |progress| {
                    update(
                        statuses,
                        job.id,
                        "rendering",
                        progress.completed_frames as f32 / total as f32,
                        None,
                        None,
                    );
                },
            )
            .map_err(|error| error.to_string())?;
            Ok(output)
        }
    }
}

fn update(
    statuses: &Arc<Mutex<Vec<PreviewJobStatus>>>,
    id: u64,
    state: &str,
    progress: f32,
    output_path: Option<PathBuf>,
    error: Option<String>,
) {
    if let Ok(mut jobs) = statuses.lock() {
        if let Some(job) = jobs.iter_mut().find(|job| job.id == id) {
            job.state = state.into();
            job.progress = progress;
            if output_path.is_some() {
                job.output_path = output_path;
            }
            if error.is_some() {
                job.error = error;
            }
        }
    }
}
