use std::{env, process::ExitCode};

use render_core::{BackendPreference, GpuConfig, GpuContext};

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
    if !options.gpu_info {
        return Err("no command specified; use --gpu-info or --help".into());
    }

    let context = pollster::block_on(GpuContext::new(GpuConfig {
        backend: options.backend,
    }))
    .map_err(|error| format!("GPU initialization failed: {error}"))?;
    println!("{}", context.info());
    Ok(())
}

#[derive(Debug, Default, Eq, PartialEq)]
struct CliOptions {
    gpu_info: bool,
    help: bool,
    backend: BackendPreference,
}

impl CliOptions {
    fn parse(mut args: impl Iterator<Item = String>) -> Result<Self, String> {
        let mut options = Self::default();
        while let Some(argument) = args.next() {
            match argument.as_str() {
                "--gpu-info" => options.gpu_info = true,
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
    println!("particle-render --gpu-info [--backend auto|dx12|vulkan]");
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
        assert_eq!(
            options,
            CliOptions {
                gpu_info: true,
                help: false,
                backend: BackendPreference::Vulkan,
            }
        );
    }

    #[test]
    fn rejects_unknown_backend() {
        let error =
            CliOptions::parse(["--backend", "metal"].into_iter().map(str::to_owned)).unwrap_err();
        assert!(error.contains("unsupported backend"));
    }
}
