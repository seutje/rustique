//! Windows-only native child window used for direct wgpu presentation.

use std::{
    num::{NonZeroIsize, NonZeroU32},
    sync::{Arc, Condvar, Mutex, mpsc},
    thread,
    time::{Duration, Instant},
};

use audio_engine::{AudioAnalysis, AudioFeatureFrame};
use project_format::{
    ActiveModulation, EnvelopeSmoother, ModulatedParameters, ProjectV1, evaluate_automation,
    evaluate_mappings,
};
use raw_window_handle::{
    RawDisplayHandle, RawWindowHandle, Win32WindowHandle, WindowsDisplayHandle,
};
use render_core::{
    BenchmarkConfig, GpuConfig, GpuContext, OffscreenRenderTarget, ParticleRenderer,
    PerspectiveCamera, PostProcessConfig, PostProcessQuality, RgbaColor, SurfacePresenter,
};
use serde::Serialize;
use simulation::SimulationTiming;
use windows::{
    Win32::{
        Foundation::HWND,
        UI::WindowsAndMessaging::{
            CreateWindowExW, DestroyWindow, HWND_TOP, SW_HIDE, SW_SHOW, SWP_NOACTIVATE,
            SWP_SHOWWINDOW, SetWindowPos, ShowWindow, WINDOW_EX_STYLE, WS_CHILD, WS_CLIPSIBLINGS,
            WS_VISIBLE,
        },
    },
    core::w,
};

