use std::{env, num::NonZeroU32, path::PathBuf, process::ExitCode, time::Instant};

use audio_engine::{AnalysisConfig, analyze_cached};
use exporter::{PngSequenceConfig, render_png_sequence};
use project_format::{EnvelopeSmoother, ProjectV1, evaluate_mappings};
use render_core::{
    BackendPreference, BenchmarkConfig, GpuConfig, GpuContext, OffscreenRenderTarget,
    ParticleRenderer, RgbaColor,
};
use simulation::{Force, SimulationTiming};

const BENCHMARK_COUNTS: [u32; 7] = [
    100_000, 500_000, 1_000_000, 2_000_000, 5_000_000, 10_000_000, 20_000_000,
];

fn main() -> ExitCode {
    match run(env::args().skip(1)) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run(args: impl Iterator<Item = String>) -> Result<(), String> {
    let options = CliOptions::parse(args)?;
    if options.help {
        print_help();
        return Ok(());
    }
    if let Some(Command::AudioInfo(audio)) = &options.command {
        return run_audio_info(audio);
    }
    if let Some(Command::ModulationInfo(modulation)) = &options.command {
        return run_modulation_info(modulation);
    }
    let context = pollster::block_on(GpuContext::new(GpuConfig {
        backend: options.backend,
    }))
    .map_err(|error| format!("GPU initialization failed: {error}"))?;
    match options.command {
        Some(Command::GpuInfo) => {
            println!("{}", context.info());
            Ok(())
        }
        Some(Command::Still(still)) => {
            if let Some(project_path) = &still.project {
                return render_project_still(&context, &still, project_path);
            }
            let width = still.width.unwrap_or(1920);
            let height = still.height.unwrap_or(1080);
            let target = OffscreenRenderTarget::new(&context, width, height)
                .map_err(|error| format!("failed to create offscreen target: {error}"))?;
            target
                .save_clear_png(&context, still.color, &still.output)
                .map_err(|error| format!("failed to render still: {error}"))?;
            println!(
                "Rendered {}x{} still to {}",
                width,
                height,
                still.output.display()
            );
            Ok(())
        }
        Some(Command::Particles(particles)) => {
            let target = OffscreenRenderTarget::new(&context, particles.width, particles.height)
                .map_err(|error| format!("failed to create offscreen target: {error}"))?;
            let mut renderer = ParticleRenderer::new(&context, particles.count, particles.seed)
                .map_err(|error| format!("failed to create particle renderer: {error}"))?;
            renderer
                .set_forces(&context, particles.motion.forces())
                .map_err(|error| format!("failed to configure forces: {error}"))?;
            let timing = SimulationTiming::new(
                NonZeroU32::new(particles.fps)
                    .ok_or_else(|| "fps must be greater than zero".to_owned())?,
                NonZeroU32::new(particles.fps)
                    .ok_or_else(|| "fps must be greater than zero".to_owned())?,
                NonZeroU32::new(particles.substeps)
                    .ok_or_else(|| "substeps must be greater than zero".to_owned())?,
            );
            renderer
                .save_timeline_frame_png(
                    &context,
                    &target,
                    particles.frame,
                    timing,
                    BenchmarkConfig::default(),
                    RgbaColor::BLACK,
                    &particles.output,
                )
                .map_err(|error| format!("failed to render particles: {error}"))?;
            println!(
                "Rendered {} particles at frame {} to {}",
                particles.count,
                particles.frame,
                particles.output.display()
            );
            Ok(())
        }
        Some(Command::Benchmark(benchmark)) => run_benchmark(&context, &benchmark),
        Some(Command::AudioInfo(_)) => unreachable!("audio command returned before GPU setup"),
        Some(Command::ModulationInfo(_)) => {
            unreachable!("modulation command returned before GPU setup")
        }
        Some(Command::Sequence(sequence)) => run_sequence(&context, &sequence),
        None => Err("no command specified; use still, --gpu-info, or --help".into()),
    }
}

#[derive(Debug, Default, PartialEq)]
struct CliOptions {
    command: Option<Command>,
    help: bool,
    backend: BackendPreference,
}

#[derive(Debug, PartialEq)]
enum Command {
    GpuInfo,
    Still(StillOptions),
    Particles(ParticleOptions),
    Benchmark(BenchmarkOptions),
    AudioInfo(AudioInfoOptions),
    ModulationInfo(ModulationInfoOptions),
    Sequence(SequenceOptions),
}

#[derive(Debug, PartialEq)]
struct StillOptions {
    project: Option<PathBuf>,
    output: PathBuf,
    width: Option<u32>,
    height: Option<u32>,
    color: RgbaColor,
    frame: u32,
}

#[derive(Debug, PartialEq)]
struct ParticleOptions {
    output: PathBuf,
    width: u32,
    height: u32,
    count: u32,
    seed: u64,
    frame: u32,
    fps: u32,
    substeps: u32,
    motion: MotionPreset,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
enum MotionPreset {
    #[default]
    None,
    Orbit,
    Swirl,
}

impl MotionPreset {
    fn forces(self) -> &'static [Force] {
        const ORBIT: &[Force] = &[
            Force::PointAttractor {
                position: [0.0; 3],
                strength: 0.08,
            },
            Force::Vortex {
                center: [0.0; 3],
                strength: 0.2,
            },
            Force::Drag { coefficient: 0.02 },
        ];
        const SWIRL: &[Force] = &[
            Force::CurlNoise {
                strength: 0.3,
                frequency: 4.0,
            },
            Force::Vortex {
                center: [0.0; 3],
                strength: 0.12,
            },
            Force::SphereConstraint {
                center: [0.0; 3],
                radius: 0.95,
                bounce: 0.8,
            },
        ];
        match self {
            Self::None => &[],
            Self::Orbit => ORBIT,
            Self::Swirl => SWIRL,
        }
    }
}

#[derive(Debug, PartialEq)]
struct BenchmarkOptions {
    count: Option<u32>,
    frames: u32,
    width: u32,
    height: u32,
    particle_size: f32,
    overdraw: bool,
    readback: bool,
}

#[derive(Debug, PartialEq)]
struct AudioInfoOptions {
    input: PathBuf,
    time_seconds: f64,
}

#[derive(Debug, PartialEq)]
struct ModulationInfoOptions {
    project: PathBuf,
    audio: PathBuf,
    time_seconds: f64,
}

#[derive(Debug, PartialEq)]
struct SequenceOptions {
    project: PathBuf,
    audio: PathBuf,
    output_directory: PathBuf,
    start_frame: u32,
    frame_count: Option<u32>,
    width: Option<u32>,
    height: Option<u32>,
}

impl CliOptions {
    fn parse(mut args: impl Iterator<Item = String>) -> Result<Self, String> {
        let mut options = Self::default();
        while let Some(argument) = args.next() {
            match argument.as_str() {
                "--gpu-info" => set_command(&mut options.command, Command::GpuInfo)?,
                "still" => {
                    let still = StillOptions::parse(&mut args, &mut options.backend)?;
                    set_command(&mut options.command, Command::Still(still))?;
                }
                "particles" => {
                    let particles = ParticleOptions::parse(&mut args, &mut options.backend)?;
                    set_command(&mut options.command, Command::Particles(particles))?;
                }
                "benchmark" => {
                    let benchmark = BenchmarkOptions::parse(&mut args, &mut options.backend)?;
                    set_command(&mut options.command, Command::Benchmark(benchmark))?;
                }
                "audio-info" => {
                    let audio = AudioInfoOptions::parse(&mut args)?;
                    set_command(&mut options.command, Command::AudioInfo(audio))?;
                }
                "modulation-info" => {
                    let modulation = ModulationInfoOptions::parse(&mut args)?;
                    set_command(&mut options.command, Command::ModulationInfo(modulation))?;
                }
                "sequence" => {
                    let sequence = SequenceOptions::parse(&mut args, &mut options.backend)?;
                    set_command(&mut options.command, Command::Sequence(sequence))?;
                }
                "-h" | "--help" => options.help = true,
                "--backend" => {
                    let value = args
                        .next()
                        .ok_or_else(|| "--backend requires auto, dx12, or vulkan".to_owned())?;
                    options.backend = parse_backend(&value)?;
                }
                _ => return Err(format!("unknown argument: {argument}")),
            }
        }
        Ok(options)
    }
}

impl StillOptions {
    fn parse(
        args: &mut impl Iterator<Item = String>,
        backend: &mut BackendPreference,
    ) -> Result<Self, String> {
        let mut output = None;
        let mut project = None;
        let mut width = None;
        let mut height = None;
        let mut color = RgbaColor::new(0.02, 0.04, 0.12, 1.0);
        let mut frame = 0;
        while let Some(argument) = args.next() {
            match argument.as_str() {
                "--output" => {
                    output = Some(PathBuf::from(required_value(args, "--output")?));
                }
                "--width" => {
                    width = Some(parse_dimension("width", &required_value(args, "--width")?)?);
                }
                "--height" => {
                    height = Some(parse_dimension(
                        "height",
                        &required_value(args, "--height")?,
                    )?);
                }
                "--color" => color = parse_color(&required_value(args, "--color")?)?,
                "--frame" => frame = parse_number("frame", &required_value(args, "--frame")?)?,
                "--backend" => *backend = parse_backend(&required_value(args, "--backend")?)?,
                _ if !argument.starts_with('-') && project.is_none() => {
                    project = Some(PathBuf::from(argument));
                }
                _ => return Err(format!("unknown still argument: {argument}")),
            }
        }
        Ok(Self {
            project,
            output: output.ok_or_else(|| "still requires --output <path>".to_owned())?,
            width,
            height,
            color,
            frame,
        })
    }
}

impl ParticleOptions {
    fn parse(
        args: &mut impl Iterator<Item = String>,
        backend: &mut BackendPreference,
    ) -> Result<Self, String> {
        let mut output = None;
        let mut width = 1920;
        let mut height = 1080;
        let mut count = 10_000;
        let mut seed = 1;
        let mut frame = 0;
        let mut fps = 60;
        let mut substeps = 1;
        let mut motion = MotionPreset::None;
        while let Some(argument) = args.next() {
            match argument.as_str() {
                "--output" => output = Some(PathBuf::from(required_value(args, "--output")?)),
                "--width" => width = parse_dimension("width", &required_value(args, "--width")?)?,
                "--height" => {
                    height = parse_dimension("height", &required_value(args, "--height")?)?;
                }
                "--count" => count = parse_dimension("count", &required_value(args, "--count")?)?,
                "--seed" => seed = parse_number("seed", &required_value(args, "--seed")?)?,
                "--frame" => frame = parse_number("frame", &required_value(args, "--frame")?)?,
                "--fps" => fps = parse_dimension("fps", &required_value(args, "--fps")?)?,
                "--substeps" => {
                    substeps = parse_dimension("substeps", &required_value(args, "--substeps")?)?;
                }
                "--motion" => motion = parse_motion(&required_value(args, "--motion")?)?,
                "--backend" => *backend = parse_backend(&required_value(args, "--backend")?)?,
                _ => return Err(format!("unknown particles argument: {argument}")),
            }
        }
        Ok(Self {
            output: output.ok_or_else(|| "particles requires --output <path>".to_owned())?,
            width,
            height,
            count,
            seed,
            frame,
            fps,
            substeps,
            motion,
        })
    }
}

impl BenchmarkOptions {
    fn parse(
        args: &mut impl Iterator<Item = String>,
        backend: &mut BackendPreference,
    ) -> Result<Self, String> {
        let mut result = Self {
            count: None,
            frames: 10,
            width: 1920,
            height: 1080,
            particle_size: 2.0,
            overdraw: false,
            readback: false,
        };
        while let Some(argument) = args.next() {
            match argument.as_str() {
                "--count" => {
                    result.count =
                        Some(parse_dimension("count", &required_value(args, "--count")?)?);
                }
                "--frames" => {
                    result.frames = parse_dimension("frames", &required_value(args, "--frames")?)?;
                }
                "--width" => {
                    result.width = parse_dimension("width", &required_value(args, "--width")?)?;
                }
                "--height" => {
                    result.height = parse_dimension("height", &required_value(args, "--height")?)?;
                }
                "--particle-size" => {
                    result.particle_size = parse_positive_float(
                        "particle-size",
                        &required_value(args, "--particle-size")?,
                    )?;
                }
                "--overdraw" => result.overdraw = true,
                "--readback" => result.readback = true,
                "--backend" => *backend = parse_backend(&required_value(args, "--backend")?)?,
                _ => return Err(format!("unknown benchmark argument: {argument}")),
            }
        }
        Ok(result)
    }
}

impl AudioInfoOptions {
    fn parse(args: &mut impl Iterator<Item = String>) -> Result<Self, String> {
        let input = PathBuf::from(required_value(args, "audio-info")?);
        let mut time_seconds = 0.0;
        while let Some(argument) = args.next() {
            match argument.as_str() {
                "--time" => {
                    time_seconds = required_value(args, "--time")?
                        .parse::<f64>()
                        .map_err(|_| "--time requires a non-negative number".to_owned())?;
                    if !time_seconds.is_finite() || time_seconds < 0.0 {
                        return Err("--time requires a non-negative finite number".into());
                    }
                }
                _ => return Err(format!("unknown audio-info argument: {argument}")),
            }
        }
        Ok(Self {
            input,
            time_seconds,
        })
    }
}

impl ModulationInfoOptions {
    fn parse(args: &mut impl Iterator<Item = String>) -> Result<Self, String> {
        let project = PathBuf::from(required_value(args, "modulation-info project")?);
        let audio = PathBuf::from(required_value(args, "modulation-info audio")?);
        let mut time_seconds = 0.0;
        while let Some(argument) = args.next() {
            match argument.as_str() {
                "--time" => {
                    time_seconds =
                        parse_non_negative_float("time", &required_value(args, "--time")?)?;
                }
                _ => return Err(format!("unknown modulation-info argument: {argument}")),
            }
        }
        Ok(Self {
            project,
            audio,
            time_seconds,
        })
    }
}

impl SequenceOptions {
    fn parse(
        args: &mut impl Iterator<Item = String>,
        backend: &mut BackendPreference,
    ) -> Result<Self, String> {
        let project = PathBuf::from(required_value(args, "sequence project")?);
        let mut audio = None;
        let mut output_directory = None;
        let mut start_frame = 0;
        let mut frame_count = None;
        let mut width = None;
        let mut height = None;
        while let Some(argument) = args.next() {
            match argument.as_str() {
                "--audio" => audio = Some(PathBuf::from(required_value(args, "--audio")?)),
                "--output-dir" => {
                    output_directory = Some(PathBuf::from(required_value(args, "--output-dir")?));
                }
                "--start-frame" => {
                    start_frame =
                        parse_number("start-frame", &required_value(args, "--start-frame")?)?;
                }
                "--frames" => {
                    frame_count = Some(parse_dimension(
                        "frames",
                        &required_value(args, "--frames")?,
                    )?);
                }
                "--width" => {
                    width = Some(parse_dimension("width", &required_value(args, "--width")?)?);
                }
                "--height" => {
                    height = Some(parse_dimension(
                        "height",
                        &required_value(args, "--height")?,
                    )?);
                }
                "--backend" => *backend = parse_backend(&required_value(args, "--backend")?)?,
                _ => return Err(format!("unknown sequence argument: {argument}")),
            }
        }
        Ok(Self {
            project,
            audio: audio.ok_or_else(|| "sequence requires --audio <path>".to_owned())?,
            output_directory: output_directory
                .ok_or_else(|| "sequence requires --output-dir <path>".to_owned())?,
            start_frame,
            frame_count,
            width,
            height,
        })
    }
}

fn run_audio_info(options: &AudioInfoOptions) -> Result<(), String> {
    let analysis = analyze_cached(&options.input, AnalysisConfig::default())
        .map_err(|error| format!("audio analysis failed: {error}"))?;
    let sample = analysis.sample_at(options.time_seconds);
    println!("Sample rate: {} Hz", analysis.sample_rate);
    println!("Duration: {:.3} seconds", analysis.duration_seconds);
    println!("Feature frames: {}", analysis.frames.len());
    println!("Waveform buckets: {}", analysis.waveform.len());
    println!(
        "Sample at {:.3}s: RMS {:.4}, sub {:.4}, bass {:.4}, low mids {:.4}, mids {:.4}, high mids {:.4}, highs {:.4}, centroid {:.4}, flux {:.4}, transient {:.4}",
        sample.time_seconds,
        sample.rms,
        sample.bands.sub,
        sample.bands.bass,
        sample.bands.low_mids,
        sample.bands.mids,
        sample.bands.high_mids,
        sample.bands.highs,
        sample.spectral_centroid,
        sample.spectral_flux,
        sample.transient_strength
    );
    println!(
        "Cache: {}",
        audio_engine::cache_path_for(&options.input).display()
    );
    Ok(())
}

#[allow(clippy::cast_precision_loss)]
fn run_modulation_info(options: &ModulationInfoOptions) -> Result<(), String> {
    let project = ProjectV1::load(&options.project).map_err(|error| error.to_string())?;
    let analysis = analyze_cached(&options.audio, AnalysisConfig::default())
        .map_err(|error| error.to_string())?;
    let mut smoothers = vec![EnvelopeSmoother::default(); project.modulation_mappings.len()];
    let delta = analysis.hop_size as f32 / analysis.sample_rate as f32;
    let mut active = Vec::new();
    for features in analysis
        .frames
        .iter()
        .take_while(|frame| frame.time_seconds <= options.time_seconds)
    {
        active = evaluate_mappings(
            &project.modulation_mappings,
            &mut smoothers,
            *features,
            delta,
        );
    }
    if active.is_empty() {
        active = evaluate_mappings(
            &project.modulation_mappings,
            &mut smoothers,
            analysis.sample_at(options.time_seconds),
            delta,
        );
    }
    for value in active {
        println!(
            "target={:?} source={:.4} value={:.4}",
            value.target, value.source_value, value.output_value
        );
    }
    Ok(())
}

#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn run_sequence(context: &GpuContext, options: &SequenceOptions) -> Result<(), String> {
    let project = ProjectV1::load(&options.project).map_err(|error| error.to_string())?;
    let analysis = analyze_cached(&options.audio, AnalysisConfig::default())
        .map_err(|error| error.to_string())?;
    let seconds = f64::from(project.duration_seconds).min(analysis.duration_seconds);
    let default_end = (seconds * f64::from(project.fps))
        .floor()
        .min(f64::from(u32::MAX)) as u32;
    let end_frame = options.frame_count.map_or(default_end, |count| {
        options.start_frame.saturating_add(count)
    });
    let report = render_png_sequence(
        context,
        &project,
        &analysis,
        &PngSequenceConfig {
            output_directory: options.output_directory.clone(),
            start_frame: options.start_frame,
            end_frame,
            width: options.width.unwrap_or(project.render_defaults.width),
            height: options.height.unwrap_or(project.render_defaults.height),
        },
    )
    .map_err(|error| format!("sequence render failed: {error}"))?;
    println!(
        "Rendered {} audio-reactive frames to {}",
        report.frames_rendered,
        options.output_directory.display()
    );
    Ok(())
}

