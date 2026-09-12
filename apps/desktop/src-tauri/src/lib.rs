use std::path::PathBuf;

use project_format::ProjectV1;
use render_core::{GpuConfig, GpuContext};
use serde::Serialize;

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
async fn load_project(path: PathBuf) -> Result<ProjectSummary, String> {
    let project = ProjectV1::load(&path).map_err(|error| error.to_string())?;
    Ok(ProjectSummary {
        path,
        engine_version: project.engine_version,
        duration_seconds: project.duration_seconds,
        fps: project.fps,
        particle_count: project.particle_system.count,
    })
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
/// Starts the desktop application event loop.
///
/// # Panics
///
/// Panics when Tauri cannot initialize or run the application.
pub fn run() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![gpu_info, load_project])
        .run(tauri::generate_context!())
        .expect("failed to run Rustique desktop application");
}
