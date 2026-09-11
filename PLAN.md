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

- [ ] Load project
- [ ] Load audio
- [ ] Load cached audio analysis
- [ ] Simulate frame-by-frame
- [ ] Apply modulation
- [ ] Render frames offscreen
- [ ] Save temporary PNG sequence first
- [ ] Verify synchronization with audio
- [ ] Render at:
  - [ ] 1280x720
  - [ ] 1920x1080
- [ ] Create a short 10–30 second test project

## Acceptance Criteria

- [ ] A deterministic audio-reactive sequence can be rendered from CLI
- [ ] Visual reactions line up with audio events
- [ ] Rendering can proceed slower than realtime

---

# Phase 11 — FFmpeg Video Export

## Goals

Stream frames directly into FFmpeg and create playable video files.

## Tasks

- [ ] Add FFmpeg process wrapper
- [ ] Stream raw frames through stdin
- [ ] Avoid storing full sequence in memory
- [ ] Mux original audio into final output
- [ ] Add H.264 output
- [ ] Add HEVC output
- [ ] Add ProRes 422 HQ output
- [ ] Add alpha-capable output path if practical
- [ ] Add export progress reporting
- [ ] Handle FFmpeg errors cleanly
- [ ] Add cancellation support
- [ ] Clean up failed temporary files

## Acceptance Criteria

- [ ] CLI produces a valid video with synchronized audio
- [ ] Memory use does not grow with video duration
- [ ] 4K frame streaming works
- [ ] Failed encodes produce useful errors

---

# Phase 12 — Post-Processing Foundation

## Goals

Make simple particle scenes visually rich.

## Tasks

- [ ] Add HDR scene target
- [ ] Add tone mapping
- [ ] Add bloom
- [ ] Add exposure
- [ ] Add gamma/output transform
- [ ] Add trails / temporal accumulation
- [ ] Add optional vignette
- [ ] Add optional chromatic aberration
- [ ] Add render-target pooling or explicit reuse
- [ ] Add quality presets for post-processing

## Acceptance Criteria

- [ ] Bloom works at 4K
- [ ] HDR pipeline does not cause uncontrolled VRAM growth
- [ ] Post effects can be toggled independently
- [ ] Preview and offline renderer use the same post pipeline

---

# Phase 13 — Camera System v1

## Goals

Add reusable procedural camera behaviors.

## Tasks

- [ ] Add perspective camera parameters
- [ ] Add static camera
- [ ] Add orbit camera
- [ ] Add look-at target
- [ ] Add dolly / distance control
- [ ] Add procedural drift
- [ ] Add FOV modulation
- [ ] Add audio-reactive camera shake
- [ ] Serialize camera settings
- [ ] Keep camera independent from render resolution

## Acceptance Criteria

- [ ] Camera settings load from project data
- [ ] Camera can react to modulation
- [ ] Star-system scenes can be navigated cinematically

---

# Phase 14 — Visual Preset Framework

## Goals

Make visual systems data-driven and composable.

## Tasks

- [ ] Define visual preset schema
- [ ] Define macro parameter schema
- [ ] Define preset defaults
- [ ] Define default modulation recommendations
- [ ] Load presets from files
- [ ] Add preset override support
- [ ] Implement first visual presets:
  - [ ] Star System
  - [ ] Nebula
  - [ ] Liquid Chrome prototype
  - [ ] Green Slime prototype
  - [ ] Water Droplets prototype
- [ ] Ensure presets reuse common engine systems where possible

## Acceptance Criteria

- [ ] Presets are not hardcoded into UI logic
- [ ] At least three visually distinct presets work
- [ ] Preset parameters can be overridden per project

---

# Phase 15 — Audio Profile System

## Goals

Create reusable genre-aware analysis and reaction defaults.

## Tasks

- [ ] Define analysis profile schema
- [ ] Define reaction profile schema
- [ ] Implement analysis profiles:
  - [ ] Techno
  - [ ] Drum & Bass
  - [ ] Ambient
  - [ ] Cinematic
- [ ] Implement reaction profiles:
  - [ ] Punchy
  - [ ] Fluid
  - [ ] Dreamy
  - [ ] Aggressive