#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PreviewStats {
    pub frame_index: u32,
    pub frames_per_second: f32,
    pub frame_time_ms: f32,
    pub particle_count: u32,
    pub gpu_compute_ms: Option<f32>,
    pub gpu_render_ms: Option<f32>,
    pub playing: bool,
    pub active_modulations: Vec<ActiveModulationSummary>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ActiveModulationSummary {
    pub target: String,
    pub source_value: f32,
    pub output_value: f32,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HardwareScan {
    pub adapter: String,
    pub backend: String,
    pub device_type: String,
    pub driver: String,
    pub max_preview_particles: u32,
    pub benchmark_gpu_ms: Option<f64>,
    pub benchmark_particles: u32,
}

type HardwareScanState = Arc<(Mutex<Option<Result<HardwareScan, String>>>, Condvar)>;

#[derive(Clone, Copy, Debug)]
pub enum PreviewQuality {
    Draft,
    Preview,
    Final,
}

impl PreviewQuality {
    fn particle_count(self, project_count: u32, hardware_limit: u32) -> u32 {
        match self {
            Self::Draft => project_count.min(100_000),
            Self::Preview => project_count.min(500_000),
            Self::Final => project_count,
        }
        .min(hardware_limit)
    }

    const fn post_quality(self) -> PostProcessQuality {
        match self {
            Self::Draft => PostProcessQuality::Draft,
            Self::Preview => PostProcessQuality::Preview,
            Self::Final => PostProcessQuality::Final,
        }
    }
}

enum PreviewCommand {
    Resize { width: u32, height: u32 },
    Project(Box<ProjectV1>),
    Audio(Box<AudioAnalysis>),
    Play(bool),
    Seek(u32),
    Reset,
    Quality(PreviewQuality),
    Shutdown,
}

pub struct ViewportController {
    hwnd: isize,
    sender: mpsc::Sender<PreviewCommand>,
    stats: Arc<Mutex<PreviewStats>>,
    hardware_scan: HardwareScanState,
    render_thread: Option<thread::JoinHandle<()>>,
}

impl ViewportController {
    /// Creates a child window over the Tauri webview and starts its render thread.
    ///
    /// # Errors
    ///
    /// Returns an error when Windows cannot create the child window.
    pub fn new(parent: isize) -> Result<Self, String> {
        // SAFETY: `parent` is the live Tauri window HWND. The built-in STATIC
        // class owns its window procedure, and no borrowed pointer is supplied.
        let child = unsafe {
            CreateWindowExW(
                WINDOW_EX_STYLE::default(),
                w!("STATIC"),
                w!("Rustique Viewport"),
                WS_CHILD | WS_VISIBLE | WS_CLIPSIBLINGS,
                0,
                0,
                1,
                1,
                Some(HWND(parent as *mut _)),
                None,
                None,
                None,
            )
        }
        .map_err(|error| format!("failed to create native viewport: {error}"))?;
        let hwnd = child.0 as isize;
        let (sender, receiver) = mpsc::channel();
        let stats = Arc::new(Mutex::new(PreviewStats::default()));
        let thread_stats = Arc::clone(&stats);
        let hardware_scan = Arc::new((Mutex::new(None), Condvar::new()));
        let thread_hardware_scan = Arc::clone(&hardware_scan);
        let render_thread = thread::Builder::new()
            .name("rustique-preview".into())
            .spawn(move || run_render_thread(hwnd, receiver, thread_stats, thread_hardware_scan))
            .map_err(|error| format!("failed to start viewport render thread: {error}"))?;
        Ok(Self {
            hwnd,
            sender,
            stats,
            hardware_scan,
            render_thread: Some(render_thread),
        })
    }

    pub fn resize(&self, x: i32, y: i32, width: u32, height: u32) -> Result<(), String> {
        let width = width.max(1);
        let height = height.max(1);
        let native_width = i32::try_from(width).map_err(|_| "viewport width is too large")?;
        let native_height = i32::try_from(height).map_err(|_| "viewport height is too large")?;
        // SAFETY: the controller owns a live child HWND until Drop, and all
        // dimensions have been checked for the Win32 signed integer API.
        unsafe {
            SetWindowPos(
                HWND(self.hwnd as *mut _),
                Some(HWND_TOP),
                x,
                y,
                native_width,
                native_height,
                SWP_NOACTIVATE | SWP_SHOWWINDOW,
            )
        }
        .map_err(|error| format!("failed to resize viewport: {error}"))?;
        self.send(PreviewCommand::Resize { width, height })
    }

    pub fn set_visible(&self, visible: bool) {
        // SAFETY: the controller owns this child HWND until Drop. ShowWindow's
        // return value reports the previous visibility rather than failure.
        unsafe {
            let _ = ShowWindow(
                HWND(self.hwnd as *mut _),
                if visible { SW_SHOW } else { SW_HIDE },
            );
        }
    }

    pub fn set_project(&self, project: ProjectV1) -> Result<(), String> {
        self.send(PreviewCommand::Project(Box::new(project)))
    }

    pub fn set_audio(&self, analysis: AudioAnalysis) -> Result<(), String> {
        self.send(PreviewCommand::Audio(Box::new(analysis)))
    }

    pub fn play(&self, playing: bool) -> Result<(), String> {
        self.send(PreviewCommand::Play(playing))?;
        let mut stats = self
            .stats
            .lock()
            .map_err(|_| "preview statistics lock is poisoned")?;
        stats.playing = playing;
        Ok(())
    }
    pub fn seek(&self, frame: u32) -> Result<(), String> {
        self.send(PreviewCommand::Seek(frame))
    }
    pub fn reset(&self) -> Result<(), String> {
        self.send(PreviewCommand::Reset)
    }
    pub fn quality(&self, quality: PreviewQuality) -> Result<(), String> {
        self.send(PreviewCommand::Quality(quality))
    }

    pub fn stats(&self) -> Result<PreviewStats, String> {
        self.stats
            .lock()
            .map(|value| value.clone())
            .map_err(|_| "preview statistics lock is poisoned".into())
    }

    pub fn hardware_scan(&self) -> Result<HardwareScan, String> {
        let (scan, ready) = &*self.hardware_scan;
        let guard = scan.lock().map_err(|_| "hardware scan lock is poisoned")?;
        let (guard, _) = ready
            .wait_timeout_while(guard, Duration::from_secs(15), |value| value.is_none())
            .map_err(|_| "hardware scan lock is poisoned")?;
        match guard.clone() {
            Some(result) => result,
            None => Err("hardware scan timed out".to_owned()),
        }
    }

    fn send(&self, command: PreviewCommand) -> Result<(), String> {
        self.sender
            .send(command)
            .map_err(|_| "viewport render thread is not running".into())
    }
}

impl Drop for ViewportController {
    fn drop(&mut self) {
        let _ = self.sender.send(PreviewCommand::Shutdown);
        if let Some(render_thread) = self.render_thread.take() {
            let _ = render_thread.join();
        }
        // SAFETY: the render thread (and therefore wgpu surface) has stopped,
        // so the child HWND can now be destroyed without invalidating a surface.
        let _ = unsafe { DestroyWindow(HWND(self.hwnd as *mut _)) };
    }
}

struct Scene {
    project: ProjectV1,
    renderer: ParticleRenderer,
    target: OffscreenRenderTarget,
    smoothers: Vec<EnvelopeSmoother>,
}

#[allow(clippy::too_many_lines)]
fn run_render_thread(
    hwnd: isize,
    receiver: mpsc::Receiver<PreviewCommand>,
    stats: Arc<Mutex<PreviewStats>>,
    hardware_scan: HardwareScanState,
) {
    if let Err(error) = pollster::block_on(render_loop(hwnd, receiver, stats, hardware_scan)) {
        eprintln!("viewport renderer stopped: {error}");
    }
}

#[allow(
    clippy::too_many_lines,
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation
)]
async fn render_loop(
    hwnd: isize,
    receiver: mpsc::Receiver<PreviewCommand>,
    stats: Arc<Mutex<PreviewStats>>,
    hardware_scan_state: HardwareScanState,
) -> Result<(), String> {
    let context = match GpuContext::new(GpuConfig::default()).await {
        Ok(context) => context,
        Err(error) => {
            publish_hardware_scan(&hardware_scan_state, Err(error.to_string()));
            return Err(error.to_string());
        }
    };
    let hardware_scan = scan_hardware(&context);
    let hardware_limit = hardware_scan.max_preview_particles;
    publish_hardware_scan(&hardware_scan_state, Ok(hardware_scan));
    let raw_window_handle = RawWindowHandle::Win32(Win32WindowHandle::new(
        NonZeroIsize::new(hwnd).ok_or("native viewport HWND is null")?,
    ));
    let raw_display_handle = RawDisplayHandle::Windows(WindowsDisplayHandle::new());
    // SAFETY: ViewportController owns the child HWND and joins this thread
    // before destroying it, so both raw handles outlive the returned surface.
    let surface = unsafe {
        context
            .instance
            .create_surface_unsafe(wgpu::SurfaceTargetUnsafe::RawHandle {
                raw_display_handle,
                raw_window_handle,
            })
    }
    .map_err(|error| format!("failed to create viewport surface: {error}"))?;
    let capabilities = surface.get_capabilities(&context.adapter);
    let format = capabilities
        .formats
        .iter()
        .copied()
        .find(wgpu::TextureFormat::is_srgb)
        .or_else(|| capabilities.formats.first().copied())
        .ok_or("viewport surface has no formats")?;
    let mut width = 1;
    let mut height = 1;
    let mut surface_config = wgpu::SurfaceConfiguration {
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        format,
        width,
        height,
        present_mode: wgpu::PresentMode::Fifo,
        desired_maximum_frame_latency: 2,
        alpha_mode: capabilities.alpha_modes[0],
        view_formats: vec![],
    };
    surface.configure(&context.device, &surface_config);
    let presenter = SurfacePresenter::new(&context, format);
    let mut project: Option<ProjectV1> = None;
    let mut scene: Option<Scene> = None;
    let mut analysis: Option<AudioAnalysis> = None;
    let mut quality = PreviewQuality::Preview;
    let mut playing = false;
    let mut frame = 0_u32;
    let mut dirty = true;
    let mut last_present = Instant::now();
    let mut fps_started = Instant::now();
    let mut fps_frames = 0_u32;

    loop {
        match receiver.recv_timeout(Duration::from_millis(4)) {
            Ok(PreviewCommand::Resize {
                width: next_width,
                height: next_height,
            }) => {
                width = next_width;
                height = next_height;
                surface_config.width = width;
                surface_config.height = height;
                surface.configure(&context.device, &surface_config);
                scene = None;
                dirty = true;
            }
            Ok(PreviewCommand::Project(next)) => {
                project = Some(*next);
                scene = None;
                // Project edits rebuild GPU resources, but they must not act as
                // transport controls. The new scene is replayed to the existing
                // timeline frame when it is rendered below.
                dirty = true;
            }
            Ok(PreviewCommand::Audio(next)) => {
                analysis = Some(*next);
                dirty = true;
            }
            Ok(PreviewCommand::Play(value)) => {
                playing = value;
                last_present = Instant::now();
                dirty = true;
            }
            Ok(PreviewCommand::Seek(next)) => {
                frame = project.as_ref().map_or(next, |value| {
                    next.min(timeline_frame_count(value).saturating_sub(1))
                });
                dirty = true;
            }
            Ok(PreviewCommand::Reset) => {
                frame = 0;
                if let Some(scene) = &mut scene {
                    scene.renderer.reset(&context);
                    scene.smoothers.fill(EnvelopeSmoother::default());
                }
                dirty = true;
            }
            Ok(PreviewCommand::Quality(next)) => {
                quality = next;
                scene = None;
                dirty = true;
            }
            Ok(PreviewCommand::Shutdown) | Err(mpsc::RecvTimeoutError::Disconnected) => break,
            Err(mpsc::RecvTimeoutError::Timeout) => {}
        }
        if scene.is_none() {
            if let Some(project) = project.clone() {
                let count = quality
                    .particle_count(project.particle_system.count, hardware_limit)
                    .max(1);
                let mut renderer = ParticleRenderer::new_with_initialization(
                    &context,
                    count,
                    project.seed,
                    project.particle_system.initialization,
                    project.particle_system.boundary,
                )
                .map_err(|error| error.to_string())?;
                renderer
                    .set_forces(&context, &project.forces)
                    .map_err(|error| error.to_string())?;
                let target = OffscreenRenderTarget::new_with_post_process(
                    &context,
                    width,
                    height,
                    PostProcessConfig::for_quality(quality.post_quality()),
                )
                .map_err(|error| error.to_string())?;
                let smoothers =
                    vec![EnvelopeSmoother::default(); project.modulation_mappings.len()];
                scene = Some(Scene {
                    project,
                    renderer,
                    target,
                    smoothers,
                });
            }
        }
        let frame_duration = scene.as_ref().map_or(Duration::from_millis(16), |scene| {
            Duration::from_secs_f64(1.0 / f64::from(scene.project.fps))
        });
        if playing && last_present.elapsed() >= frame_duration {
            let next_frame = frame.saturating_add(1);
            if project
                .as_ref()
                .is_some_and(|value| next_frame >= timeline_frame_count(value))
            {
                frame = 0;
                if let Some(scene) = &mut scene {
                    scene.renderer.reset(&context);
                    scene.smoothers.fill(EnvelopeSmoother::default());
                }
            } else {
                frame = next_frame;
            }
            dirty = true;
            last_present = Instant::now();
        }
        if !dirty {
            continue;
        }
        let started = Instant::now();
        let surface_texture = match surface.get_current_texture() {
            Ok(texture) => texture,
            Err(wgpu::SurfaceError::Lost | wgpu::SurfaceError::Outdated) => {
                surface.configure(&context.device, &surface_config);
                continue;
            }
            Err(wgpu::SurfaceError::Timeout) => continue,
            Err(error) => return Err(format!("failed to acquire viewport frame: {error}")),
        };
        let destination = surface_texture
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let mut active_modulations = Vec::new();
        let particle_count = if let Some(scene) = &mut scene {
            let fps = NonZeroU32::new(scene.project.fps).ok_or("project FPS is zero")?;
            let substeps = NonZeroU32::new(scene.project.particle_system.substeps)
                .ok_or("project substeps are zero")?;
            let timing = SimulationTiming::new(fps, fps, substeps);
            let time = f64::from(frame) / f64::from(scene.project.fps);
            let features = analysis
                .as_ref()
                .map_or_else(AudioFeatureFrame::default, |value| value.sample_at(time));
            let mut active = evaluate_mappings(
                &scene.project.modulation_mappings,
                &mut scene.smoothers,
                scene.project.apply_analysis_profile(features),
                1.0 / scene.project.fps as f32,
            );
            active.extend(evaluate_automation(
                &scene.project.automation_tracks,
                time as f32,
            ));
            active_modulations = summarize_modulations(&active);
            let mut parameters = ModulatedParameters {
                particle_size: scene.project.render_defaults.particle_size_pixels,
                ..ModulatedParameters::default()
            };
            parameters.apply(&active);
            let camera = scene.project.camera.sample(
                time as f32,
                parameters.camera_fov,
                parameters.camera_shake,
                scene.project.seed,
            );
            let view_projection = PerspectiveCamera {
                position: camera.position,
                target: camera.target,
                up: camera.up,
                vertical_fov_degrees: camera.vertical_fov_degrees,
                near_plane: camera.near_plane,
                far_plane: camera.far_plane,
            }
            .view_projection(width as f32 / height as f32);
            let background = scene.project.render_defaults.background;
            scene
                .renderer
                .render_timeline_frame_gpu(
                    &context,
                    &scene.target,
                    frame,
                    timing,
                    BenchmarkConfig {
                        particle_size_pixels: parameters.particle_size,
                        force_scale: parameters.gravity_strength,
                        brightness: parameters.brightness,
                        active_particle_count: None,
                        view_projection: Some(view_projection),
                        ..BenchmarkConfig::default()
                    },
                    RgbaColor::new(background[0], background[1], background[2], background[3]),
                )
                .map_err(|error| error.to_string())?;
            context.queue.submit([presenter.encode(
                &context,
                &scene.target.output_view(),
                &destination,
            )]);
            scene.renderer.particle_count()
        } else {
            let mut encoder =
                context
                    .device
                    .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                        label: Some("rustique-empty-preview"),
                    });
            {
                let _pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("rustique-empty-preview-pass"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &destination,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    depth_stencil_attachment: None,
                    timestamp_writes: None,
                    occlusion_query_set: None,
                });
            }
            context.queue.submit([encoder.finish()]);
            0
        };
        surface_texture.present();
        dirty = false;
        fps_frames += 1;
        let elapsed = fps_started.elapsed();
        if let Ok(mut current) = stats.lock() {
            current.frame_index = frame;
            current.frame_time_ms = started.elapsed().as_secs_f32() * 1000.0;
            current.particle_count = particle_count;
            current.playing = playing;
            current.active_modulations = active_modulations;
            if elapsed >= Duration::from_millis(500) {
                current.frames_per_second = fps_frames as f32 / elapsed.as_secs_f32();
            }
        }
        if elapsed >= Duration::from_millis(500) {
            fps_started = Instant::now();
            fps_frames = 0;
        }
    }
    Ok(())
}

