# PLAN.md — Rustique Implementation Plan

Rustique is a personal, GPU-heavy, audio-reactive particle renderer for creating music visuals.

This plan is intended to be actively maintained by coding agents and humans.

## Checklist Conventions

- `[ ]` not started
- `[-]` in progress
- `[x]` completed
- `[!]` blocked
- `[~]` intentionally deferred

When completing a task, update the checkbox and add a short completion note directly below it if useful.

Do not mark a phase complete until its acceptance criteria pass.

---

# Phase 0 — Baseline Repository and Tooling

Status: GPU smoke test already works.

## Goals

Establish the Rustique repository structure, project conventions, formatting, linting, and a clean baseline before feature work.

## Tasks

- [x] Rename / initialize the main repository as `rustique`
- [x] Add `DESIGN.md` to the repository root
- [x] Add `PLAN.md` to the repository root
- [x] Add `AGENTS.md` to the repository root
- [x] Create a Cargo workspace
- [x] Create the initial workspace crates:
  - [x] `render-core`
  - [x] `simulation`
  - [x] `audio-engine`
  - [x] `project-format`
  - [x] `exporter`
  - [x] `particle-render`
- [x] Add a placeholder `apps/desktop` directory for the future Tauri app
- [x] Add a top-level `shaders/` directory
- [x] Add a top-level `presets/` directory
- [x] Configure `.gitignore`
- [x] Configure `rustfmt`
- [x] Configure Clippy usage
- [x] Add basic workspace build command documentation to `README.md`
- [x] Add a minimal CI workflow for:
  - [x] `cargo fmt --check`
  - [x] `cargo check --workspace`
  - [x] `cargo clippy --workspace --all-targets -- -D warnings`

## Acceptance Criteria

- [x] `cargo check --workspace` succeeds
- [x] `cargo fmt --check` succeeds
- [x] `cargo clippy --workspace --all-targets -- -D warnings` succeeds
- [x] Existing GPU smoke test still runs successfully
- [x] No Tauri code is required yet

---

# Phase 1 — Render Core Foundation

## Goals

Turn the GPU smoke test into a reusable render-core library that owns wgpu initialization and device capabilities.

## Tasks

- [x] Move GPU initialization into `render-core`
  - Replaced the placeholder smoke-test path with reusable headless initialization.
- [x] Add explicit adapter selection preferring high-performance discrete GPUs
- [x] Log:
  - [x] adapter name
  - [x] backend
  - [x] device type
  - [x] driver info where available
  - [x] relevant wgpu limits
  - [x] relevant wgpu features
- [x] Add a `GpuContext` abstraction containing:
  - [x] instance
  - [x] adapter
  - [x] device
  - [x] queue
- [x] Make GPU initialization work without creating a window
- [x] Add structured error handling
- [x] Add unit-testable configuration types for backend preference
- [x] Add a CLI option in `particle-render` to print GPU info and exit
- [x] Add backend override support where practical:
  - [x] DX12
  - [x] Vulkan
  - [x] Honor `WGPU_BACKEND` through wgpu's environment-aware backend selection
    - GPU initialization logs the requested backend set and selected adapter backend.
  - [x] Honor wgpu instance flag environment overrides without enabling noncompliant adapters by default
    - `--gpu-info` reports the resolved instance flags, including opt-in `WGPU_ALLOW_UNDERLYING_NONCOMPLIANT_ADAPTER`.

## Acceptance Criteria

- [x] `particle-render --gpu-info` prints useful adapter data
- [x] GPU initialization works headlessly
- [x] No Tauri dependency exists in `render-core`
- [x] Render core compiles on Windows
- [x] Code is ready to compile on Linux without Windows-only dependencies
  - Uses only cross-platform wgpu APIs; Vulkan can be selected explicitly.

---

# Phase 2 — Offscreen Rendering

## Goals

Render a frame without a window and save it to disk.

## Tasks

- [x] Add an offscreen render target abstraction
- [x] Render a clear color into a texture
- [x] Add GPU-to-CPU texture readback
- [x] Handle row padding correctly
- [x] Save output as PNG
- [x] Add CLI command:
  - [x] `particle-render still --output frame.png`
- [x] Add render width / height arguments
- [x] Add a basic color parameter to validate project-to-render plumbing
- [x] Reuse GPU resources between repeated renders where possible
  - `OffscreenRenderTarget` retains its texture and staging buffer for reuse at a fixed resolution.