- [ ] Add profile-level attack/release defaults
- [ ] Add profile-level frequency weighting
- [ ] Add recommended mappings
- [ ] Allow project overrides

## Acceptance Criteria

- [ ] Same visual preset behaves differently under at least two audio profiles
- [ ] Profiles remain editable data
- [ ] Profile selection does not bypass the universal modulation system

---

# Phase 16 — Desktop App Scaffold

## Goals

Create the Tauri 2 + React + TypeScript editor.

## Tasks

- [ ] Create Tauri 2 application
- [ ] Use Vite
- [ ] Use React
- [ ] Use TypeScript
- [ ] Add Zustand
- [ ] Add application shell
- [ ] Add viewport area
- [ ] Add inspector area
- [ ] Add preset browser area
- [ ] Add audio/timeline area
- [ ] Add render settings area
- [ ] Keep UI state separate from engine state
- [ ] Create minimal Rust command bridge
- [ ] Do not duplicate renderer logic inside Tauri

## Acceptance Criteria

- [ ] Desktop app launches
- [ ] UI can query GPU info from native engine
- [ ] UI can load a project
- [ ] Render core remains usable without Tauri

---

# Phase 17 — Interactive Viewport

## Goals

Use the same engine for live preview.

## Tasks

- [ ] Add window/surface rendering path
- [ ] Share pipelines with offscreen renderer
- [ ] Add resize handling
- [ ] Add preview quality configuration
- [ ] Add play/pause
- [ ] Add timeline scrub
- [ ] Add reset
- [ ] Add realtime audio-position sampling
- [ ] Add preview FPS display
- [ ] Add particle count display
- [ ] Add GPU timing display where practical

## Acceptance Criteria

- [ ] Interactive viewport runs in Tauri
- [ ] Preview uses same simulation code as offline rendering
- [ ] Timeline scrubbing is functional
- [ ] Preview quality can be reduced without changing project intent

---

# Phase 18 — Parameter Inspector and Modulation UI

## Goals

Make the engine usable without editing JSON.

## Tasks

- [ ] Generate controls from parameter schema
- [ ] Add sliders
- [ ] Add numeric inputs
- [ ] Add toggles
- [ ] Add color controls
- [ ] Add macro controls
- [ ] Add modulation button per parameter
- [ ] Add modulation editor
- [ ] Show source
- [ ] Show amount
- [ ] Show attack/release
- [ ] Show curve
- [ ] Show live modulation value
- [ ] Allow disabling a mapping
- [ ] Persist changes to project state

## Acceptance Criteria

- [ ] A visual preset can be meaningfully edited without JSON
- [ ] Audio mappings can be created and modified in UI
- [ ] UI edits serialize back into project data

---

# Phase 19 — Audio Waveform and Timeline

## Goals

Add enough timeline functionality for music-video composition.

## Tasks

- [ ] Render waveform
- [ ] Display playback cursor
- [ ] Add zoom
- [ ] Add selection range
- [ ] Add preview slice range
- [ ] Display transient markers
- [ ] Display beat markers when available
- [ ] Add simple parameter automation tracks
- [ ] Add camera automation track
- [ ] Add scene marker support

## Acceptance Criteria

- [ ] User can select a 5–10 second preview region
- [ ] User can scrub audio and visuals together
- [ ] Basic automation can be edited

---

# Phase 20 — Preview Slice and Still Workflow

## Goals

Support rapid look-development before committing to long renders.

## Tasks

- [ ] Add interactive preview mode
- [ ] Add high-quality still preview
- [ ] Add high-quality slice preview
- [ ] Add preview queue
- [ ] Add temporary preview output management
- [ ] Add render comparison workflow if simple enough
- [ ] Add "use final settings for selected range" option

## Acceptance Criteria

- [ ] User can render one full-quality 4K still
- [ ] User can render a short near-final-quality clip
- [ ] Preview workflow does not require full-song export

---

# Phase 21 — 4K Production Export

## Goals

Make Rustique useful for complete music videos.

## Tasks