fn publish_hardware_scan(
    state: &(Mutex<Option<Result<HardwareScan, String>>>, Condvar),
    result: Result<HardwareScan, String>,
) {
    let (scan, ready) = state;
    if let Ok(mut value) = scan.lock() {
        *value = Some(result);
        ready.notify_all();
    }
}

#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn scan_hardware(context: &GpuContext) -> HardwareScan {
    const BENCHMARK_PARTICLES: u32 = 100_000;
    const PARTICLE_BYTES: u64 = 64;
    let info = context.info();
    let adapter_ceiling = match info.adapter.device_type {
        wgpu::DeviceType::DiscreteGpu => 2_000_000,
        wgpu::DeviceType::IntegratedGpu => 750_000,
        wgpu::DeviceType::VirtualGpu => 350_000,
        wgpu::DeviceType::Cpu | wgpu::DeviceType::Other => 100_000,
    };
    // Each of the two resident particle buffers must fit one storage binding.
    // Keep a 25% reserve for future per-particle data. wgpu does not expose
    // total VRAM portably.
    let buffer_ceiling = ((u64::from(info.limits.max_storage_buffer_binding_size) * 3 / 4)
        / PARTICLE_BYTES)
        .min(u64::from(u32::MAX)) as u32;
    let benchmark = (|| -> Result<Option<f64>, ()> {
        let target = OffscreenRenderTarget::new(context, 1280, 720).map_err(|_| ())?;
        let mut renderer =
            ParticleRenderer::new(context, BENCHMARK_PARTICLES, 1).map_err(|_| ())?;
        let mut samples = Vec::with_capacity(3);
        for frame in 0..3 {
            let timing = renderer
                .benchmark_frame(context, &target, frame, 60.0, BenchmarkConfig::default())
                .map_err(|_| ())?;
            if let (Some(compute), Some(render)) = (timing.gpu_compute_ms, timing.gpu_render_ms) {
                samples.push(compute + render);
            }
        }
        samples.sort_by(f64::total_cmp);
        Ok(samples.get(samples.len() / 2).copied())
    })()
    .ok()
    .flatten();
    // Leave roughly 4.7 ms of a 60 Hz frame for post-processing, presentation,
    // and editor overhead. Adapter ceilings prevent optimistic extrapolation.
    let measured_ceiling = benchmark.map_or(adapter_ceiling, |gpu_ms| {
        if gpu_ms <= f64::EPSILON {
            adapter_ceiling
        } else {
            ((f64::from(BENCHMARK_PARTICLES) * 12.0 / gpu_ms) as u32)
                .max(50_000)
                .min(adapter_ceiling)
        }
    });
    let raw_ceiling = measured_ceiling.min(buffer_ceiling).max(1);
    let max_preview_particles = if raw_ceiling >= 10_000 {
        raw_ceiling.div_euclid(10_000) * 10_000
    } else {
        raw_ceiling
    };
    eprintln!(
        "hardware scan selected a {max_preview_particles} particle interactive preview cap (benchmark: {benchmark:?} ms)"
    );
    HardwareScan {
        adapter: info.adapter.name,
        backend: format!("{:?}", info.adapter.backend),
        device_type: format!("{:?}", info.adapter.device_type),
        driver: info.adapter.driver,
        max_preview_particles,
        benchmark_gpu_ms: benchmark,
        benchmark_particles: BENCHMARK_PARTICLES,
    }
}

fn summarize_modulations(active: &[ActiveModulation]) -> Vec<ActiveModulationSummary> {
    active
        .iter()
        .map(|value| ActiveModulationSummary {
            target: format!("{:?}", value.target).to_lowercase(),
            source_value: value.source_value,
            output_value: value.output_value,
        })
        .collect()
}

#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn timeline_frame_count(project: &ProjectV1) -> u32 {
    (f64::from(project.duration_seconds) * f64::from(project.fps))
        .ceil()
        .clamp(1.0, f64::from(u32::MAX)) as u32
}