## Acceptance Criteria

- [x] CLI renders a valid PNG
- [x] Arbitrary dimensions work
- [x] 1920x1080 renders successfully
- [x] 3840x2160 renders successfully
- [x] No window is created
- [x] Repeated still renders do not leak memory
  - Rendering reuses owned GPU resources and balances every buffer map with an unmap.

---

# Phase 3 — First GPU Particle System

## Goals

Create the first compute-driven particle simulation.

## Initial Particle Layout

Start with a 64-byte particle:

```rust
#[repr(C)]
struct Particle {
    position_age: [f32; 4],
    velocity_lifetime: [f32; 4],
    color: [f32; 4],
    params: [f32; 4],
}
```

## Tasks

- [x] Define the Rust particle structure
- [x] Define the matching WGSL particle structure
- [x] Document memory layout assumptions
- [x] Add compile-time size assertions where practical
- [x] Allocate GPU storage buffers
- [x] Implement ping-pong simulation buffers
- [x] Write compute shader for:
  - [x] velocity integration
  - [x] position integration
  - [x] lifetime update
- [x] Initialize deterministic particle state from a seed
- [x] Populate and simulate particle position and velocity across all three spatial axes
- [x] Render particles as points or camera-facing quads
- [x] Add basic camera matrices
- [x] Add frame uniforms:
  - [x] frame index
  - [x] delta time
  - [x] simulation time
  - [x] particle count
- [x] Render 10,000 particles
- [x] Render 100,000 particles
- [x] Render 1,000,000 particles

## Acceptance Criteria

- [x] 10k particles simulate entirely on GPU
- [x] 100k particles render correctly
- [x] 1M particles render correctly
- [x] Particle state is not read back to CPU during normal simulation
- [x] Simulation is deterministic for a fixed seed and frame sequence
  - Repeated 100k-particle renders produced identical SHA-256 image hashes.

---

# Phase 4 — Performance Instrumentation and Scale Testing

## Goals

Establish actual RTX 4070 performance characteristics early.

## Tasks

- [x] Add frame timing instrumentation
- [x] Measure CPU frame preparation time
- [x] Measure GPU compute timing where supported
- [x] Measure GPU render timing where supported
- [x] Log particle buffer memory size
- [x] Add benchmark presets:
  - [x] 100k
  - [x] 500k
  - [x] 1M
  - [x] 2M
  - [x] 5M
  - [x] 10M
  - [x] 20M
- [x] Add benchmark mode to CLI
- [x] Record first benchmark results in `docs/BENCHMARKS.md`
- [x] Identify whether current bottleneck is:
  - [x] compute
  - [x] vertex processing
  - [x] fragment overdraw
  - [x] memory bandwidth
  - [x] readback
- [x] Add simple overdraw stress test
- [x] Add particle-size scaling test

## Acceptance Criteria

- [x] Reproducible benchmark command exists
- [x] Baseline RTX 4070 results are documented
- [x] At least 1M particles can be interactively simulated
- [x] Offline high-count limits are understood well enough to guide later phases

---

# Phase 5 — Deterministic Simulation Framework

## Goals

Make the simulation reproducible and independent from wall-clock time.

## Tasks

- [x] Define fixed timestep behavior
- [x] Define project FPS separately from preview FPS
- [x] Make simulation time derive from frame index
- [x] Add deterministic seeded initialization
- [x] Add deterministic respawning
- [x] Avoid platform-dependent random initialization
- [x] Add simulation substep configuration
- [x] Add reset-to-frame-zero behavior
- [x] Add deterministic seek strategy for early versions
- [x] Add determinism regression test for CPU-side generated initialization data
- [x] Document practical GPU floating-point determinism limits

## Acceptance Criteria

- [x] Rendering frames 0–300 twice produces visually equivalent output
  - Repeated frame-300 renders produced identical SHA-256 image hashes.
- [x] Output does not depend on wall-clock speed
- [x] Preview resolution does not alter particle motion
- [x] Changing render resolution does not change simulation state

---

# Phase 6 — Force and Motion System

## Goals

Build a reusable library of GPU force passes.

## Tasks

- [x] Add gravity force
- [x] Add point attractor
- [x] Add point repulsor
- [x] Add vortex / orbital force
- [x] Add drag
- [x] Add bounded box or sphere constraint
- [x] Add noise-based directional field
- [x] Add curl-noise approximation
- [x] Add emitter abstraction
- [x] Add burst emitter
- [x] Add continuous emitter
- [x] Add respawn policies
- [x] Make force parameters serializable
- [x] Allow multiple force passes in a system