- [ ] Add 3840x2160 presets
- [ ] Add 30 / 60 FPS presets
- [ ] Add supersampling configuration
- [ ] Add motion blur framework
- [ ] Add configurable simulation substeps
- [ ] Add render progress ETA based on measured throughput
- [ ] Add output naming
- [ ] Add resume strategy investigation
- [ ] Add render manifest with:
  - [ ] project version
  - [ ] engine version
  - [ ] GPU
  - [ ] render settings
  - [ ] seed
- [ ] Add final audio mux validation

## Acceptance Criteria

- [ ] A complete 4K video can be rendered locally
- [ ] Long renders remain memory-stable
- [ ] Output can be reproduced from the saved project and render config

---

# Phase 22 — Render Packages

## Goals

Make projects portable to Linux / cloud renderers.

## Tasks

- [ ] Define `.rustiqueproject` or directory-based package layout
- [ ] Bundle:
  - [ ] project JSON
  - [ ] render config
  - [ ] audio
  - [ ] textures
  - [ ] HDRIs
  - [ ] meshes
- [ ] Add package validation
- [ ] Add relative asset paths
- [ ] Add package creation command
- [ ] Add package load command
- [ ] Add checksum or asset fingerprinting if useful

## Acceptance Criteria

- [ ] A package renders without access to original local asset paths
- [ ] Same package can be moved between machines

---

# Phase 23 — Linux / WSL Validation

## Goals

Verify the renderer independently of Windows desktop tooling.

## Tasks

- [ ] Create WSL clone
- [ ] Install Linux Rust toolchain
- [ ] Install required Vulkan userspace dependencies
- [ ] Verify NVIDIA adapter is visible
- [ ] Compile workspace under Linux
- [ ] Run `particle-render --gpu-info`
- [ ] Render offscreen still
- [ ] Render short audio-reactive clip
- [ ] Document Linux dependencies
- [ ] Fix any accidental Windows coupling

## Acceptance Criteria

- [ ] Headless renderer works in WSL
- [ ] Core crates remain platform-neutral
- [ ] Output is visually consistent with Windows within acceptable GPU differences

---

# Phase 24 — Dockerized Renderer

## Goals

Create a portable cloud-ready renderer image.

## Tasks

- [ ] Add Dockerfile
- [ ] Use multi-stage Rust build
- [ ] Include runtime Vulkan dependencies
- [ ] Include FFmpeg
- [ ] Copy shaders and runtime assets
- [ ] Add container entrypoint
- [ ] Support render package input
- [ ] Support mounted input/output directories
- [ ] Test GPU passthrough locally if available
- [ ] Keep image independent from RunPod APIs

## Acceptance Criteria

- [ ] Container launches `particle-render`
- [ ] Container renders a still using GPU
- [ ] Container renders a video using GPU
- [ ] Input/output are passed through mounted paths

---

# Phase 25 — RunPod Validation

## Goals

Run the exact headless renderer on a rented GPU.

## Tasks

- [ ] Publish or upload Docker image
- [ ] Start a RunPod GPU Pod
- [ ] Verify Vulkan / wgpu adapter
- [ ] Upload render package
- [ ] Render still
- [ ] Render short clip
- [ ] Render full test song segment
- [ ] Record GPU model and performance
- [ ] Compare local RTX 4070 and cloud output
- [ ] Document RunPod launch procedure

## Acceptance Criteria

- [ ] Same render package works locally and on RunPod
- [ ] No code changes are required for cloud rendering
- [ ] Render output can be retrieved cleanly

---

# Phase 26 — Cloud Render Submission from Desktop

## Goals

Optional convenience layer for sending heavy exports to a remote renderer.

## Tasks

- [ ] Define remote render job schema
- [ ] Package project automatically
- [ ] Upload package
- [ ] Start remote job
- [ ] Poll job status
- [ ] Download final output
- [ ] Show progress in UI
- [ ] Keep provider-specific code isolated
- [ ] Support local render fallback

## Acceptance Criteria

- [ ] Desktop can submit a project for remote rendering
- [ ] Core renderer remains provider-independent

---

# Phase 27 — Advanced Simulation: Spatial Structures

## Goals

Prepare for physically richer systems.

## Tasks