fn parse_non_negative_float(name: &str, value: &str) -> Result<f64, String> {
    let parsed = value
        .parse::<f64>()
        .map_err(|_| format!("invalid {name} '{value}'"))?;
    if parsed.is_finite() && parsed >= 0.0 {
        Ok(parsed)
    } else {
        Err(format!("{name} must be non-negative and finite"))
    }
}

fn run_benchmark(context: &GpuContext, options: &BenchmarkOptions) -> Result<(), String> {
    let counts: Vec<u32> = options
        .count
        .map_or_else(|| BENCHMARK_COUNTS.to_vec(), |count| vec![count]);
    println!(
        "count,buffer_mib,cpu_prepare_ms,gpu_compute_ms,gpu_render_ms,total_gpu_ms,readback_frame_ms"
    );
    for count in counts {
        let target = OffscreenRenderTarget::new(context, options.width, options.height)
            .map_err(|error| error.to_string())?;
        let mut renderer =
            ParticleRenderer::new(context, count, 1).map_err(|error| error.to_string())?;
        let mut cpu = 0.0;
        let mut compute = 0.0;
        let mut render = 0.0;
        let mut readback = 0.0;
        for frame in 0..options.frames {
            let timing = renderer
                .benchmark_frame(
                    context,
                    &target,
                    frame,
                    60.0,
                    BenchmarkConfig {
                        particle_size_pixels: options.particle_size,
                        position_scale: if options.overdraw { 0.02 } else { 1.0 },
                        ..BenchmarkConfig::default()
                    },
                )
                .map_err(|error| error.to_string())?;
            cpu += timing.cpu_prepare_ms;
            compute += timing.gpu_compute_ms.unwrap_or(0.0);
            render += timing.gpu_render_ms.unwrap_or(0.0);
            if options.readback {
                let started = Instant::now();
                let _pixels = renderer
                    .render_frame(context, &target, frame, 60.0, RgbaColor::BLACK)
                    .map_err(|error| error.to_string())?;
                readback += started.elapsed().as_secs_f64() * 1000.0;
            }
        }
        let frames = f64::from(options.frames);
        println!(
            "{count},{:.2},{:.4},{:.4},{:.4},{:.4},{}",
            f64::from(count) * 128.0 / 1_048_576.0,
            cpu / frames,
            compute / frames,
            render / frames,
            (compute + render) / frames,
            if options.readback {
                format!("{:.4}", readback / frames)
            } else {
                String::new()
            }
        );
    }
    Ok(())
}