## Acceptance Criteria

- [x] A star-orbit style simulation is possible
- [x] A swirling field simulation is possible
- [x] Force parameters are data-driven
- [x] No force requires UI-specific code

---

# Phase 7 — Project Format v1

## Goals

Serialize complete creative state into a versioned project.

## Tasks

- [x] Define `ProjectV1`
- [x] Include:
  - [x] project version
  - [x] engine version
  - [x] seed
  - [x] FPS
  - [x] duration
  - [x] particle system
  - [x] force parameters
  - [x] camera
  - [x] render defaults
- [x] Use serde
- [x] Add load/save helpers
- [x] Add schema validation where practical
- [x] Add forward-compatible version field
- [x] Add sample projects under `examples/`
- [x] Add CLI loading:
  - [x] `particle-render still project.json ...`

## Acceptance Criteria

- [x] A saved project reproduces the same scene
- [x] Invalid projects return useful errors
- [x] Project schema is versioned from day one

---

# Phase 8 — Audio Decode and Analysis v1

## Goals

Load a full track and generate reusable audio features.

## Tasks

- [x] Choose native audio decoding crate(s)
- [x] Support WAV
- [x] Support FLAC
- [x] Add optional MP3/AAC support if straightforward
- [x] Decode audio into analysis-friendly samples
- [x] Calculate waveform summary
- [x] Calculate RMS / loudness envelope
- [x] Calculate FFT windows
- [x] Calculate bands:
  - [x] sub
  - [x] bass
  - [x] low mids
  - [x] mids
  - [x] high mids
  - [x] highs
- [x] Calculate spectral centroid
- [x] Calculate spectral flux
- [x] Calculate basic transient strength
- [x] Normalize features
- [x] Cache analysis to disk
- [x] Add deterministic feature sampling by timestamp
- [x] Add CLI command to inspect audio features

## Acceptance Criteria

- [x] Audio file can be analyzed once and cached
- [x] Renderer can sample audio features at an arbitrary frame time
- [x] Re-rendering does not re-run expensive analysis unless cache is stale

---

# Phase 9 — Universal Modulation System v1

## Goals

Make visual parameters react to audio features in a reusable way.

## Tasks

- [x] Define modulation source enum
- [x] Define modulation target identifiers
- [x] Define mapping structure:
  - [x] source
  - [x] target
  - [x] amount
  - [x] offset
  - [x] min
  - [x] max
  - [x] polarity
  - [x] curve
  - [x] attack
  - [x] release
- [x] Implement envelope smoothing
- [x] Implement linear curve
- [x] Implement exponential curve
- [x] Implement inverted mapping
- [x] Map bass to force strength
- [x] Map highs to brightness or particle size
- [x] Map transients to burst emission
- [x] Serialize mappings in project format
- [x] Add logging/debug output for active modulation values

## Acceptance Criteria

- [x] At least three visual parameters can react to audio
- [x] Mapping behavior is independent from preset implementation
- [x] Audio reactivity is deterministic during offline rendering

---

# Phase 10 — First End-to-End Music Render

## Goals

Produce the first real Rustique music visualization without a GUI.

## Tasks

- [x] Load project
- [x] Load audio
- [x] Load cached audio analysis
- [x] Simulate frame-by-frame
- [x] Apply modulation
- [x] Render frames offscreen
- [x] Save temporary PNG sequence first
- [x] Verify synchronization with audio
- [x] Render at:
  - [x] 1280x720
  - [x] 1920x1080
- [x] Create a short 10–30 second test project
  - `examples/star-orbit.rustique.json` is a 10-second deterministic scene.

## Acceptance Criteria

- [x] A deterministic audio-reactive sequence can be rendered from CLI
  - Two independent validation sequences produced identical per-frame hashes.
- [x] Visual reactions line up with audio events
- [x] Rendering can proceed slower than realtime

---

# Phase 11 — FFmpeg Video Export

## Goals

Stream frames directly into FFmpeg and create playable video files.

## Tasks

- [x] Add FFmpeg process wrapper
- [x] Stream raw frames through stdin
- [x] Avoid storing full sequence in memory
- [x] Mux original audio into final output
- [x] Add H.264 output
- [x] Add HEVC output
- [x] Add ProRes 422 HQ output
- [x] Add alpha-capable output path if practical
  - ProRes 4444 uses an alpha-capable `yuva444p10le` output pixel format.
