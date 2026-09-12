#![allow(clippy::needless_pass_by_value)] // Tauri commands require owned State extractors.

use std::path::PathBuf;

use audio_engine::{AnalysisConfig, analyze_cached};
use project_format::{MacroParameterV1, ProjectV1, VisualPresetV1};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use render_core::{GpuConfig, GpuContext};
use serde::Serialize;
use tauri::{Manager, State};

mod preview;
use preview::{PreviewJobStatus, PreviewQueue};
mod production;
use production::{ProductionQueue, ProductionSettings, ProductionStatus};

#[cfg(target_os = "windows")]
mod viewport;
#[cfg(target_os = "windows")]
use viewport::{PreviewQuality, PreviewStats, ViewportController};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct GpuSummary {
    adapter: String,
    backend: String,
    device_type: String,
    driver: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct EditorProject {
    path: PathBuf,
    project: ProjectV1,
    parameters: Vec<ParameterSchema>,
    macros: Vec<MacroParameterV1>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ParameterSchema {
    path: &'static str,
    label: &'static str,
    kind: &'static str,
    minimum: Option<f64>,
    maximum: Option<f64>,
    step: Option<f64>,
    modulation_target: Option<&'static str>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct TimelineAudio {
    duration_seconds: f64,
    waveform: Vec<[f32; 2]>,
    transient_times: Vec<f64>,
    beat_times: Vec<f64>,
}

#[tauri::command]
async fn gpu_info() -> Result<GpuSummary, String> {
    let context = GpuContext::new(GpuConfig::default())
        .await
        .map_err(|error| format!("failed to query GPU: {error}"))?;
    let info = context.info();
    Ok(GpuSummary {
        adapter: info.adapter.name,
        backend: format!("{:?}", info.adapter.backend),
        device_type: format!("{:?}", info.adapter.device_type),
        driver: info.adapter.driver,
    })
}

#[tauri::command]
async fn load_project(
    path: PathBuf,
    viewport: State<'_, ViewportController>,
) -> Result<EditorProject, String> {
    let load_path = resolve_project_path(&path);
    let project = ProjectV1::load(&load_path).map_err(|error| error.to_string())?;
    let resolved_path = load_path.canonicalize().map_err(|error| {
        format!(
            "failed to resolve loaded project path {}: {error}",
            load_path.display()
        )
    })?;
    let macros = load_macro_schema(&load_path, &project)?;
    viewport.set_project(project.clone())?;
    Ok(EditorProject {
        path: resolved_path,
        project,
        parameters: parameter_schema(),
        macros,
    })
}

fn load_macro_schema(
    path: &std::path::Path,
    project: &ProjectV1,
) -> Result<Vec<MacroParameterV1>, String> {
    let Some(selection) = &project.visual_preset else {
        return Ok(Vec::new());
    };
    let preset_path = path
        .parent()
        .unwrap_or_else(|| std::path::Path::new("."))
        .join(&selection.source);
    VisualPresetV1::load(preset_path)
        .map(|preset| preset.macros)
        .map_err(|error| error.to_string())
}

fn parameter_schema() -> Vec<ParameterSchema> {
    vec![
        ParameterSchema {
            path: "particle_system.count",
            label: "Particle count",
            kind: "number",
            minimum: Some(1.0),
            maximum: Some(20_000_000.0),
            step: Some(1000.0),
            modulation_target: None,
        },
        ParameterSchema {
            path: "particle_system.substeps",
            label: "Simulation substeps",
            kind: "number",
            minimum: Some(1.0),
            maximum: Some(16.0),
            step: Some(1.0),
            modulation_target: None,
        },
        ParameterSchema {
            path: "render_defaults.particle_size_pixels",
            label: "Particle size",
            kind: "number",
            minimum: Some(0.1),
            maximum: Some(64.0),
            step: Some(0.1),
            modulation_target: Some("particle_size"),
        },
        ParameterSchema {
            path: "camera.vertical_fov_degrees",
            label: "Camera FOV",
            kind: "number",
            minimum: Some(1.0),
            maximum: Some(178.0),
            step: Some(0.5),
            modulation_target: Some("camera_fov"),
        },
        ParameterSchema {
            path: "camera.orbit_degrees_per_second",
            label: "Orbit speed",
            kind: "number",
            minimum: Some(-180.0),
            maximum: Some(180.0),
            step: Some(0.5),
            modulation_target: None,
        },
        ParameterSchema {
            path: "camera.shake_amplitude",
            label: "Camera shake",
            kind: "number",
            minimum: Some(0.0),
            maximum: Some(2.0),
            step: Some(0.01),
            modulation_target: Some("camera_shake"),
        },
        ParameterSchema {
            path: "camera.mode",
            label: "Orbit camera",
            kind: "toggle",
            minimum: None,
            maximum: None,
            step: None,
            modulation_target: None,
        },
        ParameterSchema {
            path: "render_defaults.background",
            label: "Background",
            kind: "color",
            minimum: None,
            maximum: None,
            step: None,
            modulation_target: None,
        },
    ]
}

#[tauri::command]
fn update_project(
    project: ProjectV1,
    viewport: State<'_, ViewportController>,
) -> Result<ProjectV1, String> {
    project.validate().map_err(|error| error.to_string())?;
    viewport.set_project(project.clone())?;
    Ok(project)
}

#[tauri::command]
fn save_project(path: PathBuf, mut project: ProjectV1) -> Result<(), String> {
    project.visual_preset = None;
    project.reaction_profile = None;
    project.save(path).map_err(|error| error.to_string())
}

fn resolve_project_path(path: &std::path::Path) -> PathBuf {
    if path.is_absolute() || path.exists() {
        return path.to_owned();
    }
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../..")
        .join(path)
}

#[tauri::command]
async fn load_preview_audio(
    path: PathBuf,
    viewport: State<'_, ViewportController>,
) -> Result<TimelineAudio, String> {
    let analysis =
        analyze_cached(path, AnalysisConfig::default()).map_err(|error| error.to_string())?;
    let stride = analysis.waveform.len().div_ceil(4_096).max(1);
    let waveform = analysis
        .waveform
        .chunks(stride)
        .map(|chunk| {
            chunk
                .iter()
                .fold([1.0_f32, -1.0_f32], |[minimum, maximum], bucket| {
                    [minimum.min(bucket.minimum), maximum.max(bucket.maximum)]
                })
        })
        .collect();
    let transient_times = analysis
        .frames
        .windows(3)
        .filter(|frames| {
            frames[1].transient_strength >= 0.65
                && frames[1].transient_strength > frames[0].transient_strength
                && frames[1].transient_strength >= frames[2].transient_strength
        })
        .map(|frames| frames[1].time_seconds)
        .collect();
    let timeline = TimelineAudio {
        duration_seconds: analysis.duration_seconds,
        waveform,
        transient_times,
        beat_times: Vec::new(),
    };
    viewport.set_audio(analysis)?;
    Ok(timeline)
}

#[tauri::command]
fn viewport_resize(
    x: i32,
    y: i32,
    width: u32,
    height: u32,
    viewport: State<'_, ViewportController>,
) -> Result<(), String> {
    viewport.resize(x, y, width, height)
}

#[tauri::command]
fn viewport_visible(visible: bool, viewport: State<'_, ViewportController>) {
    viewport.set_visible(visible);
}

#[tauri::command]
fn preview_play(playing: bool, viewport: State<'_, ViewportController>) -> Result<(), String> {
    viewport.play(playing)
}

#[tauri::command]
fn preview_seek(frame: u32, viewport: State<'_, ViewportController>) -> Result<(), String> {
    viewport.seek(frame)
}

#[tauri::command]
fn preview_reset(viewport: State<'_, ViewportController>) -> Result<(), String> {
    viewport.reset()
}

#[tauri::command]
fn preview_quality(quality: &str, viewport: State<'_, ViewportController>) -> Result<(), String> {
    let quality = match quality {
        "draft" => PreviewQuality::Draft,
        "preview" => PreviewQuality::Preview,
        "final" => PreviewQuality::Final,
        _ => return Err(format!("unknown preview quality '{quality}'")),
    };
    viewport.quality(quality)
}

#[tauri::command]
fn preview_stats(viewport: State<'_, ViewportController>) -> Result<PreviewStats, String> {
    viewport.stats()
}

#[tauri::command]
fn enqueue_preview(
    kind: &str,
    project: ProjectV1,
    audio_path: PathBuf,
    start_frame: u32,
    end_frame: u32,
    use_final_settings: bool,
    queue: State<'_, PreviewQueue>,
) -> Result<u64, String> {
    queue.enqueue(
        kind,
        project,
        audio_path,
        start_frame,
        end_frame,
        use_final_settings,
    )
}

#[tauri::command]
fn preview_jobs(queue: State<'_, PreviewQueue>) -> Result<Vec<PreviewJobStatus>, String> {
    queue.statuses()
}

#[tauri::command]
fn clear_previews(queue: State<'_, PreviewQueue>) -> Result<(), String> {
    queue.clear()
}

#[tauri::command]
fn open_preview(path: PathBuf, queue: State<'_, PreviewQueue>) -> Result<(), String> {
    queue.open(&path)
}

#[tauri::command]
fn enqueue_production_render(
    project: ProjectV1,
    audio_path: PathBuf,
    settings: ProductionSettings,
    queue: State<'_, ProductionQueue>,
) -> Result<(), String> {
    queue.enqueue(project, audio_path, settings)
}

#[tauri::command]
fn production_render_status(queue: State<'_, ProductionQueue>) -> Result<ProductionStatus, String> {
    queue.status()
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
/// Starts the desktop application event loop.
///
/// # Panics
///
/// Panics when Tauri cannot initialize or run the application.
pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            let window = app
                .get_webview_window("main")
                .ok_or("main window is missing")?;
            let handle = window.window_handle()?.as_raw();
            let RawWindowHandle::Win32(handle) = handle else {
                return Err("embedded viewport currently requires Windows".into());
            };
            app.manage(ViewportController::new(handle.hwnd.get())?);
            app.manage(PreviewQueue::new()?);
            app.manage(ProductionQueue::new()?);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            gpu_info,
            load_project,
            load_preview_audio,
            viewport_resize,
            viewport_visible,
            preview_play,
            preview_seek,
            preview_reset,
            preview_quality,
            preview_stats,
            update_project,
            save_project,
            enqueue_preview,
            preview_jobs,
            clear_previews,
            open_preview,
            enqueue_production_render,
            production_render_status,
        ])
        .run(tauri::generate_context!())
        .expect("failed to run Rustique desktop application");
}
