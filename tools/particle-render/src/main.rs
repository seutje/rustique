use std::{env, num::NonZeroU32, path::PathBuf, process::ExitCode, time::Instant};

use audio_engine::{AnalysisConfig, analyze_cached};
use exporter::{
    CancellationToken, PngSequenceConfig, VideoCodec, VideoExportConfig, export_video,
    render_png_sequence,
};
use project_format::{
    AssetKind, EnvelopeSmoother, LayerBlendModeV1, PackageAsset, PackageCreateOptions, ProjectV1,
    RenderModeV1, RenderPackage, SceneLayerV1, evaluate_mappings,
};
use render_core::{
    BackendPreference, BenchmarkConfig, FluidConfig, FluidRenderer, GpuConfig, GpuContext,
    LayerBlendMode, LiquidChromeConfig, LiquidChromeRenderer, OffscreenRenderTarget,
    ParticleRenderer, PerspectiveCamera, PostProcessConfig, PostProcessQuality, PrimitiveTarget,
    RenderPass, RgbaColor, SpatialGrid, SpatialGridConfig, VolumetricConfig, VolumetricQuality,
    VolumetricRenderer, WaterDropletConfig, WaterDropletRenderer, composite_rgba8,
    load_gltf_points, load_svg_points, load_text_points, particles_from_target, primitive_points,
    save_exr, save_render_passes,
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
    if let Some(Command::PackageCreate(package)) = &options.command {
        return run_package_create(package);
    }
    if let Some(Command::PackageValidate(path)) = &options.command {
        return run_package_validate(path);
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
            let target = OffscreenRenderTarget::new_with_post_process(
                &context,
                width,
                height,
                PostProcessConfig::for_quality(still.post_quality),
            )
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
        Some(Command::SpatialBenchmark(options)) => run_spatial_benchmark(&context, &options),
        Some(Command::Fluid(options)) => run_fluid(&context, &options),
        Some(Command::Target(options)) => run_target(&context, &options),
        Some(Command::AudioInfo(_)) => unreachable!("audio command returned before GPU setup"),
        Some(Command::ModulationInfo(_)) => {
            unreachable!("modulation command returned before GPU setup")
        }
        Some(Command::PackageCreate(_) | Command::PackageValidate(_)) => {
            unreachable!("package command returned before GPU setup")
        }
        Some(Command::Sequence(sequence)) => run_sequence(&context, &sequence),
        Some(Command::Video(video)) => run_video(&context, &video),
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
    SpatialBenchmark(SpatialBenchmarkOptions),
    Fluid(FluidOptions),
    Target(TargetOptions),
    AudioInfo(AudioInfoOptions),
    ModulationInfo(ModulationInfoOptions),
    Sequence(SequenceOptions),
    Video(VideoOptions),
    PackageCreate(PackageCreateCliOptions),
    PackageValidate(PathBuf),
}

#[derive(Debug, PartialEq)]
struct StillOptions {
    project: Option<PathBuf>,
    output: PathBuf,
    width: Option<u32>,
    height: Option<u32>,
    color: RgbaColor,
    frame: u32,
    post_quality: PostProcessQuality,
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
struct SpatialBenchmarkOptions {
    count: u32,
    cells_per_axis: u32,
    cell_capacity: u32,
    max_neighbors: u32,
    iterations: u32,
    debug_output: Option<PathBuf>,
}

#[derive(Debug, PartialEq)]
struct FluidOptions {
    output: PathBuf,
    count: u32,
    frames: u32,
    width: u32,
    height: u32,
    cells: u32,
    pressure: f32,
    viscosity: f32,
    cohesion: f32,
    audio_pressure: f32,
    audio_turbulence: f32,
}

#[derive(Debug, PartialEq)]
struct TargetOptions {
    source: TargetSource,
    output: PathBuf,
    count: u32,
    seed: u64,
    width: u32,
    height: u32,
    dissolution: f32,
    passes: bool,
    exr: Option<PathBuf>,
}

#[derive(Debug, PartialEq)]
enum TargetSource {
    Shape(PrimitiveTarget),
    Mesh(PathBuf),
    Svg(PathBuf),
    Text { value: String, font: PathBuf },
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
    audio: Option<PathBuf>,
    output_directory: PathBuf,
    start_frame: u32,
    frame_count: Option<u32>,
    width: Option<u32>,
    height: Option<u32>,
    post_quality: PostProcessQuality,
}

#[derive(Debug, PartialEq)]
struct VideoOptions {
    project: PathBuf,
    audio: Option<PathBuf>,
    output: PathBuf,
    start_frame: u32,
    frame_count: Option<u32>,
    width: Option<u32>,
    height: Option<u32>,
    codec: VideoCodec,
    ffmpeg_path: PathBuf,
    post_quality: PostProcessQuality,
}

#[derive(Debug, PartialEq)]
struct PackageCreateCliOptions {
    project: PathBuf,
    audio: PathBuf,
    output: PathBuf,
    width: Option<u32>,
    height: Option<u32>,
    assets: Vec<PackageAsset>,
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
                "spatial-benchmark" => {
                    let benchmark =
                        SpatialBenchmarkOptions::parse(&mut args, &mut options.backend)?;
                    set_command(&mut options.command, Command::SpatialBenchmark(benchmark))?;
                }
                "fluid" => {
                    let fluid = FluidOptions::parse(&mut args, &mut options.backend)?;
                    set_command(&mut options.command, Command::Fluid(fluid))?;
                }
                "target" => {
                    let target = TargetOptions::parse(&mut args, &mut options.backend)?;
                    set_command(&mut options.command, Command::Target(target))?;
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
                "video" => {
                    let video = VideoOptions::parse(&mut args, &mut options.backend)?;
                    set_command(&mut options.command, Command::Video(video))?;
                }
                "package-create" => {
                    set_command(
                        &mut options.command,
                        Command::PackageCreate(PackageCreateCliOptions::parse(&mut args)?),
                    )?;
                }
                "package-validate" => {
                    let path = PathBuf::from(required_value(&mut args, "package-validate")?);
                    if let Some(extra) = args.next() {
                        return Err(format!("unknown package-validate argument: {extra}"));
                    }
                    set_command(&mut options.command, Command::PackageValidate(path))?;
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
        let mut post_quality = PostProcessQuality::Preview;
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
                "--post-quality" => {
                    post_quality = parse_post_quality(&required_value(args, "--post-quality")?)?;
                }
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
            post_quality,
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

impl SpatialBenchmarkOptions {
    fn parse(
        args: &mut impl Iterator<Item = String>,
        backend: &mut BackendPreference,
    ) -> Result<Self, String> {
        let mut result = Self {
            count: 100_000,
            cells_per_axis: 32,
            cell_capacity: 64,
            max_neighbors: 128,
            iterations: 3,
            debug_output: None,
        };
        while let Some(argument) = args.next() {
            match argument.as_str() {
                "--count" => {
                    result.count = parse_dimension("count", &required_value(args, "--count")?)?;
                }
                "--cells" => {
                    result.cells_per_axis =
                        parse_dimension("cells", &required_value(args, "--cells")?)?;
                }
                "--cell-capacity" => {
                    result.cell_capacity = parse_dimension(
                        "cell-capacity",
                        &required_value(args, "--cell-capacity")?,
                    )?;
                }
                "--max-neighbors" => {
                    result.max_neighbors = parse_dimension(
                        "max-neighbors",
                        &required_value(args, "--max-neighbors")?,
                    )?;
                }
                "--iterations" => {
                    result.iterations =
                        parse_dimension("iterations", &required_value(args, "--iterations")?)?;
                }
                "--debug-output" => {
                    result.debug_output =
                        Some(PathBuf::from(required_value(args, "--debug-output")?));
                }
                "--backend" => *backend = parse_backend(&required_value(args, "--backend")?)?,
                _ => return Err(format!("unknown spatial-benchmark argument: {argument}")),
            }
        }
        Ok(result)
    }
}

impl FluidOptions {
    fn parse(
        args: &mut impl Iterator<Item = String>,
        backend: &mut BackendPreference,
    ) -> Result<Self, String> {
        let mut result = Self {
            output: PathBuf::new(),
            count: 100_000,
            frames: 60,
            width: 1920,
            height: 1080,
            cells: 32,
            pressure: 0.45,
            viscosity: 0.18,
            cohesion: 0.35,
            audio_pressure: 0.0,
            audio_turbulence: 0.0,
        };
        while let Some(argument) = args.next() {
            match argument.as_str() {
                "--output" => result.output = PathBuf::from(required_value(args, "--output")?),
                "--count" => {
                    result.count = parse_dimension("count", &required_value(args, "--count")?)?;
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
                "--cells" => {
                    result.cells = parse_dimension("cells", &required_value(args, "--cells")?)?;
                }
                "--pressure" => {
                    result.pressure =
                        parse_positive_float("pressure", &required_value(args, "--pressure")?)?;
                }
                "--viscosity" => {
                    result.viscosity =
                        parse_positive_float("viscosity", &required_value(args, "--viscosity")?)?;
                }
                "--cohesion" => {
                    result.cohesion =
                        parse_positive_float("cohesion", &required_value(args, "--cohesion")?)?;
                }
                "--audio-pressure" => {
                    result.audio_pressure = parse_positive_float(
                        "audio-pressure",
                        &required_value(args, "--audio-pressure")?,
                    )?;
                }
                "--audio-turbulence" => {
                    result.audio_turbulence = parse_positive_float(
                        "audio-turbulence",
                        &required_value(args, "--audio-turbulence")?,
                    )?;
                }
                "--backend" => *backend = parse_backend(&required_value(args, "--backend")?)?,
                _ => return Err(format!("unknown fluid argument: {argument}")),
            }
        }
        if result.output.as_os_str().is_empty() {
            return Err("fluid requires --output <path>".to_owned());
        }
        Ok(result)
    }
}

impl TargetOptions {
    fn parse(
        args: &mut impl Iterator<Item = String>,
        backend: &mut BackendPreference,
    ) -> Result<Self, String> {
        let mut source = None;
        let mut output = None;
        let mut count = 100_000;
        let mut seed = 1;
        let mut width = 1920;
        let mut height = 1080;
        let mut dissolution = 0.0;
        let mut passes = false;
        let mut exr = None;
        let mut text = None;
        let mut font = None;
        while let Some(argument) = args.next() {
            match argument.as_str() {
                "--shape" => {
                    let value = required_value(args, "--shape")?;
                    let shape = match value.as_str() {
                        "sphere" => PrimitiveTarget::Sphere,
                        "cube" => PrimitiveTarget::Cube,
                        "ring" => PrimitiveTarget::Ring,
                        _ => return Err("--shape requires sphere, cube, or ring".into()),
                    };
                    source = Some(TargetSource::Shape(shape));
                }
                "--mesh" => {
                    source = Some(TargetSource::Mesh(PathBuf::from(required_value(
                        args, "--mesh",
                    )?)));
                }
                "--svg" => {
                    source = Some(TargetSource::Svg(PathBuf::from(required_value(
                        args, "--svg",
                    )?)));
                }
                "--text" => text = Some(required_value(args, "--text")?),
                "--font" => font = Some(PathBuf::from(required_value(args, "--font")?)),
                "--output" => output = Some(PathBuf::from(required_value(args, "--output")?)),
                "--count" => count = parse_dimension("count", &required_value(args, "--count")?)?,
                "--seed" => seed = parse_number("seed", &required_value(args, "--seed")?)?,
                "--width" => width = parse_dimension("width", &required_value(args, "--width")?)?,
                "--height" => {
                    height = parse_dimension("height", &required_value(args, "--height")?)?;
                }
                "--dissolution" => {
                    dissolution =
                        parse_number("dissolution", &required_value(args, "--dissolution")?)?;
                    if !(0.0..=1.0).contains(&dissolution) {
                        return Err("--dissolution must be between 0 and 1".into());
                    }
                }
                "--passes" => passes = true,
                "--exr" => exr = Some(PathBuf::from(required_value(args, "--exr")?)),
                "--backend" => *backend = parse_backend(&required_value(args, "--backend")?)?,
                _ => return Err(format!("unknown target argument: {argument}")),
            }
        }
        if text.is_some() || font.is_some() {
            source = Some(TargetSource::Text {
                value: text.ok_or("--text requires --font")?,
                font: font.ok_or("--font requires --text")?,
            });
        }
        Ok(Self {
            source: source.ok_or("target requires --shape, --mesh, --svg, or --text/--font")?,
            output: output.ok_or("target requires --output <path>")?,
            count,
            seed,
            width,
            height,
            dissolution,
            passes,
            exr,
        })
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
        let mut post_quality = PostProcessQuality::Preview;
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
                "--post-quality" => {
                    post_quality = parse_post_quality(&required_value(args, "--post-quality")?)?;
                }
                _ => return Err(format!("unknown sequence argument: {argument}")),
            }
        }
        Ok(Self {
            project,
            audio,
            output_directory: output_directory
                .ok_or_else(|| "sequence requires --output-dir <path>".to_owned())?,
            start_frame,
            frame_count,
            width,
            height,
            post_quality,
        })
    }
}

impl VideoOptions {
    fn parse(
        args: &mut impl Iterator<Item = String>,
        backend: &mut BackendPreference,
    ) -> Result<Self, String> {
        let project = PathBuf::from(required_value(args, "video project")?);
        let mut audio = None;
        let mut output = None;
        let mut start_frame = 0;
        let mut frame_count = None;
        let mut width = None;
        let mut height = None;
        let mut codec = VideoCodec::H264;
        let mut ffmpeg_path = PathBuf::from("ffmpeg");
        let mut post_quality = PostProcessQuality::Preview;
        while let Some(argument) = args.next() {
            match argument.as_str() {
                "--audio" => audio = Some(PathBuf::from(required_value(args, "--audio")?)),
                "--output" => output = Some(PathBuf::from(required_value(args, "--output")?)),
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
                "--codec" => codec = parse_video_codec(&required_value(args, "--codec")?)?,
                "--ffmpeg" => ffmpeg_path = PathBuf::from(required_value(args, "--ffmpeg")?),
                "--backend" => *backend = parse_backend(&required_value(args, "--backend")?)?,
                "--post-quality" => {
                    post_quality = parse_post_quality(&required_value(args, "--post-quality")?)?;
                }
                _ => return Err(format!("unknown video argument: {argument}")),
            }
        }
        Ok(Self {
            project,
            audio,
            output: output.ok_or_else(|| "video requires --output <path>".to_owned())?,
            start_frame,
            frame_count,
            width,
            height,
            codec,
            ffmpeg_path,
            post_quality,
        })
    }
}

impl PackageCreateCliOptions {
    fn parse(args: &mut impl Iterator<Item = String>) -> Result<Self, String> {
        let project = PathBuf::from(required_value(args, "package-create project")?);
        let mut audio = None;
        let mut output = None;
        let mut width = None;
        let mut height = None;
        let mut assets = Vec::new();
        while let Some(argument) = args.next() {
            match argument.as_str() {
                "--audio" => audio = Some(PathBuf::from(required_value(args, "--audio")?)),
                "--output" => output = Some(PathBuf::from(required_value(args, "--output")?)),
                "--width" => {
                    width = Some(parse_dimension("width", &required_value(args, "--width")?)?);
                }
                "--height" => {
                    height = Some(parse_dimension(
                        "height",
                        &required_value(args, "--height")?,
                    )?);
                }
                "--texture" | "--hdri" | "--mesh" => {
                    let kind = match argument.as_str() {
                        "--texture" => AssetKind::Texture,
                        "--hdri" => AssetKind::Hdri,
                        _ => AssetKind::Mesh,
                    };
                    assets.push(PackageAsset {
                        kind,
                        source: PathBuf::from(required_value(args, &argument)?),
                    });
                }
                _ => return Err(format!("unknown package-create argument: {argument}")),
            }
        }
        Ok(Self {
            project,
            audio: audio.ok_or_else(|| "package-create requires --audio <path>".to_owned())?,
            output: output.ok_or_else(|| {
                "package-create requires --output <directory.rustiqueproject>".to_owned()
            })?,
            width,
            height,
            assets,
        })
    }
}

fn run_package_create(options: &PackageCreateCliOptions) -> Result<(), String> {
    let package = RenderPackage::create(&PackageCreateOptions {
        project: options.project.clone(),
        audio: options.audio.clone(),
        output: options.output.clone(),
        width: options.width,
        height: options.height,
        assets: options.assets.clone(),
    })
    .map_err(|error| error.to_string())?;
    println!(
        "Created and validated render package {} ({}x{})",
        options.output.display(),
        package.render.width,
        package.render.height
    );
    Ok(())
}

fn run_package_validate(path: &PathBuf) -> Result<(), String> {
    let package = RenderPackage::load(path).map_err(|error| error.to_string())?;
    println!(
        "Valid render package {} (project {}, audio {})",
        path.display(),
        package.project_path().display(),
        package.audio_path().display()
    );
    Ok(())
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
            project.apply_analysis_profile(*features),
            delta,
        );
    }
    if active.is_empty() {
        active = evaluate_mappings(
            &project.modulation_mappings,
            &mut smoothers,
            project.apply_analysis_profile(analysis.sample_at(options.time_seconds)),
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
    let (project, audio, package_dimensions) =
        load_render_input(&options.project, options.audio.as_ref())?;
    let analysis =
        analyze_cached(&audio, AnalysisConfig::default()).map_err(|error| error.to_string())?;
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
            width: options.width.unwrap_or(
                package_dimensions.map_or(project.render_defaults.width, |value| value.0),
            ),
            height: options.height.unwrap_or(
                package_dimensions.map_or(project.render_defaults.height, |value| value.1),
            ),
            post_process: PostProcessConfig::for_quality(options.post_quality),
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

#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn run_video(context: &GpuContext, options: &VideoOptions) -> Result<(), String> {
    let (project, audio, package_dimensions) =
        load_render_input(&options.project, options.audio.as_ref())?;
    let analysis =
        analyze_cached(&audio, AnalysisConfig::default()).map_err(|error| error.to_string())?;
    let seconds = f64::from(project.duration_seconds).min(analysis.duration_seconds);
    let default_end = (seconds * f64::from(project.fps))
        .floor()
        .min(f64::from(u32::MAX)) as u32;
    let end_frame = options.frame_count.map_or(default_end, |count| {
        options.start_frame.saturating_add(count)
    });
    let cancellation = CancellationToken::default();
    let handler_token = cancellation.clone();
    ctrlc::set_handler(move || handler_token.cancel())
        .map_err(|error| format!("failed to install cancellation handler: {error}"))?;
    let report = export_video(
        context,
        &project,
        &analysis,
        &VideoExportConfig {
            ffmpeg_path: options.ffmpeg_path.clone(),
            audio_path: audio,
            output_path: options.output.clone(),
            width: options.width.unwrap_or(
                package_dimensions.map_or(project.render_defaults.width, |value| value.0),
            ),
            height: options.height.unwrap_or(
                package_dimensions.map_or(project.render_defaults.height, |value| value.1),
            ),
            output_width: options.width.unwrap_or(
                package_dimensions.map_or(project.render_defaults.width, |value| value.0),
            ),
            output_height: options.height.unwrap_or(
                package_dimensions.map_or(project.render_defaults.height, |value| value.1),
            ),
            motion_blur_samples: 1,
            start_frame: options.start_frame,
            end_frame,
            codec: options.codec,
            post_process: PostProcessConfig::for_quality(options.post_quality),
        },
        &cancellation,
        |progress| {
            if progress.completed_frames == progress.total_frames
                || progress.completed_frames % project.fps == 0
            {
                eprintln!(
                    "Exported {}/{} frames",
                    progress.completed_frames, progress.total_frames
                );
            }
        },
    )
    .map_err(|error| format!("video export failed: {error}"))?;
    println!(
        "Rendered {} frames with audio to {}",
        report.frames_rendered,
        options.output.display()
    );
    Ok(())
}

type LoadedRenderInput = (ProjectV1, PathBuf, Option<(u32, u32)>);

fn load_render_input(
    input: &PathBuf,
    audio_override: Option<&PathBuf>,
) -> Result<LoadedRenderInput, String> {
    if input.is_dir() {
        let package = RenderPackage::load(input).map_err(|error| error.to_string())?;
        if audio_override.is_some() {
            return Err("--audio cannot be used with a render package".into());
        }
        let audio = package.audio_path();
        let dimensions = Some((package.render.width, package.render.height));
        Ok((package.project, audio, dimensions))
    } else {
        let audio = audio_override.cloned().ok_or_else(|| {
            "--audio <path> is required when the input is a project JSON file".to_owned()
        })?;
        let project = ProjectV1::load(input).map_err(|error| error.to_string())?;
        Ok((project, audio, None))
    }
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

#[allow(clippy::cast_precision_loss)]
fn run_spatial_benchmark(
    context: &GpuContext,
    options: &SpatialBenchmarkOptions,
) -> Result<(), String> {
    let grid = SpatialGrid::new(
        context,
        options.count,
        1,
        SpatialGridConfig {
            cells_per_axis: options.cells_per_axis,
            cell_capacity: options.cell_capacity,
            max_neighbors: options.max_neighbors,
            ..SpatialGridConfig::default()
        },
    )
    .map_err(|error| error.to_string())?;
    println!(
        "iteration,particles,cells,occupied,max_occupancy,avg_neighbors,max_neighbors,overflow,memory_mib,elapsed_ms"
    );
    let mut last_cells = Vec::new();
    for iteration in 1..=options.iterations {
        let (stats, cells) = grid.run(context).map_err(|error| error.to_string())?;
        println!(
            "{iteration},{},{},{},{},{:.3},{},{},{:.2},{:.3}",
            stats.particle_count,
            stats.cell_count,
            stats.occupied_cells,
            stats.maximum_cell_occupancy,
            stats.average_neighbors,
            stats.maximum_neighbors,
            stats.overflowed_particles,
            stats.gpu_memory_bytes as f64 / 1_048_576.0,
            stats.elapsed_ms
        );
        last_cells = cells;
    }
    if let Some(path) = &options.debug_output {
        grid.save_debug_png(&last_cells, path)
            .map_err(|error| error.to_string())?;
        println!(
            "Saved projected cell-occupancy visualization to {}",
            path.display()
        );
    }
    Ok(())
}

#[allow(clippy::cast_precision_loss)]
fn run_fluid(context: &GpuContext, options: &FluidOptions) -> Result<(), String> {
    let target = OffscreenRenderTarget::new(context, options.width, options.height)
        .map_err(|error| format!("failed to create fluid target: {error}"))?;
    let mut fluid = FluidRenderer::new(
        context,
        options.count,
        1,
        FluidConfig {
            cells_per_axis: options.cells,
            pressure: options.pressure,
            viscosity: options.viscosity,
            cohesion: options.cohesion,
            audio_pressure: options.audio_pressure,
            audio_turbulence: options.audio_turbulence,
            ..FluidConfig::default()
        },
        options.width,
        options.height,
    )
    .map_err(|error| format!("failed to create fluid simulation: {error}"))?;
    let stats = fluid
        .save_png(context, &target, options.frames, &options.output)
        .map_err(|error| format!("failed to render fluid: {error}"))?;
    println!(
        "Rendered {} SPH slime particles to {}: avg_density={:.3}, max_density={:.3}, memory={:.2} MiB, final_frame_ms={:.3}",
        stats.particle_count,
        options.output.display(),
        stats.average_density,
        stats.maximum_density,
        stats.gpu_memory_bytes as f64 / 1_048_576.0,
        stats.elapsed_ms,
    );
    Ok(())
}

fn run_target(context: &GpuContext, options: &TargetOptions) -> Result<(), String> {
    let points = match &options.source {
        TargetSource::Shape(shape) => primitive_points(*shape, options.count, options.seed),
        TargetSource::Mesh(path) => load_gltf_points(path).map_err(|error| error.to_string())?,
        TargetSource::Svg(path) => load_svg_points(path, 512).map_err(|error| error.to_string())?,
        TargetSource::Text { value, font } => {
            load_text_points(value, font, 96.0).map_err(|error| error.to_string())?
        }
    };
    let particles =
        particles_from_target(&points, options.count, options.seed, options.dissolution)
            .map_err(|error| error.to_string())?;
    let target = OffscreenRenderTarget::new(context, options.width, options.height)
        .map_err(|error| error.to_string())?;
    let mut renderer = ParticleRenderer::new_with_particles(context, &particles, options.seed)
        .map_err(|error| error.to_string())?;
    let pixels = renderer
        .render_frame(
            context,
            &target,
            0,
            60.0,
            RgbaColor::new(0.0, 0.0, 0.0, 0.0),
        )
        .map_err(|error| error.to_string())?;
    target
        .save_png(&pixels, &options.output)
        .map_err(|error| error.to_string())?;
    if options.passes {
        let directory = options
            .output
            .parent()
            .unwrap_or_else(|| std::path::Path::new("."));
        let stem = options
            .output
            .file_stem()
            .and_then(|value| value.to_str())
            .unwrap_or("target");
        save_render_passes(
            &pixels,
            None,
            options.width,
            options.height,
            directory,
            stem,
            &[
                RenderPass::Alpha,
                RenderPass::Depth,
                RenderPass::Normals,
                RenderPass::MotionVectors,
                RenderPass::Emission,
            ],
        )
        .map_err(|error| error.to_string())?;
    }
    if let Some(path) = &options.exr {
        save_exr(&pixels, options.width, options.height, path)
            .map_err(|error| error.to_string())?;
    }
    println!(
        "Rendered {} target particles to {}",
        options.count,
        options.output.display()
    );
    Ok(())
}

#[allow(clippy::cast_precision_loss, clippy::too_many_lines)]
fn render_project_still(
    context: &GpuContext,
    options: &StillOptions,
    project_path: &PathBuf,
) -> Result<(), String> {
    let (project, package_dimensions) = if project_path.is_dir() {
        let package = RenderPackage::load(project_path)
            .map_err(|error| format!("failed to load package: {error}"))?;
        let dimensions = Some((package.render.width, package.render.height));
        (package.project, dimensions)
    } else {
        (
            ProjectV1::load(project_path)
                .map_err(|error| format!("failed to load project: {error}"))?,
            None,
        )
    };
    let width = options
        .width
        .unwrap_or(package_dimensions.map_or(project.render_defaults.width, |value| value.0));
    let height = options
        .height
        .unwrap_or(package_dimensions.map_or(project.render_defaults.height, |value| value.1));
    let target = OffscreenRenderTarget::new_with_post_process(
        context,
        width,
        height,
        PostProcessConfig::for_quality(options.post_quality),
    )
    .map_err(|error| format!("failed to create offscreen target: {error}"))?;
    if !project.layers.is_empty() {
        return render_layered_still(
            context,
            options,
            project_path,
            &project,
            &target,
            width,
            height,
        );
    }
    if project.render_mode == RenderModeV1::LiquidChrome {
        let material = &project.liquid_chrome;
        let environment = material.environment.as_ref().map(|path| {
            project_path
                .parent()
                .unwrap_or_else(|| std::path::Path::new("."))
                .join(path)
        });
        let renderer = LiquidChromeRenderer::new(
            context,
            width,
            height,
            &LiquidChromeConfig {
                environment,
                roughness: material.roughness,
                reflection_intensity: material.reflection_intensity,
                metallic: material.metallic,
                surface_scale: material.surface_scale,
            },
        )
        .map_err(|error| format!("failed to create liquid chrome renderer: {error}"))?;
        let background = project.render_defaults.background;
        renderer
            .save_frame_png(
                context,
                &target,
                options.frame as f32 / project.fps as f32,
                material.roughness,
                material.reflection_intensity,
                material.surface_scale,
                RgbaColor::new(background[0], background[1], background[2], background[3]),
                &options.output,
            )
            .map_err(|error| format!("failed to render liquid chrome project still: {error}"))?;
        println!(
            "Rendered liquid chrome project {} frame {} at {}x{} to {}",
            project_path.display(),
            options.frame,
            width,
            height,
            options.output.display()
        );
        return Ok(());
    }
    if project.render_mode == RenderModeV1::WaterDroplets {
        let droplets = &project.water_droplets;
        let renderer = WaterDropletRenderer::new(
            context,
            width,
            height,
            WaterDropletConfig {
                seed: project.seed,
                density: droplets.density,
                size: droplets.size,
                size_variation: droplets.size_variation,
                refraction_strength: droplets.refraction_strength,
                fresnel_strength: droplets.fresnel_strength,
                gravity: droplets.gravity,
                emission: droplets.emission,
            },
        );
        let background = project.render_defaults.background;
        renderer
            .save_frame_png(
                context,
                &target,
                options.frame as f32 / project.fps as f32,
                0.0,
                RgbaColor::new(background[0], background[1], background[2], background[3]),
                &options.output,
            )
            .map_err(|error| format!("failed to render water droplet project still: {error}"))?;
        println!(
            "Rendered water droplet project {} frame {} at {}x{} to {}",
            project_path.display(),
            options.frame,
            width,
            height,
            options.output.display()
        );
        return Ok(());
    }
    if project.render_mode == RenderModeV1::Volumetric {
        let quality = match options.post_quality {
            PostProcessQuality::Draft => VolumetricQuality::Draft,
            PostProcessQuality::Preview => VolumetricQuality::Preview,
            PostProcessQuality::Final => VolumetricQuality::Final,
        };
        let renderer = VolumetricRenderer::new(
            context,
            project.particle_system.count,
            project.seed,
            VolumetricConfig::for_quality(quality),
        );
        let background = project.render_defaults.background;
        renderer
            .save_frame_png(
                context,
                &target,
                options.frame,
                project.fps,
                RgbaColor::new(background[0], background[1], background[2], background[3]),
                &options.output,
            )
            .map_err(|error| format!("failed to render volumetric project still: {error}"))?;
        println!(
            "Rendered volumetric project {} frame {} at {}x{} to {}",
            project_path.display(),
            options.frame,
            width,
            height,
            options.output.display()
        );
        return Ok(());
    }
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
                view_projection: Some(project_camera_matrix(
                    &project,
                    options.frame as f32 / project.fps as f32,
                    width,
                    height,
                )),
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

#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    clippy::cast_sign_loss,
    clippy::too_many_arguments
)]
fn render_layered_still(
    context: &GpuContext,
    options: &StillOptions,
    project_path: &std::path::Path,
    project: &ProjectV1,
    target: &OffscreenRenderTarget,
    width: u32,
    height: u32,
) -> Result<(), String> {
    let background = project.render_defaults.background;
    let mut composite = vec![0_u8; width as usize * height as usize * 4];
    for pixel in composite.chunks_exact_mut(4) {
        pixel.copy_from_slice(&[
            (background[0] * 255.0).round() as u8,
            (background[1] * 255.0).round() as u8,
            (background[2] * 255.0).round() as u8,
            (background[3] * 255.0).round() as u8,
        ]);
    }
    let mut layers: Vec<(usize, &SceneLayerV1)> = project.layers.iter().enumerate().collect();
    layers.sort_by_key(|(index, layer)| (layer.depth, *index));
    let mut rendered = 0;
    for (_, layer) in layers {
        if !layer.visible || layer.opacity == 0.0 {
            continue;
        }
        let pixels = render_scene_layer(context, options, project, layer, target, width, height)?;
        let blend = match layer.blend {
            LayerBlendModeV1::Alpha => LayerBlendMode::Alpha,
            LayerBlendModeV1::Add => LayerBlendMode::Add,
            LayerBlendModeV1::Screen => LayerBlendMode::Screen,
        };
        composite_rgba8(&mut composite, &pixels, blend, layer.opacity);
        rendered += 1;
    }
    target
        .save_png(&composite, &options.output)
        .map_err(|error| format!("failed to save layered still: {error}"))?;
    println!(
        "Rendered {rendered} visible layers from {} frame {} at {}x{} to {}",
        project_path.display(),
        options.frame,
        width,
        height,
        options.output.display()
    );
    Ok(())
}

#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    clippy::cast_sign_loss,
    clippy::too_many_arguments
)]
fn render_scene_layer(
    context: &GpuContext,
    options: &StillOptions,
    project: &ProjectV1,
    layer: &SceneLayerV1,
    target: &OffscreenRenderTarget,
    width: u32,
    height: u32,
) -> Result<Vec<u8>, String> {
    let clear = RgbaColor::new(0.0, 0.0, 0.0, 1.0);
    let scaled_count = ((f64::from(layer.particle_system.count) * f64::from(layer.quality_scale))
        .round()
        .clamp(1.0, f64::from(u32::MAX))) as u32;
    match layer.render_mode {
        RenderModeV1::Volumetric => {
            let quality = match options.post_quality {
                PostProcessQuality::Draft => VolumetricQuality::Draft,
                PostProcessQuality::Preview => VolumetricQuality::Preview,
                PostProcessQuality::Final => VolumetricQuality::Final,
            };
            VolumetricRenderer::new(
                context,
                scaled_count,
                project.seed ^ hash_name(&layer.name),
                VolumetricConfig::for_quality(quality),
            )
            .render_frame(context, target, options.frame, project.fps, clear)
            .map_err(|error| format!("failed to render layer '{}': {error}", layer.name))
        }
        RenderModeV1::WaterDroplets => {
            let d = &layer.water_droplets;
            WaterDropletRenderer::new(
                context,
                width,
                height,
                WaterDropletConfig {
                    seed: project.seed ^ hash_name(&layer.name),
                    density: d.density,
                    size: d.size,
                    size_variation: d.size_variation,
                    refraction_strength: d.refraction_strength,
                    fresnel_strength: d.fresnel_strength,
                    gravity: d.gravity,
                    emission: d.emission,
                },
            )
            .render_frame(
                context,
                target,
                options.frame as f32 / project.fps as f32,
                0.0,
                clear,
            )
            .map_err(|error| format!("failed to render layer '{}': {error}", layer.name))
        }
        RenderModeV1::Particles => {
            let mut renderer =
                ParticleRenderer::new(context, scaled_count, project.seed ^ hash_name(&layer.name))
                    .map_err(|error| format!("failed to create layer '{}': {error}", layer.name))?;
            renderer
                .set_forces(context, &layer.forces)
                .map_err(|error| format!("failed to configure layer '{}': {error}", layer.name))?;
            let fps = NonZeroU32::new(project.fps).ok_or("project FPS must be positive")?;
            let substeps = NonZeroU32::new(layer.particle_system.substeps)
                .ok_or("layer substeps must be positive")?;
            renderer
                .render_timeline_frame(
                    context,
                    target,
                    options.frame,
                    SimulationTiming::new(fps, fps, substeps),
                    BenchmarkConfig {
                        particle_size_pixels: project.render_defaults.particle_size_pixels,
                        view_projection: Some(project_camera_matrix(
                            project,
                            options.frame as f32 / project.fps as f32,
                            width,
                            height,
                        )),
                        ..BenchmarkConfig::default()
                    },
                    clear,
                )
                .map_err(|error| format!("failed to render layer '{}': {error}", layer.name))
        }
        RenderModeV1::LiquidChrome => Err(format!(
            "layer '{}' uses liquid_chrome, which cannot yet be composited without a per-layer environment path",
            layer.name
        )),
    }
}

fn hash_name(name: &str) -> u64 {
    name.bytes().fold(0xcbf2_9ce4_8422_2325, |hash, byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x100_0000_01b3)
    })
}