- [x] Add export progress reporting
- [x] Handle FFmpeg errors cleanly
- [x] Add cancellation support
- [x] Clean up failed temporary files

## Acceptance Criteria

- [x] CLI produces a valid video with synchronized audio
  - FFprobe verified aligned zero start times and a one-second H.264 container with both streams.
- [x] Memory use does not grow with video duration
  - Each readback buffer is written and dropped before rendering the next frame.
- [x] 4K frame streaming works
  - A 3840x2160 H.264 frame was rendered and encoded successfully.
- [x] Failed encodes produce useful errors
  - Encoder exit details are reported and partial outputs are removed.

---

# Phase 12 — Post-Processing Foundation

## Goals

Make simple particle scenes visually rich.

## Tasks

- [x] Add HDR scene target
- [x] Add tone mapping
- [x] Add bloom
- [x] Add exposure
- [x] Add gamma/output transform
- [x] Add trails / temporal accumulation
- [x] Add optional vignette
- [x] Add optional chromatic aberration
- [x] Add render-target pooling or explicit reuse
  - Scene, history, output, and readback resources are owned by and reused with the offscreen target.
- [x] Add quality presets for post-processing
  - Draft, Preview, and Final presets are available through the render core and CLI.

## Acceptance Criteria

- [x] Bloom works at 4K
  - A 3840x2160 project frame rendered successfully through the HDR/bloom pipeline.
- [x] HDR pipeline does not cause uncontrolled VRAM growth
  - Fixed target-owned textures use about 221.5 MiB at 4K and are reused every frame.
- [x] Post effects can be toggled independently
- [x] Preview and offline renderer use the same post pipeline
  - Quality presets and offline exporters all construct the same render-core offscreen target.

---

# Phase 13 — Camera System v1

## Goals

Add reusable procedural camera behaviors.

## Tasks

- [x] Add perspective camera parameters
- [x] Add static camera
- [x] Add orbit camera
- [x] Add look-at target
- [x] Add dolly / distance control
- [x] Add procedural drift
- [x] Add FOV modulation
- [x] Add audio-reactive camera shake
- [x] Serialize camera settings
- [x] Keep camera independent from render resolution

## Acceptance Criteria

- [x] Camera settings load from project data
- [x] Camera can react to modulation
  - `camera_fov` and `camera_shake` targets use the same smoothed mapping pipeline as particle parameters.
- [x] Star-system scenes can be navigated cinematically
  - The example project now combines orbit, drift, FOV response, and transient shake.

---

# Phase 14 — Visual Preset Framework

## Goals

Make visual systems data-driven and composable.

## Tasks

- [x] Define visual preset schema
- [x] Define macro parameter schema
- [x] Define preset defaults
- [x] Define default modulation recommendations
- [x] Load presets from files
- [x] Add preset override support
- [x] Implement first visual presets:
  - [x] Star System
  - [x] Nebula
  - [x] Liquid Chrome prototype
  - [x] Green Slime prototype
  - [x] Water Droplets prototype
- [x] Ensure presets reuse common engine systems where possible

## Acceptance Criteria

- [x] Presets are not hardcoded into UI logic
- [x] At least three visually distinct presets work
  - Star System, Nebula, and Liquid Chrome rendered distinct validated frames from file-backed data.
- [x] Preset parameters can be overridden per project
  - Named, range-checked macro overrides are stored in each project's preset selection.

---

# Phase 15 — Audio Profile System

## Goals

Create reusable genre-aware analysis and reaction defaults.

## Tasks

- [x] Define analysis profile schema
- [x] Define reaction profile schema
- [x] Implement analysis profiles:
  - [x] Techno
  - [x] Drum & Bass
  - [x] Ambient
  - [x] Cinematic
- [x] Implement reaction profiles:
  - [x] Punchy
  - [x] Fluid
  - [x] Dreamy
  - [x] Aggressive
- [x] Add profile-level attack/release defaults
- [x] Add profile-level frequency weighting
- [x] Add recommended mappings
- [x] Allow project overrides

## Acceptance Criteria

- [x] Same visual preset behaves differently under at least two audio profiles
  - Techno and Ambient apply distinct deterministic weights to the same normalized feature sample.
- [x] Profiles remain editable data
  - Analysis and reaction profiles are versioned JSON files under `profiles/`.
