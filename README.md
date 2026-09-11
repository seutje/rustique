# Rustique

Rustique is a headless-first, audio-reactive particle renderer written in Rust. The repository is currently in its foundation phase; renderer, simulation, audio, project-format, and export concerns are kept in separate workspace crates.

See [`DESIGN.md`](DESIGN.md) for architecture and product intent and [`PLAN.md`](PLAN.md) for implementation progress.

## Prerequisites

- Rust 1.85 or newer, installed through [rustup](https://rustup.rs/)
- The `rustfmt` and `clippy` components

Install the required components with:

```powershell
rustup component add rustfmt clippy
```

## Workspace checks

Run the baseline checks from the repository root:

```powershell
cargo fmt --check
cargo check --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

Print information about the preferred high-performance GPU with:

```powershell
cargo run -p particle-render -- --gpu-info
```

Render a headless still image (the color accepts `RRGGBB` or `RRGGBBAA`):

```bash
cargo run -p particle-render -- still --output frame.png --width 1920 --height 1080 --color 204080
```

To force a supported backend, add `--backend dx12` or `--backend vulkan`.

The future desktop editor belongs under `apps/desktop`; the render core and CLI do not depend on Tauri or a window.
