use std::{env, path::PathBuf, process::ExitCode, time::Instant};

use render_core::{
    BackendPreference, BenchmarkConfig, GpuConfig, GpuContext, OffscreenRenderTarget,
    ParticleRenderer, RgbaColor,
};

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
            let target = OffscreenRenderTarget::new(&context, still.width, still.height)
                .map_err(|error| format!("failed to create offscreen target: {error}"))?;
            target
                .save_clear_png(&context, still.color, &still.output)
                .map_err(|error| format!("failed to render still: {error}"))?;
            println!(
                "Rendered {}x{} still to {}",
                still.width,
                still.height,
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
                .save_frame_png(
                    &context,
                    &target,
                    particles.frame,
                    particles.fps,
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
}

#[derive(Debug, PartialEq)]
struct StillOptions {
    output: PathBuf,
    width: u32,
    height: u32,
    color: RgbaColor,
}

#[derive(Debug, PartialEq)]
struct ParticleOptions {
    output: PathBuf,
    width: u32,
    height: u32,
    count: u32,
    seed: u64,
    frame: u32,
    fps: f32,
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
        let mut width = 1920;
        let mut height = 1080;
        let mut color = RgbaColor::new(0.02, 0.04, 0.12, 1.0);
        while let Some(argument) = args.next() {
            match argument.as_str() {
                "--output" => {
                    output = Some(PathBuf::from(required_value(args, "--output")?));
                }
                "--width" => width = parse_dimension("width", &required_value(args, "--width")?)?,
                "--height" => {
                    height = parse_dimension("height", &required_value(args, "--height")?)?;
                }
                "--color" => color = parse_color(&required_value(args, "--color")?)?,
                "--backend" => *backend = parse_backend(&required_value(args, "--backend")?)?,
                _ => return Err(format!("unknown still argument: {argument}")),
            }
        }
        Ok(Self {
            output: output.ok_or_else(|| "still requires --output <path>".to_owned())?,
            width,
            height,
            color,
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
        let mut fps = 60.0;
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
                "--fps" => {
                    fps = required_value(args, "--fps")?
                        .parse::<f32>()
                        .map_err(|_| "fps must be a positive number".to_owned())?;
                    if !fps.is_finite() || fps <= 0.0 {
                        return Err("fps must be a positive finite number".to_owned());
                    }
                }
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
         particle-render still --output <path> [--width 1920] [--height 1080] \
         [--color RRGGBB[AA]] [--backend auto|dx12|vulkan]\n\
         particle-render particles --output <path> [--count 10000] [--seed 1] \
         [--frame 0] [--fps 60] [--width 1920] [--height 1080] \
         [--backend auto|dx12|vulkan]\n\
         particle-render benchmark [--count N] [--frames 10] [--width 1920] \
         [--height 1080] [--particle-size 2] [--overdraw] [--readback] \
         [--backend auto|dx12|vulkan]"
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
                output: PathBuf::from("frame.png"),
                width: 13,
                height: 7,
                color: RgbaColor::new(1.0, 128.0 / 255.0, 0.0, 64.0 / 255.0),
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