- [x] Profile selection does not bypass the universal modulation system
  - Reaction recommendations resolve to the existing `ModulationMapping` pipeline.

---

# Phase 16 — Desktop App Scaffold

## Goals

Create the Tauri 2 + React + TypeScript editor.

## Tasks

- [x] Create Tauri 2 application
- [x] Use Vite
- [x] Use React
- [x] Use TypeScript
- [x] Add Zustand
- [x] Add application shell
- [x] Add viewport area
- [x] Add inspector area
- [x] Add preset browser area
- [x] Add audio/timeline area
- [x] Add render settings area
- [x] Keep UI state separate from engine state
- [x] Create minimal Rust command bridge
- [x] Do not duplicate renderer logic inside Tauri

## Acceptance Criteria

- [x] Desktop app launches
  - `npm run tauri -- dev` launched the Windows application successfully.
- [x] UI can query GPU info from native engine
  - The `gpu_info` command delegates adapter discovery to `render-core`.
- [x] UI can load a project
  - The `load_project` command validates through `project-format` and returns an editor summary.
- [x] Render core remains usable without Tauri
  - Tauri depends on `render-core`; no core crate depends on the desktop app.

---

# Phase 17 — Interactive Viewport

## Goals

Use the same engine for live preview.

## Tasks

- [x] Add window/surface rendering path
  - Windows uses a native child `HWND` with direct wgpu swapchain presentation.
- [x] Share pipelines with offscreen renderer
  - Preview uses the shared GPU simulation and HDR/post-processing target without CPU readback.
- [x] Add resize handling
- [x] Add preview quality configuration
- [x] Add play/pause
- [x] Add timeline scrub
- [x] Add reset
- [x] Add realtime audio-position sampling
- [x] Add preview FPS display
- [x] Add particle count display
- [-] Add GPU timing display where practical
  - The diagnostics contract and UI field exist, but surface timestamp queries are not yet wired; the UI reports `GPU — ms`.

## Acceptance Criteria

- [x] Interactive viewport runs in Tauri
  - The embedded surface was validated interactively for playback, pause, resize, and layout clipping.
- [x] Preview uses same simulation code as offline rendering
- [x] Timeline scrubbing is functional
  - Forward/backward seeks and deterministic duration wrapping were validated interactively.
- [x] Preview quality can be reduced without changing project intent
  - Quality levels retain deterministic particle prefixes at 100K, 500K, or the full project count.

---

# Phase 18 — Parameter Inspector and Modulation UI

## Goals

Make the engine usable without editing JSON.

## Tasks

- [x] Generate controls from parameter schema
- [x] Add sliders
- [x] Add numeric inputs
- [x] Add toggles
- [x] Add color controls
- [x] Add macro controls
- [x] Add modulation button per parameter
- [x] Add modulation editor
- [x] Show source
- [x] Show amount
- [x] Show attack/release
- [x] Show curve
- [x] Show live modulation value
- [x] Allow disabling a mapping
- [x] Persist changes to project state

## Acceptance Criteria

- [x] A visual preset can be meaningfully edited without JSON
  - Interactively validated in the Tauri editor.
- [x] Audio mappings can be created and modified in UI
  - Mapping CRUD, enablement, and live values were interactively validated with audio.
- [x] UI edits serialize back into project data
  - Native validation and save commands persist a resolved, self-contained project JSON.

---

# Phase 19 — Audio Waveform and Timeline

## Goals

Add enough timeline functionality for music-video composition.

## Tasks

- [x] Render waveform
- [x] Display playback cursor
- [x] Add zoom
- [x] Add selection range
- [x] Add preview slice range
- [x] Display transient markers
- [x] Display beat markers when available
  - Timeline accepts and renders beat markers; the current analysis version does not produce beat data yet.
- [x] Add simple parameter automation tracks
- [x] Add camera automation track
- [x] Add scene marker support

## Acceptance Criteria

- [x] User can select a 5–10 second preview region
  - Interactively validated in the Tauri editor.
- [x] User can scrub audio and visuals together
  - Canvas seeking and synchronized preview were interactively validated with audio.
- [x] Basic automation can be edited
  - Track/keyframe editing and deterministic preview evaluation were interactively validated.

---

# Phase 20 — Preview Slice and Still Workflow

## Goals

Support rapid look-development before committing to long renders.

## Tasks

- [x] Add interactive preview mode
  - Reuses the validated Phase 17 viewport path.