fn render_project_still(
    context: &GpuContext,
    options: &StillOptions,
    project_path: &PathBuf,
) -> Result<(), String> {
    let project = ProjectV1::load(project_path)
        .map_err(|error| format!("failed to load project: {error}"))?;
    let width = options.width.unwrap_or(project.render_defaults.width);
    let height = options.height.unwrap_or(project.render_defaults.height);
    let target = OffscreenRenderTarget::new(context, width, height)
        .map_err(|error| format!("failed to create offscreen target: {error}"))?;
    let mut renderer = ParticleRenderer::new(context, project.particle_system.count, project.seed)
        .map_err(|error| format!("failed to create particle renderer: {error}"))?;
    renderer
        .set_forces(context, &project.forces)
        .map_err(|error| format!("failed to configure project forces: {error}"))?;
    let project_fps = NonZeroU32::new(project.fps)
        .ok_or_else(|| "project FPS must be greater than zero".to_owned())?;
    let substeps = NonZeroU32::new(project.particle_system.substeps)
        .ok_or_else(|| "project substeps must be greater than zero".to_owned())?;
    let background = project.render_defaults.background;
    renderer
        .save_timeline_frame_png(
            context,
            &target,
            options.frame,
            SimulationTiming::new(project_fps, project_fps, substeps),
            BenchmarkConfig {
                particle_size_pixels: project.render_defaults.particle_size_pixels,
                position_scale: 1.0,
                ..BenchmarkConfig::default()
            },
            RgbaColor::new(background[0], background[1], background[2], background[3]),
            &options.output,
        )
        .map_err(|error| format!("failed to render project still: {error}"))?;
    println!(
        "Rendered project {} frame {} at {}x{} to {}",
        project_path.display(),
        options.frame,
        width,
        height,
        options.output.display()
    );
    Ok(())
}

