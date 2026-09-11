use std::{env, path::PathBuf, process::ExitCode};

use render_core::{BackendPreference, GpuConfig, GpuContext, OffscreenRenderTarget, RgbaColor};

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
}

#[derive(Debug, PartialEq)]
struct StillOptions {
    output: PathBuf,
    width: u32,
    height: u32,
    color: RgbaColor,
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
         [--color RRGGBB[AA]] [--backend auto|dx12|vulkan]"
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
}