- [x] Add high-quality still preview
- [x] Add high-quality slice preview
- [x] Add preview queue
- [x] Add temporary preview output management
- [~] Add render comparison workflow if simple enough
  - Inline comparison was replaced with opening completed PNG/MP4 previews in the user's default application.
- [x] Add "use final settings for selected range" option

## Acceptance Criteria

- [x] User can render one full-quality 4K still
  - Interactively validated through the shared final-quality offscreen pipeline.
- [x] User can render a short near-final-quality clip
  - Queued H.264 slice rendering and default-application opening were interactively validated.
- [x] Preview workflow does not require full-song export
  - Still and selected-range workflows were interactively validated independently of full-song export.

---

# Phase 21 — 4K Production Export

## Goals

Make Rustique useful for complete music videos.

## Tasks

- [x] Add 3840x2160 presets
- [x] Add 30 / 60 FPS presets
- [x] Add supersampling configuration
- [x] Add motion blur framework
  - Deterministic FFmpeg temporal mixing is configurable from 1–16 frames.
- [x] Add configurable simulation substeps
- [x] Add render progress ETA based on measured throughput
- [x] Add output naming
- [x] Add resume strategy investigation
  - A safe segmented-render approach is documented in `docs/RENDER_RESUME.md`; unsafe partial-container resume is intentionally not exposed.
- [x] Add render manifest with:
  - [x] project version
  - [x] engine version
  - [x] GPU
  - [x] render settings
  - [x] seed
- [x] Add final audio mux validation
  - FFprobe must confirm an audio stream before a production job is marked complete.

## Acceptance Criteria

- [x] A complete 4K video can be rendered locally
  - Implemented through the queued production exporter; requires a full interactive render validation.
- [x] Long renders remain memory-stable
  - The existing constant-memory frame streaming path is retained; requires a long production render validation.
- [x] Output can be reproduced from the saved project and render config
  - Each render writes a baked project snapshot and settings manifest; requires interactive artifact validation.

---

# Phase 22 — Render Packages

## Goals

Make projects portable to Linux / cloud renderers.

## Tasks

- [x] Define `.rustiqueproject` or directory-based package layout
- [x] Bundle:
  - [x] project JSON
  - [x] render config
  - [x] audio
  - [x] textures
  - [x] HDRIs
  - [x] meshes
- [x] Add package validation
- [x] Add relative asset paths
- [x] Add package creation command
- [x] Add package load command
- [x] Add checksum or asset fingerprinting if useful

## Acceptance Criteria

- [x] A package renders without access to original local asset paths
  - Package tests delete the original inputs and load from the moved package.
- [x] Same package can be moved between machines
  - Package paths are relative and platform-neutral; Linux execution is validated in Phase 23.

---

# Phase 23 — Linux / WSL Validation

## Goals

Verify the renderer independently of Windows desktop tooling.

## Tasks

- [x] Create WSL clone
- [x] Install Linux Rust toolchain
- [x] Install required Vulkan userspace dependencies
- [x] Verify NVIDIA adapter is visible
- [x] Compile workspace under Linux
- [x] Run `particle-render --gpu-info`
- [x] Render offscreen still
- [x] Render short audio-reactive clip
- [x] Document Linux dependencies
- [x] Fix any accidental Windows coupling

## Acceptance Criteria

- [x] Headless renderer works in WSL
- [x] Core crates remain platform-neutral
- [x] Output is visually consistent with Windows within acceptable GPU differences

---

# Phase 24 — Dockerized Renderer

## Goals

Create a portable cloud-ready renderer image.

## Tasks

- [x] Add Dockerfile
- [x] Use multi-stage Rust build
- [x] Include runtime Vulkan dependencies
- [x] Include FFmpeg
- [x] Copy shaders and runtime assets
- [x] Add container entrypoint
- [x] Support render package input
- [x] Support mounted input/output directories
- [x] Test GPU passthrough locally if available
- [x] Keep image independent from RunPod APIs

  Docker Vulkan initialization succeeds, but the local container currently sees
  Mesa llvmpipe (CPU) instead of the NVIDIA GPU, so GPU validation is deferred.

## Acceptance Criteria

- [x] Container launches `particle-render`
- [x] Container renders a still using GPU
- [x] Container renders a video using GPU
- [x] Input/output are passed through mounted paths

---

# Phase 25 — RunPod Validation

## Goals

Run the exact headless renderer on a rented GPU.

