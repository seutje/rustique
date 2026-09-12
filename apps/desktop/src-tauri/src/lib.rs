#![allow(clippy::needless_pass_by_value)] // Tauri commands require owned State extractors.

use std::path::PathBuf;

use audio_engine::{AnalysisConfig, analyze_cached};
use project_format::ProjectV1;
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use render_core::{GpuConfig, GpuContext};
use serde::Serialize;
use tauri::{Manager, State};

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
struct ProjectSummary {
    path: PathBuf,
    engine_version: String,
    duration_seconds: f32,
    fps: u32,
    particle_count: u32,
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
) -> Result<ProjectSummary, String> {
    let load_path = resolve_project_path(&path);
    let project = ProjectV1::load(&load_path).map_err(|error| error.to_string())?;
    let resolved_path = load_path.canonicalize().map_err(|error| {
        format!(
            "failed to resolve loaded project path {}: {error}",
            load_path.display()
        )
    })?;
    let summary = ProjectSummary {
        path: resolved_path,
        engine_version: project.engine_version.clone(),
        duration_seconds: project.duration_seconds,
        fps: project.fps,
        particle_count: project.particle_system.count,
    };
    viewport.set_project(project)?;
    Ok(summary)
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
) -> Result<(), String> {
    let analysis =
        analyze_cached(path, AnalysisConfig::default()).map_err(|error| error.to_string())?;
    viewport.set_audio(analysis)
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
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            gpu_info,
            load_project,
            load_preview_audio,
            viewport_resize,
            preview_play,
            preview_seek,
            preview_reset,
            preview_quality,
            preview_stats,
        ])
        .run(tauri::generate_context!())
        .expect("failed to run Rustique desktop application");
}