- [ ] Implement GPU spatial hash or uniform grid
- [ ] Add neighbor lookup
- [ ] Benchmark neighbor counts
- [ ] Add debug visualization
- [ ] Add memory-bound stress tests
- [ ] Document complexity and limits

## Acceptance Criteria

- [ ] Neighbor queries scale substantially better than O(N^2)
- [ ] Structure can support future fluid / boid systems

---

# Phase 28 — Advanced Simulation: Fluid / Slime Systems

## Goals

Build the first legitimately ridiculous material simulation.

## Tasks

- [ ] Prototype SPH or PBF approach
- [ ] Add density calculation
- [ ] Add pressure / constraint solve
- [ ] Add viscosity
- [ ] Add surface cohesion
- [ ] Add audio-reactive pressure/turbulence
- [ ] Add 3D density reconstruction experiment
- [ ] Investigate metaball / marching-cubes surface generation
- [ ] Investigate screen-space fluid rendering
- [ ] Profile 100k / 500k / 1M+ fluid particles

## Acceptance Criteria

- [ ] Green Slime preset uses genuine neighbor-based behavior
- [ ] Fluid system remains usable for offline 4K rendering

---

# Phase 29 — Advanced Rendering: Volumetrics

## Goals

Support nebula, smoke, glow, and density-based scenes.

## Tasks

- [ ] Add 3D density texture or sparse alternative
- [ ] Add particle-to-volume splatting
- [ ] Add raymarching
- [ ] Add light absorption
- [ ] Add emission
- [ ] Add quality-controlled ray steps
- [ ] Add temporal accumulation
- [ ] Add half-resolution preview mode

## Acceptance Criteria

- [ ] Nebula preset can use real volumetric rendering
- [ ] Volumetric quality scales independently between preview and export

---

# Phase 30 — Advanced Rendering: Liquid Chrome

## Goals

Build a convincing reflective liquid-metal system.

## Tasks

- [ ] Add environment map loading
- [ ] Add HDRI support
- [ ] Add PBR-like metallic shading
- [ ] Add roughness
- [ ] Add reflection intensity
- [ ] Add fluid/metaball surface source
- [ ] Add normal generation
- [ ] Add Milky Way environment preset
- [ ] Add audio-reactive material modulation

## Acceptance Criteria

- [ ] Liquid Chrome can reflect an HDRI/image environment
- [ ] Material parameters are tweakable and modulatable

---

# Phase 31 — Advanced Rendering: Water Droplets

## Goals

Create a distinct refractive droplet system.

## Tasks

- [ ] Add droplet instancing or surface representation
- [ ] Add refraction approximation
- [ ] Add Fresnel
- [ ] Add environment/background distortion
- [ ] Add size distribution
- [ ] Add gravity / surface motion
- [ ] Add audio-reactive droplet emission
- [ ] Add condensation-style preset

## Acceptance Criteria

- [ ] Water Droplets looks visually distinct from generic particles
- [ ] Droplet properties are data-driven

---

# Phase 32 — Multi-Layer Scenes

## Goals

Combine multiple particle/material systems into one scene.

## Tasks

- [ ] Define scene layer schema
- [ ] Add per-layer simulation settings
- [ ] Add per-layer modulation
- [ ] Add per-layer blending
- [ ] Add per-layer visibility
- [ ] Add layer ordering/depth behavior
- [ ] Add independent quality scaling
- [ ] Add example:
  - [ ] nebula background
  - [ ] stars
  - [ ] foreground dust
  - [ ] transient burst layer

## Acceptance Criteria

- [ ] At least three independent systems can render together
- [ ] Layers serialize cleanly

---

# Phase 33 — Experimental Creative Tools

These are optional and should not block core development.

## Tasks

- [ ] Controlled parameter randomization
- [ ] Mutation amount slider
- [ ] Preset morphing
- [ ] Seamless loop generation
- [ ] Shape targets
- [ ] GLTF/GLB mesh emission
- [ ] Mesh dissolution
- [ ] Text/SVG particle targets
- [ ] MIDI input
- [ ] OSC input
- [ ] Render passes:
  - [ ] alpha
  - [ ] depth
  - [ ] normals
  - [ ] motion vectors
  - [ ] emission
- [ ] EXR output
- [ ] Transparent ProRes output

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