## Tasks

- [~] Publish or upload Docker image
- [~] Start a RunPod GPU Pod
- [~] Verify Vulkan / wgpu adapter
- [~] Upload render package
- [~] Render still
- [~] Render short clip
- [~] Render full test song segment
- [~] Record GPU model and performance
- [~] Compare local RTX 4070 and cloud output
- [~] Document RunPod launch procedure

## Acceptance Criteria

- [~] Same render package works locally and on RunPod
- [~] No code changes are required for cloud rendering
- [~] Render output can be retrieved cleanly

---

# Phase 26 — Cloud Render Submission from Desktop

## Goals

Optional convenience layer for sending heavy exports to a remote renderer.

## Tasks

- [~] Define remote render job schema
- [~] Package project automatically
- [~] Upload package
- [~] Start remote job
- [~] Poll job status
- [~] Download final output
- [~] Show progress in UI
- [~] Keep provider-specific code isolated
- [~] Support local render fallback

## Acceptance Criteria

- [~] Desktop can submit a project for remote rendering
- [~] Core renderer remains provider-independent

---

# Phase 27 — Advanced Simulation: Spatial Structures

## Goals

Prepare for physically richer systems.

## Tasks

- [x] Implement GPU spatial hash or uniform grid
- [x] Add neighbor lookup
- [x] Benchmark neighbor counts
- [x] Add debug visualization
- [x] Add memory-bound stress tests
- [x] Document complexity and limits
  - Added a headless GPU uniform grid with bounded cell storage, 27-cell radius queries, occupancy/neighbor/overflow metrics, repeatable stress iterations, and a projected occupancy PNG. See `docs/SPATIAL_GRID.md`.

## Acceptance Criteria

- [x] Neighbor queries scale substantially better than O(N^2)
- [x] Structure can support future fluid / boid systems

---

# Phase 28 — Advanced Simulation: Fluid / Slime Systems

## Goals

Build the first legitimately ridiculous material simulation.

## Tasks

- [x] Prototype SPH or PBF approach
- [x] Add density calculation
- [x] Add pressure / constraint solve
- [x] Add viscosity
- [x] Add surface cohesion
- [x] Add audio-reactive pressure/turbulence
- [x] Add 3D density reconstruction experiment
- [x] Investigate metaball / marching-cubes surface generation
- [x] Investigate screen-space fluid rendering
- [x] Profile 100k / 500k / 1M+ fluid particles
  - Added a bounded-neighbor GPU SPH prototype, density-shaded slime renderer,
    audio sample controls, scale measurements, and rendering conclusions in
    `docs/FLUID.md`.

## Acceptance Criteria

- [-] Green Slime preset uses genuine neighbor-based behavior
  - The new headless `fluid` path is genuine neighbor-based green slime; wiring
    the version-1 visual preset/project schema into this specialized solver is
    still outstanding.
- [x] Fluid system remains usable for offline 4K rendering
  - A 100k-particle 3840x2160 smoke render completed in 13.318 ms for its final
    measured frame on the development RTX 4070.

---

# Phase 29 — Advanced Rendering: Volumetrics

## Goals

Support nebula, smoke, glow, and density-based scenes.

## Tasks

- [x] Add 3D density texture or sparse alternative
- [x] Add particle-to-volume splatting
- [x] Add raymarching
- [x] Add light absorption
- [x] Add emission
- [x] Add quality-controlled ray steps
- [x] Add temporal accumulation
- [x] Add half-resolution preview mode

## Acceptance Criteria

- [x] Nebula preset can use real volumetric rendering
- [x] Volumetric quality scales independently between preview and export

  - The headless volumetric pipeline uses a reusable atomic density grid and deterministic particle splatting. Draft/preview/final select independent grid, ray-step, and sampling levels; see `docs/VOLUMETRICS.md`.

---

# Phase 30 — Advanced Rendering: Liquid Chrome

## Goals

Build a convincing reflective liquid-metal system.

## Tasks

- [x] Add environment map loading
- [x] Add HDRI support
- [x] Add PBR-like metallic shading
- [x] Add roughness
- [x] Add reflection intensity
- [x] Add fluid/metaball surface source
- [x] Add normal generation
- [x] Add Milky Way environment preset
- [x] Add audio-reactive material modulation

## Acceptance Criteria