fn parse_positive_float(name: &str, value: &str) -> Result<f32, String> {
    let parsed = value
        .parse::<f32>()
        .map_err(|_| format!("invalid {name} '{value}'"))?;
    if parsed.is_finite() && parsed > 0.0 {
        Ok(parsed)
    } else {
        Err(format!("{name} must be a positive finite number"))
    }
}

fn parse_motion(value: &str) -> Result<MotionPreset, String> {
    match value {
        "none" => Ok(MotionPreset::None),
        "orbit" => Ok(MotionPreset::Orbit),
        "swirl" => Ok(MotionPreset::Swirl),
        _ => Err(format!(
            "unknown motion preset '{value}'; expected none, orbit, or swirl"
        )),
    }
}

fn set_command(command: &mut Option<Command>, value: Command) -> Result<(), String> {
    if command.is_some() {
        return Err("only one command may be specified".into());
    }
    *command = Some(value);
    Ok(())
}

fn required_value(args: &mut impl Iterator<Item = String>, option: &str) -> Result<String, String> {
    args.next()
        .ok_or_else(|| format!("{option} requires a value"))
}

fn parse_dimension(name: &str, value: &str) -> Result<u32, String> {
    let dimension = value
        .parse::<u32>()
        .map_err(|_| format!("invalid {name} '{value}'; expected a positive integer"))?;
    if dimension == 0 {
        return Err(format!("{name} must be greater than zero"));
    }
    Ok(dimension)
}

