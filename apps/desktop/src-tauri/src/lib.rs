#![allow(clippy::needless_pass_by_value)] // Tauri commands require owned State extractors.

use std::path::PathBuf;

use audio_engine::{AnalysisConfig, analyze_cached};
use project_format::{
    MacroParameterV1, ProjectV1, VisualPresetV1, configure_seamless_camera_loop, morph_presets,
    randomize_preset_macros,
};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use serde::Serialize;
use tauri::{Manager, State};

mod preview;
use preview::{PreviewJobStatus, PreviewQueue};
mod live_input;
use live_input::{LiveInputController, LiveInputValue};
mod production;
use production::{ProductionQueue, ProductionSettings, ProductionStatus};

#[cfg(target_os = "windows")]
mod viewport;
#[cfg(target_os = "windows")]
use viewport::{HardwareScan, PreviewQuality, PreviewStats, ViewportController};

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
fn gpu_info(viewport: State<'_, ViewportController>) -> Result<HardwareScan, String> {
    viewport.hardware_scan()
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
    let parameters = parameter_schema(&project);
    viewport.set_project(project.clone())?;
    Ok(EditorProject {
        path: resolved_path,
        project,
        parameters,
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

#[allow(clippy::too_many_lines)]
fn parameter_schema(project: &ProjectV1) -> Vec<ParameterSchema> {
    let mut schema = vec![
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
            path: "render_defaults.particle_depth_near",
            label: "Depth near",
            kind: "number",
            minimum: Some(0.01),
            maximum: Some(100.0),
            step: Some(0.01),
            modulation_target: None,
        },
        ParameterSchema {
            path: "render_defaults.particle_depth_far",
            label: "Depth far",
            kind: "number",
            minimum: Some(0.02),
            maximum: Some(100.0),
            step: Some(0.01),
            modulation_target: None,
        },
        ParameterSchema {
            path: "render_defaults.particle_depth_size_strength",
            label: "Depth size",
            kind: "number",
            minimum: Some(0.0),
            maximum: Some(1.0),
            step: Some(0.01),
            modulation_target: None,
        },
        ParameterSchema {
            path: "render_defaults.particle_depth_brightness_strength",
            label: "Depth dimming",
            kind: "number",
            minimum: Some(0.0),
            maximum: Some(1.0),
            step: Some(0.01),
            modulation_target: None,
        },
        ParameterSchema {
            path: "render_defaults.particle_depth_tint",
            label: "Depth tint",
            kind: "color",
            minimum: None,
            maximum: None,
            step: None,
            modulation_target: None,
        },
        ParameterSchema {
            path: "render_defaults.particle_depth_color_strength",
            label: "Atmospheric depth",
            kind: "number",
            minimum: Some(0.0),
            maximum: Some(1.0),
            step: Some(0.01),
            modulation_target: None,
        },
        ParameterSchema {
            path: "render_defaults.particle_glow_strength",
            label: "Particle glow",
            kind: "number",
            minimum: Some(0.0),
            maximum: Some(2.0),
            step: Some(0.01),
            modulation_target: None,
        },
        ParameterSchema {
            path: "render_defaults.particle_focus_distance",
            label: "Focus distance",
            kind: "number",
            minimum: Some(0.01),
            maximum: Some(20.0),
            step: Some(0.01),
            modulation_target: None,
        },
        ParameterSchema {
            path: "render_defaults.particle_focus_range",
            label: "Focus range",
            kind: "number",
            minimum: Some(0.01),
            maximum: Some(20.0),
            step: Some(0.01),
            modulation_target: None,
        },
        ParameterSchema {
            path: "render_defaults.particle_dof_strength",
            label: "Particle bokeh",
            kind: "number",
            minimum: Some(0.0),
            maximum: Some(1.0),
            step: Some(0.01),
            modulation_target: None,
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
    ];
    if project.particle_system.flocking.is_some() {
        schema.extend([
            ParameterSchema {
                path: "particle_system.flocking.enabled",
                label: "GPU flocking",
                kind: "toggle",
                minimum: None,
                maximum: None,
                step: None,
                modulation_target: None,
            },
            numeric_parameter(
                "particle_system.flocking.grid_resolution",
                "Field grid resolution",
                4.0,
                1024.0,
                1.0,
                None,
            ),
            numeric_parameter(
                "particle_system.flocking.separation_strength",
                "Separation",
                0.0,
                5.0,
                0.01,
                Some("flocking_separation"),
            ),
            numeric_parameter(
                "particle_system.flocking.alignment_strength",
                "Alignment",
                0.0,
                5.0,
                0.01,
                None,
            ),
            numeric_parameter(
                "particle_system.flocking.cohesion_strength",
                "Cohesion",
                0.0,
                5.0,
                0.01,
                Some("flocking_cohesion"),
            ),
            numeric_parameter(
                "particle_system.flocking.neighborhood_radius",
                "Neighborhood radius",
                0.01,
                0.5,
                0.005,
                None,
            ),
            numeric_parameter(
                "particle_system.flocking.noise_strength",
                "Curl noise",
                0.0,
                5.0,
                0.01,
                Some("flocking_turbulence"),
            ),
            numeric_parameter(
                "particle_system.flocking.noise_scale",
                "Noise scale",
                0.05,
                20.0,
                0.05,
                None,
            ),
            numeric_parameter(
                "particle_system.flocking.noise_evolution_speed",
                "Noise evolution",
                0.0,
                5.0,
                0.01,
                None,
            ),
            numeric_parameter(
                "particle_system.flocking.inertia",
                "Inertia",
                0.0,
                1.0,
                0.01,
                None,
            ),
            numeric_parameter(
                "particle_system.flocking.drag",
                "Drag",
                0.0,
                5.0,
                0.01,
                None,
            ),
            numeric_parameter(
                "particle_system.flocking.max_velocity",
                "Maximum velocity",
                0.01,
                5.0,
                0.01,
                Some("flocking_speed"),
            ),
            numeric_parameter(
                "particle_system.flocking.max_steering_force",
                "Maximum steering",
                0.01,
                10.0,
                0.01,
                None,
            ),
            numeric_parameter(
                "particle_system.flocking.attractor_strength",
                "Attractor strength",
                0.0,
                5.0,
                0.01,
                None,
            ),
            numeric_parameter(
                "particle_system.flocking.attractor_radius",
                "Attractor radius",
                0.01,
                2.0,
                0.01,
                None,
            ),
            numeric_parameter(
                "particle_system.flocking.repulsor_strength",
                "Repulsor strength",
                0.0,
                10.0,
                0.01,
                None,
            ),
            numeric_parameter(
                "particle_system.flocking.repulsor_radius",
                "Repulsor radius",
                0.01,
                2.0,
                0.01,
                None,
            ),
            numeric_parameter(
                "particle_system.flocking.swarm_compactness",
                "Swarm compactness",
                0.05,
                2.0,
                0.01,
                None,
            ),
            numeric_parameter(
                "particle_system.flocking.randomness",
                "Individual variation",
                0.0,
                2.0,
                0.005,
                Some("flocking_randomness"),
            ),
            numeric_parameter(
                "particle_system.flocking.boundary_avoidance_strength",
                "Boundary avoidance",
                0.0,
                10.0,
                0.05,
                None,
            ),
            numeric_parameter(
                "particle_system.flocking.boundary_margin",
                "Boundary margin",
                0.01,
                1.0,
                0.01,
                None,
            ),
            numeric_parameter(
                "particle_system.flocking.directional_bias.0",
                "Flow bias X",
                -2.0,
                2.0,
                0.01,
                None,
            ),
            numeric_parameter(
                "particle_system.flocking.directional_bias.1",
                "Flow bias Y",
                -2.0,
                2.0,
                0.01,
                None,
            ),
            numeric_parameter(
                "particle_system.flocking.directional_bias.2",
                "Flow bias Z",
                -2.0,
                2.0,
                0.01,
                None,
            ),
            ParameterSchema {
                path: "particle_system.flocking.murmuration.enabled",
                label: "Murmuration states",
                kind: "toggle",
                minimum: None,
                maximum: None,
                step: None,
                modulation_target: None,
            },
            numeric_parameter(
                "particle_system.flocking.murmuration.state_duration_seconds",
                "State duration",
                1.0,
                60.0,
                0.1,
                None,
            ),
            numeric_parameter(
                "particle_system.flocking.murmuration.transition_duration_seconds",
                "State transition",
                0.0,
                30.0,
                0.1,
                None,
            ),
        ]);
    }
    schema
}

fn numeric_parameter(
    path: &'static str,
    label: &'static str,
    minimum: f64,
    maximum: f64,
    step: f64,
    modulation_target: Option<&'static str>,
) -> ParameterSchema {
    ParameterSchema {
        path,
        label,
        kind: "number",
        minimum: Some(minimum),
        maximum: Some(maximum),
        step: Some(step),
        modulation_target,
    }
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
fn save_project(path: PathBuf, project: ProjectV1) -> Result<PathBuf, String> {
    let save_path = resolve_project_path(&path);
    project
        .save(&save_path)
        .map_err(|error| error.to_string())?;
    save_path.canonicalize().map_err(|error| {
        format!(
            "saved project but failed to resolve its path {}: {error}",
            save_path.display()
        )
    })
}

#[tauri::command]
fn randomize_project(
    project_path: PathBuf,
    mut project: ProjectV1,
    amount: f32,
    variation: u64,
) -> Result<ProjectV1, String> {
    let selection = project
        .visual_preset
        .clone()
        .ok_or("controlled randomization requires a selected visual preset")?;
    let load_path = resolve_project_path(&project_path);
    let preset_path = load_path
        .parent()
        .unwrap_or_else(|| std::path::Path::new("."))
        .join(&selection.source);
    let preset = VisualPresetV1::load(preset_path).map_err(|error| error.to_string())?;
    let overrides = randomize_preset_macros(&preset, project.seed ^ variation, amount)
        .map_err(|error| error.to_string())?;
    preset
        .apply(&mut project, &overrides)
        .map_err(|error| error.to_string())?;
    project.visual_preset = Some(project_format::PresetSelectionV1 {
        source: selection.source,
        overrides,
    });
    project.validate().map_err(|error| error.to_string())?;
    Ok(project)
}

#[tauri::command]
fn morph_project(
    mut project: ProjectV1,
    left: PathBuf,
    right: PathBuf,
    amount: f32,
) -> Result<ProjectV1, String> {
    let left =
        VisualPresetV1::load(resolve_project_path(&left)).map_err(|error| error.to_string())?;
    let right =
        VisualPresetV1::load(resolve_project_path(&right)).map_err(|error| error.to_string())?;
    morph_presets(&left, &right, amount, &mut project).map_err(|error| error.to_string())?;
    Ok(project)
}

#[tauri::command]
fn make_seamless_loop(mut project: ProjectV1) -> Result<ProjectV1, String> {
    configure_seamless_camera_loop(&mut project).map_err(|error| error.to_string())?;
    project.validate().map_err(|error| error.to_string())?;
    Ok(project)
}

fn resolve_project_path(path: &std::path::Path) -> PathBuf {
    if path.is_absolute() {
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

#[tauri::command]
fn cancel_production_render(queue: State<'_, ProductionQueue>) -> Result<(), String> {
    queue.cancel()
}

#[tauri::command]
fn open_production_output(queue: State<'_, ProductionQueue>) -> Result<(), String> {
    queue.open_output(false)
}

#[tauri::command]
fn open_production_folder(queue: State<'_, ProductionQueue>) -> Result<(), String> {
    queue.open_output(true)
}

#[tauri::command]
fn midi_ports() -> Result<Vec<String>, String> {
    LiveInputController::midi_ports()
}
#[tauri::command]
fn midi_connect(index: usize, input: State<'_, LiveInputController>) -> Result<(), String> {
    input.connect_midi(index)
}
#[tauri::command]
fn osc_listen(address: &str, input: State<'_, LiveInputController>) -> Result<(), String> {
    input.listen_osc(address)
}
#[tauri::command]
fn live_input_values(input: State<'_, LiveInputController>) -> Result<Vec<LiveInputValue>, String> {
    input.snapshot()
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
/// Starts the desktop application event loop.
///
/// # Panics
///
/// Panics when Tauri cannot initialize or run the application.
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
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
            app.manage(LiveInputController::new());
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
            randomize_project,
            morph_project,
            make_seamless_loop,
            save_project,
            enqueue_preview,
            preview_jobs,
            clear_previews,
            open_preview,
            enqueue_production_render,
            production_render_status,
            cancel_production_render,
            open_production_output,
            open_production_folder,
            midi_ports,
            midi_connect,
            osc_listen,
            live_input_values,
        ])
        .run(tauri::generate_context!())
        .expect("failed to run Rustique desktop application");
}