#[allow(clippy::cast_precision_loss)]
fn project_camera_matrix(
    project: &ProjectV1,
    time_seconds: f32,
    width: u32,
    height: u32,
) -> [[f32; 4]; 4] {
    let camera = project.camera.sample(time_seconds, 0.0, 0.0, project.seed);
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

fn parse_video_codec(value: &str) -> Result<VideoCodec, String> {
    match value {
        "h264" => Ok(VideoCodec::H264),
        "hevc" | "h265" => Ok(VideoCodec::Hevc),
        "prores422hq" => Ok(VideoCodec::ProRes422Hq),
        "prores4444" => Ok(VideoCodec::ProRes4444),
        _ => Err(format!(
            "unsupported codec '{value}'; expected h264, hevc, prores422hq, or prores4444"
        )),
    }
}

fn parse_post_quality(value: &str) -> Result<PostProcessQuality, String> {
    match value {
        "draft" => Ok(PostProcessQuality::Draft),
        "preview" => Ok(PostProcessQuality::Preview),
        "final" => Ok(PostProcessQuality::Final),
        _ => Err(format!(
            "unsupported post quality '{value}'; expected draft, preview, or final"
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
         [--width 1920] [--height 1080] [--color RRGGBB[AA]] [--post-quality preview] \
         [--backend auto|dx12|vulkan]\n\
         particle-render particles --output <path> [--count 10000] [--seed 1] \
         [--frame 0] [--fps 60] [--substeps 1] [--motion none|orbit|swirl] \
         [--width 1920] [--height 1080] \
         [--backend auto|dx12|vulkan]\n\
         particle-render target (--shape sphere|cube|ring | --mesh model.glb | --svg art.svg | \
         --text TEXT --font font.ttf) --output target.png [--count 100000] [--dissolution 0..1] \
         [--passes] [--exr target.exr] [--width 1920] [--height 1080]\n\
         particle-render benchmark [--count N] [--frames 10] [--width 1920] \
         [--height 1080] [--particle-size 2] [--overdraw] [--readback] \
         [--backend auto|dx12|vulkan]\n\
         particle-render spatial-benchmark [--count 100000] [--cells 32] \
         [--cell-capacity 64] [--max-neighbors 128] [--iterations 3] \
         [--debug-output grid.png] [--backend auto|dx12|vulkan]\n\
         particle-render fluid --output slime.png [--count 100000] [--frames 60] \
         [--width 1920] [--height 1080] [--cells 32] [--pressure 0.45] \
         [--viscosity 0.18] [--cohesion 0.35] [--audio-pressure 0] \
         [--audio-turbulence 0] [--backend auto|dx12|vulkan]\n\
         particle-render audio-info <audio-path> [--time seconds]\n\
         particle-render modulation-info <project.json> <audio-path> [--time seconds]\n\
         particle-render package-create <project.json> --audio <path> --output <name.rustiqueproject> \
         [--width W] [--height H] [--texture <path>] [--hdri <path>] [--mesh <path>]\n\
         particle-render package-validate <name.rustiqueproject>\n\
         particle-render sequence <project.json|package.rustiqueproject> [--audio <path>] --output-dir <path> \
         [--start-frame 0] [--frames N] [--width W] [--height H] [--post-quality preview]\n\
         particle-render video <project.json|package.rustiqueproject> [--audio <path>] --output <path> \
         [--codec h264|hevc|prores422hq|prores4444] [--start-frame 0] \
         [--frames N] [--width W] [--height H] [--ffmpeg <path>] [--post-quality preview]"
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
                post_quality: PostProcessQuality::Preview,
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
    fn parses_video_export_options() {
        let options = CliOptions::parse(
            [
                "video",
                "project.json",
                "--audio",
                "track.wav",
                "--output",
                "render.mov",
                "--codec",
                "prores4444",
                "--frames",
                "12",
            ]
            .into_iter()
            .map(str::to_owned),
        )
        .unwrap();
        assert!(matches!(
            options.command,
            Some(Command::Video(VideoOptions {
                codec: VideoCodec::ProRes4444,
                frame_count: Some(12),
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
    fn parses_package_creation_assets() {
        let options = CliOptions::parse(
            [
                "package-create",
                "project.json",
                "--audio",
                "track.wav",
                "--output",
                "job.rustiqueproject",
                "--texture",
                "mask.png",
                "--mesh",
                "object.glb",
            ]
            .into_iter()
            .map(str::to_owned),
        )
        .unwrap();
        assert!(matches!(
            options.command,
            Some(Command::PackageCreate(PackageCreateCliOptions { assets, .. }))
                if assets.len() == 2
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

    #[test]
    fn parses_spatial_benchmark_options() {
        let options = CliOptions::parse(
            [
                "spatial-benchmark",
                "--count",
                "500000",
                "--cells",
                "48",
                "--debug-output",
                "grid.png",
            ]
            .into_iter()
            .map(str::to_owned),
        )
        .unwrap();
        assert!(matches!(
            options.command,
            Some(Command::SpatialBenchmark(SpatialBenchmarkOptions {
                count: 500_000,
                cells_per_axis: 48,
                debug_output: Some(_),
                ..
            }))
        ));
    }

    #[test]
    fn parses_fluid_options() {
        let options = CliOptions::parse(
            [
                "fluid",
                "--output",
                "slime.png",
                "--count",
                "500000",
                "--audio-pressure",
                "0.8",
            ]
            .into_iter()
            .map(str::to_owned),
        )
        .unwrap();
        assert!(matches!(
            options.command,
            Some(Command::Fluid(FluidOptions {
                count: 500_000,
                audio_pressure: 0.8,
                ..
            }))
        ));
    }

    #[test]
    fn parses_shape_target_with_passes_and_exr() {
        let options = CliOptions::parse(
            [
                "target",
                "--shape",
                "sphere",
                "--output",
                "shape.png",
                "--dissolution",
                "0.4",
                "--passes",
                "--exr",
                "shape.exr",
            ]
            .into_iter()
            .map(str::to_owned),
        )
        .unwrap();
        assert!(matches!(
            options.command,
            Some(Command::Target(TargetOptions {
                source: TargetSource::Shape(PrimitiveTarget::Sphere),
                passes: true,
                exr: Some(_),
                ..
            }))
        ));
    }
}