fn parse_number<T>(name: &str, value: &str) -> Result<T, String>
where
    T: std::str::FromStr,
{
    value
        .parse()
        .map_err(|_| format!("invalid {name} '{value}'"))
}

fn parse_color(value: &str) -> Result<RgbaColor, String> {
    let hex = value.strip_prefix('#').unwrap_or(value);
    if hex.len() != 6 && hex.len() != 8 {
        return Err(format!(
            "invalid color '{value}'; expected RRGGBB or RRGGBBAA"
        ));
    }
    let channel = |offset: usize| {
        u8::from_str_radix(&hex[offset..offset + 2], 16)
            .map(|component| f64::from(component) / 255.0)
            .map_err(|_| format!("invalid color '{value}'; expected hexadecimal digits"))
    };
    Ok(RgbaColor::new(
        channel(0)?,
        channel(2)?,
        channel(4)?,
        if hex.len() == 8 { channel(6)? } else { 1.0 },
    ))
}

fn parse_backend(value: &str) -> Result<BackendPreference, String> {
    match value {
        "auto" => Ok(BackendPreference::Auto),
        "dx12" => Ok(BackendPreference::Dx12),
        "vulkan" => Ok(BackendPreference::Vulkan),
        _ => Err(format!(
            "unsupported backend '{value}'; expected auto, dx12, or vulkan"
        )),
    }
}