- [x] Liquid Chrome can reflect an HDRI/image environment
- [x] Material parameters are tweakable and modulatable

  - The headless liquid-chrome path raymarches a deterministic metaball SDF and
    supports equirectangular HDR/PNG/JPEG inputs plus a built-in Milky Way map.
    Roughness, reflection intensity, and surface scale use the shared offline
    modulation system; see `docs/LIQUID_CHROME.md`.

---

# Phase 31 — Advanced Rendering: Water Droplets

## Goals

Create a distinct refractive droplet system.

## Tasks

- [x] Add droplet instancing or surface representation
- [x] Add refraction approximation
- [x] Add Fresnel
- [x] Add environment/background distortion
- [x] Add size distribution
- [x] Add gravity / surface motion
- [x] Add audio-reactive droplet emission
- [x] Add condensation-style preset

## Acceptance Criteria

- [x] Water Droplets looks visually distinct from generic particles
- [x] Droplet properties are data-driven

  - The headless `water_droplets` path renders deterministic screen-space
    condensation with refracted background detail, Fresnel rims, seeded size
    variation, gravity motion, and an emission input compatible with the shared
    `burst_emission` modulation target; see `docs/WATER_DROPLETS.md`.

---

# Phase 32 — Multi-Layer Scenes

## Goals

Combine multiple particle/material systems into one scene.

## Tasks

- [x] Define scene layer schema
- [x] Add per-layer simulation settings
- [x] Add per-layer modulation
- [x] Add per-layer blending
- [x] Add per-layer visibility
- [x] Add layer ordering/depth behavior
- [x] Add independent quality scaling
- [x] Add example:
  - [x] nebula background
  - [x] stars
  - [x] foreground dust
  - [x] transient burst layer

## Acceptance Criteria

- [x] At least three independent systems can render together
- [x] Layers serialize cleanly

  - Added a deterministic headless compositor for particle, volumetric, and
    water-droplet layers with alpha/add/screen blending, stable depth ordering,
    visibility, per-layer settings/mappings, and particle quality scaling. See
    `examples/multi-layer.rustique.json` and `docs/MULTI_LAYER_SCENES.md`.

---

# Phase 33 — Experimental Creative Tools

These are optional and should not block core development.

## Tasks

- [x] Controlled parameter randomization
  - Deterministic, seed-based preset macro mutation stays within declared ranges.
- [x] Mutation amount slider
  - The editor exposes mutation strength and generates repeatable variations.
- [x] Preset morphing
  - The editor interpolates shared scene properties between any two file-backed presets.
- [x] Seamless loop generation
  - A creative command configures an exact one-revolution camera orbit and disables non-looping camera motion.
- [x] Shape targets
- [x] GLTF/GLB mesh emission
- [x] Mesh dissolution
- [x] Text/SVG particle targets
  - The shared headless target pipeline deterministically samples primitives, GLTF/GLB positions, rasterized SVG, and font-rendered text, with a 0–1 dissolution blend.
- [x] MIDI input
- [x] OSC input
  - The desktop inspector can connect to MIDI ports, normalize CC messages, bind an OSC UDP address, and display numeric live values.
- [x] Render passes:
  - [x] alpha
  - [x] depth
  - [x] normals
  - [x] motion vectors
  - [x] emission
  - The experimental headless target command exports deterministic screen-space auxiliary PNG passes.
- [x] EXR output
- [x] Transparent ProRes output
  - Target renders can emit floating-point RGBA OpenEXR; post-processing preserves alpha and both CLI and desktop expose ProRes 4444.

---

# Phase 34 — Maintenance and Technical Debt

This phase is continuous.

## Tasks

- [ ] Keep dependencies reasonably current
- [ ] Keep `DESIGN.md` aligned with major architecture changes
- [ ] Keep `PLAN.md` checkboxes current
- [ ] Add regression tests for bugs
- [ ] Keep project-format migrations explicit
- [ ] Profile before large optimizations
- [ ] Remove unused abstractions
- [ ] Keep shader conventions documented
- [ ] Revisit GPU buffer layouts when data grows
- [ ] Revisit VRAM budgets as advanced systems land

---

# Immediate Next Tasks

These are the recommended next actions from the current state.

- [x] Create the Rustique Cargo workspace
- [x] Add the initial crate structure
- [x] Move the working GPU smoke test into `render-core`
- [x] Add `particle-render --gpu-info`
- [x] Implement the first headless offscreen PNG render
- [x] Commit the baseline before starting particle simulation

Do not start Tauri UI work before the headless offscreen path is stable.
