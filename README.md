# Rustique

Rustique is a headless-first, audio-reactive particle renderer written in Rust. The repository is currently in its foundation phase; renderer, simulation, audio, project-format, and export concerns are kept in separate workspace crates.

See [`DESIGN.md`](DESIGN.md) for architecture and product intent and [`PLAN.md`](PLAN.md) for implementation progress.

## Prerequisites

- Rust 1.85 or newer, installed through [rustup](https://rustup.rs/)
- The `rustfmt` and `clippy` components
- FFmpeg and FFprobe on `PATH` for video export

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

Render a deterministic, compute-simulated particle frame (the simulation and
particle buffers remain GPU-resident; only the final image is read back):

```bash
cargo run -p particle-render --release -- particles --output particles.png --count 1000000 --frame 60 --fps 60 --seed 42
```

Use `--substeps N` to select deterministic fixed simulation substeps. Arbitrary
frame requests replay from frame zero; see [`docs/DETERMINISM.md`](docs/DETERMINISM.md).

Try the data-driven force stacks with `--motion orbit` or `--motion swirl`.

Render a versioned project file with its saved seed, forces, timing, and render defaults:

```bash
cargo run -p particle-render --release -- still examples/star-orbit.rustique.json --output frame.png --frame 120
```

Run the reproducible RTX scale benchmark (or select one preset with `--count`):

```bash
cargo run -p particle-render --release -- benchmark --frames 20
```

See [`docs/BENCHMARKS.md`](docs/BENCHMARKS.md) for recorded baseline results and
the `--particle-size`, `--overdraw`, and `--readback` stress-test options.

Analyze and inspect cached audio features:

```bash
cargo run -p particle-render --release -- audio-info track.flac --time 12.5
```

See [`docs/AUDIO.md`](docs/AUDIO.md) for formats, features, and cache behavior.

Inspect a project's smoothed audio mappings with `modulation-info`; mapping
semantics are documented in [`docs/MODULATION.md`](docs/MODULATION.md).

Render an audio-reactive PNG sequence without a window:

```bash
cargo run -p particle-render --release -- sequence examples/star-orbit.rustique.json --audio track.wav --output-dir frames
```

Stream an audio-reactive render directly to FFmpeg:

```bash
cargo run -p particle-render --release -- video examples/star-orbit.rustique.json --audio track.wav --output render.mp4 --codec h264
```

The supported codec names are `h264`, `hevc`, `prores422hq`, and
`prores4444`. Use `--start-frame` and `--frames` for a timeline slice. The
exporter writes an adjacent partial file and removes it on failure or Ctrl+C.

Select the shared HDR/post-processing pipeline with
`--post-quality draft|preview|final`. See
[`docs/POST_PROCESSING.md`](docs/POST_PROCESSING.md) for effects and GPU memory
usage.

Perspective, orbit, dolly, drift, and audio-reactive camera settings are stored
in project data. See [`docs/CAMERA.md`](docs/CAMERA.md).

Versioned visual presets and project-local macro overrides are documented in
[`docs/PRESETS.md`](docs/PRESETS.md).

The future desktop editor belongs under `apps/desktop`; the render core and CLI do not depend on Tauri or a window.