fn print_help() {
    println!(
        "particle-render --gpu-info [--backend auto|dx12|vulkan]\n\
         particle-render still [project.json] --output <path> [--frame 0] \
         [--width 1920] [--height 1080] [--color RRGGBB[AA]] \
         [--backend auto|dx12|vulkan]\n\
         particle-render particles --output <path> [--count 10000] [--seed 1] \
         [--frame 0] [--fps 60] [--substeps 1] [--motion none|orbit|swirl] \
         [--width 1920] [--height 1080] \
         [--backend auto|dx12|vulkan]\n\
         particle-render benchmark [--count N] [--frames 10] [--width 1920] \
         [--height 1080] [--particle-size 2] [--overdraw] [--readback] \
         [--backend auto|dx12|vulkan]\n\
         particle-render audio-info <audio-path> [--time seconds]\n\
         particle-render modulation-info <project.json> <audio-path> [--time seconds]\n\
         particle-render sequence <project.json> --audio <path> --output-dir <path> \
         [--start-frame 0] [--frames N] [--width W] [--height H]"
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_gpu_info_with_vulkan_override() {
        let options = CliOptions::parse(
            ["--gpu-info", "--backend", "vulkan"]
                .into_iter()
                .map(str::to_owned),
        )
        .unwrap();
        assert_eq!(options.command, Some(Command::GpuInfo));
        assert_eq!(options.backend, BackendPreference::Vulkan);
    }

    #[test]
    fn rejects_unknown_backend() {
        let error =
            CliOptions::parse(["--backend", "metal"].into_iter().map(str::to_owned)).unwrap_err();
        assert!(error.contains("unsupported backend"));
    }

    #[test]
    fn parses_still_options_and_color() {
        let options = CliOptions::parse(
            [
                "still",
                "--output",
                "frame.png",
                "--width",
                "13",
                "--height",
                "7",
                "--color",
                "#ff800040",
            ]
            .into_iter()
            .map(str::to_owned),
        )
        .unwrap();
        assert_eq!(
            options.command,
            Some(Command::Still(StillOptions {
                project: None,
                output: PathBuf::from("frame.png"),
                width: Some(13),
                height: Some(7),
                color: RgbaColor::new(1.0, 128.0 / 255.0, 0.0, 64.0 / 255.0),
                frame: 0,
            }))
        );
    }

    #[test]
    fn rejects_zero_dimensions() {
        let error = CliOptions::parse(
            ["still", "--output", "frame.png", "--width", "0"]
                .into_iter()
                .map(str::to_owned),
        )
        .unwrap_err();
        assert!(error.contains("greater than zero"));
    }

    #[test]
    fn parses_particle_options() {
        let options = CliOptions::parse(
            [
                "particles",
                "--output",
                "particles.png",
                "--count",
                "1000000",
                "--seed",
                "9",
                "--frame",
                "4",
            ]
            .into_iter()
            .map(str::to_owned),
        )
        .unwrap();
        assert!(matches!(
            options.command,
            Some(Command::Particles(ParticleOptions {
                count: 1_000_000,
                seed: 9,
                frame: 4,
                ..
            }))
        ));
    }

    #[test]
    fn parses_project_still() {
        let options = CliOptions::parse(
            [
                "still",
                "examples/star-orbit.rustique.json",
                "--output",
                "frame.png",
                "--frame",
                "30",
            ]
            .into_iter()
            .map(str::to_owned),
        )
        .unwrap();
        assert!(matches!(
            options.command,
            Some(Command::Still(StillOptions {
                project: Some(_),
                frame: 30,
                ..
            }))
        ));
    }

    #[test]
    fn parses_audio_feature_inspection() {
        let options = CliOptions::parse(
            ["audio-info", "track.flac", "--time", "12.5"]
                .into_iter()
                .map(str::to_owned),
        )
        .unwrap();
        assert_eq!(
            options.command,
            Some(Command::AudioInfo(AudioInfoOptions {
                input: PathBuf::from("track.flac"),
                time_seconds: 12.5
            }))
        );
    }

    #[test]
    fn parses_audio_reactive_sequence() {
        let options = CliOptions::parse(
            [
                "sequence",
                "project.json",
                "--audio",
                "track.wav",
                "--output-dir",
                "frames",
                "--frames",
                "60",
            ]
            .into_iter()
            .map(str::to_owned),
        )
        .unwrap();
        assert!(matches!(
            options.command,
            Some(Command::Sequence(SequenceOptions {
                frame_count: Some(60),
                ..
            }))
        ));
    }

    #[test]
    fn parses_benchmark_stress_options() {
        let options = CliOptions::parse(
            [
                "benchmark",
                "--count",
                "500000",
                "--frames",
                "3",
                "--particle-size",
                "8",
                "--overdraw",
            ]
            .into_iter()
            .map(str::to_owned),
        )
        .unwrap();
        assert!(matches!(
            options.command,
            Some(Command::Benchmark(BenchmarkOptions {
                count: Some(500_000),
                frames: 3,
                particle_size: 8.0,
                overdraw: true,
                ..
            }))
        ));
    }
}
