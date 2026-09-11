# DESIGN.md — Audio-Reactive Particle Renderer

## 1. Project Goal

Build a personal desktop application for creating elaborate, audio-reactive particle simulations and rendering them as high-quality music videos.

The application should:

- Provide a fast interactive editor for designing visuals.
- Support highly configurable visual presets such as:
  - Liquid metal / chrome
  - Water droplets
  - Green slime / organic goo
  - Star systems / galaxies
  - Nebulae
  - Particle swarms
  - Abstract vector-field systems
- Support reusable audio-reactivity presets aimed at musical genres and reaction styles.
- Allow any meaningful visual parameter to be modulated by audio features.
- Support deterministic simulation so preview and final renders are reproducible.
- Support local 4K offline rendering.
- Support a headless CLI renderer.
- Be deployable in a Docker container to GPU services such as RunPod.
- Allow heavier cloud renders than the local development GPU can comfortably handle.

Primary development machine:

- NVIDIA RTX 4070, 12 GB VRAM
- 64 GB DDR5 system RAM

This is a personal creative tool, not a commercial product. Favor experimentation, power, and developer ergonomics over enterprise complexity.

---

## 2. Core Technology Stack

### Desktop UI

- Tauri 2
- React
- TypeScript
- Vite
- Zustand for editor state

The UI should remain in TypeScript.

### Native Engine

- Rust
- wgpu
- WGSL compute/render shaders

Rust owns:

- GPU initialization
- Simulation
- Rendering
- GPU resources
- Audio analysis
- Preset/project deserialization
- Offline frame rendering
- FFmpeg process management
- Headless rendering

### Audio

Initial approach:

- Decode audio natively in Rust.
- Pre-analyze complete tracks before playback/rendering.
- Store normalized time-series features.

Features should eventually include:

- RMS / loudness
- Sub energy
- Bass
- Low mids
- Mids
- High mids
- Highs
- Spectral centroid
- Spectral flux
- Transient strength
- Beat phase
- BPM
- Kick likelihood
- Snare likelihood
- Hi-hat activity
- Stereo width

The render engine samples cached audio features by simulation time.

### Encoding

Use FFmpeg as an external process.

Final frames should be streamed to FFmpeg rather than accumulated in RAM.

Possible output formats:

- H.264
- H.265 / HEVC
- AV1
- ProRes 422 HQ
- ProRes 4444 where alpha is required
- Image sequence for debugging / professional workflows

---

## 3. Architectural Principle

The editor and renderer must be separate.

The rendering engine must not depend on Tauri, React, or a window.

Architecture:

```text
                +------------------+
                | React / Tauri UI |
                +---------+--------+
                          |
                    project state
                          |
                          v
                +------------------+
                |   Render Core    |
                |  Rust + wgpu     |
                +--------+---------+
                         |
               +---------+----------+
               |                    |
               v                    v
        Interactive Preview   Headless Renderer
               |                    |
            Window                Offscreen
                                    |
                                    v
                                  FFmpeg
                                    |
                                    v
                               Video Output
```

The same render core must be usable by:

1. The desktop application.
2. A command-line renderer.
3. A Dockerized cloud renderer.

---

## 4. Repository Layout

Suggested monorepo:

```text
particle-studio/

  DESIGN.md
  README.md
  Cargo.toml

  apps/
    desktop/
      src/
      src-tauri/
      package.json

  crates/
    render-core/
      src/
        lib.rs
        engine.rs
        gpu.rs
        frame.rs
        resources.rs

    simulation/
      src/
        lib.rs
        particle.rs
        emitter.rs
        forces.rs
        systems.rs

    audio-engine/
      src/
        lib.rs
        decode.rs
        analysis.rs
        features.rs
        cache.rs

    project-format/
      src/
        lib.rs
        project.rs
        preset.rs
        modulation.rs
        render_config.rs

    exporter/
      src/
        lib.rs
        ffmpeg.rs
        headless.rs

  tools/
    particle-render/
      src/
        main.rs

  shaders/
    particles/
      update.wgsl
      render.wgsl

    forces/
      attractor.wgsl
      curl_noise.wgsl

    post/
      bloom.wgsl
      composite.wgsl

  presets/
    visual/
    audio/
    reaction/

  assets/
```

Avoid putting substantial renderer logic inside the Tauri app.

---

## 5. Engine Design

### Frame Inputs

Every rendered frame should depend on explicit state:

```text
Project
FrameIndex
FPS
SimulationSeed
RenderConfig
AudioFeatureSample
```

Simulation time:

```text
time = frame_index / fps
```

Never use wall-clock time to advance the simulation during offline rendering.

### Determinism

Deterministic rendering is a hard requirement.

Given:

```text
same project
same seed
same frame index
same engine version
same render configuration
```

the engine should reproduce the same scene state to the practical extent allowed by GPU floating-point behavior.

Store:

- Random seed
- FPS
- Simulation timestep
- Simulation substeps
- Engine/project version

---

## 6. Particle Representation

Start with a GPU-friendly 64-byte particle:

```rust
#[repr(C)]
struct Particle {
    position_age: [f32; 4],
    velocity_lifetime: [f32; 4],
    color: [f32; 4],
    params: [f32; 4],
}
```

Equivalent WGSL struct must have matching layout.

Initial state size:

```text
64 bytes / particle
```

Double-buffered simulation:

```text
1M particles  ~= 128 MB
5M particles  ~= 640 MB
10M particles ~= 1.28 GB
20M particles ~= 2.56 GB
```

Future systems may use specialized smaller particle layouts.

Do not use per-particle Rust or JS heap objects during simulation.

Particle state should remain GPU-resident whenever possible.

---

## 7. GPU Simulation Model

Use compute shaders.

Typical frame:

```text
ParticleBuffer A
      |
      v
Emitter / Spawn Pass
      |
      v
Force Pass
      |
      v
Integration Pass
      |
      v
Optional Collision Pass
      |
      v
ParticleBuffer B
      |
      v
Render Pass
```

Use ping-pong buffers for simulation state.

Initial compute systems:

1. Basic integration
2. Gravity
3. Point attractor / repulsor
4. Curl-noise-like field
5. Orbital / vortex force
6. Particle lifetime / respawn

Later systems may add:

- Spatial hashing
- Neighbor grids
- Boids
- SPH / PBF fluids
- SDF collisions
- GPU sorting
- Density fields
- Marching cubes
- Volumetric splatting
- N-body approximations

Avoid naive O(N^2) particle interactions.

---

## 8. Rendering Pipeline

Initial pipeline:

```text
Simulation Compute
       |
       v
Particle Render
       |
       v
HDR Scene Texture
       |
       +--> Bloom
       |
       +--> Trails / temporal accumulation
       |
       +--> Optional depth / velocity
       |
       v
Tone Mapping
       |
       v
Final Frame
```

Use an HDR intermediate format where appropriate.

Do not make simulation state depend on output resolution.

The same scene should render consistently at:

- 720p
- 1080p
- 4K
- 8K

### Quality Levels

Preview:

```text
1280x720 or 1920x1080
reduced particle count
reduced volumetrics
reduced bloom
lower trail quality
```

Final:

```text
3840x2160
full particle count
full post-processing
optional supersampling
motion blur
temporal effects
```

Offline rendering does not need to run in realtime.

---

## 9. Resource Budget

Target local GPU:

RTX 4070 12 GB.

Do not intentionally consume all available VRAM.

Suggested budget ceiling:

```text
Simulation state       4–6 GB
Textures / volumes     1–2 GB
Render targets         1–2 GB
Assets / environments  ~1 GB
Safety headroom        2+ GB
```

Expected experimental ranges:

```text
Simple point/star systems:
  preview: 2–10M
  offline: 10–40M

Moderate force-field systems:
  preview: 1–5M
  offline: 5–20M

Complex trails / heavy shading:
  preview: 0.5–3M
  offline: 2–10M

Neighbor-based fluids:
  preview: 0.1–1M
  offline: 0.5–5M
```

These are architectural targets, not guaranteed performance numbers.

Profile every simulation type independently.

---

## 10. Visual Preset System

Visual presets are data, not hardcoded UI behavior.

Examples:

- Liquid Chrome
- Green Slime
- Water Droplets
- Galaxy
- Star System
- Nebula
- Ferrofluid
- Particle Tunnel
- Strange Attractor
- Plasma
- Embers
- Smoke
- Crystal Growth

A visual preset may define:

```text
Simulation
Material
Environment
Lighting
Camera defaults
Post-processing
Macro parameters
Default modulation targets
```

Example:

```text
Liquid Chrome

Simulation:
  metaball-like particle system

Material:
  reflective metallic surface

Environment:
  HDR environment map

Post:
  bloom
  motion blur
  tone mapping
```

---

## 11. Audio Mapping Architecture

Audio analysis and visual behavior should be separate.

Pipeline:

```text
Audio Track
    |
    v
Feature Extraction
    |
    v
Analysis Profile
    |
    v
Normalized Musical Signals
    |
    v
Reaction Profile
    |
    v
Visual Parameter Modulation
```

### Analysis Profiles

Examples:

- Techno
- House
- Drum & Bass
- Trap
- Hip-Hop
- Ambient
- Rock
- Metal
- Classical
- Cinematic

They define:

- Frequency emphasis
- Attack / release defaults
- Detector sensitivity
- Relevant musical features
- Smoothing behavior

### Reaction Profiles

Examples:

- Punchy
- Fluid
- Hypnotic
- Minimal
- Chaotic
- Dreamy
- Aggressive
- Slow Burn
- Percussive
- Melodic

This lets a user combine:

```text
Visual: Liquid Chrome
Analysis: Techno
Reaction: Fluid
```

or:

```text
Visual: Galaxy
Analysis: Ambient
Reaction: Dreamy
```

---

## 12. Universal Modulation System

Every important visual parameter should eventually be modulatable.

Possible sources:

```text
Audio:
  sub
  bass
  mids
  highs
  RMS
  transient
  kick
  snare
  hi-hat
  beat phase
  spectral centroid
  spectral flux

Procedural:
  LFO
  noise
  random
  envelope

Project:
  timeline curve
  scene time

Future:
  MIDI
  OSC
```

Each mapping should support:

```text
source
amount
polarity
offset
min
max
attack
release
curve
```

Example:

```text
Source: Bass
Target: Gravity Strength
Amount: 1.4
Attack: 20 ms
Release: 300 ms
Curve: Exponential
```

---

## 13. Editor UX

The editor should initially prioritize usefulness over polish.

Core areas:

```text
Viewport
Preset browser
Inspector
Audio waveform
Timeline
Render panel
```

Visual preset controls should expose a small set of macro parameters first.

Example:

```text
Liquid Chrome

Viscosity
Blob Size
Turbulence
Reflectivity
Audio Energy
Gravity
```

Advanced controls can expose deeper parameters.

Potential playful parameter names are acceptable because this is a personal tool.

Examples:

- Cosmic Violence
- Slime Nervousness
- Bass Gravity
- Stellar Chaos
- Drop Explosion

---

## 14. Preview Modes

Provide three useful preview modes.

### Interactive Preview

Fast continuous playback.

Example target:

```text
1280x720
0.5–2M particles
30–60 fps
```

### Slice Preview

Render a short timeline region at near-final quality.

Example:

```text
5 seconds
1920x1080
full simulation quality
```

### Still Preview

Render a single full-quality frame.

Example:

```text
3840x2160
full particle count
full post-processing
```

---

## 15. Offline Rendering

The final renderer should operate frame-by-frame:

```text
for frame_index in start_frame..end_frame:
    time = frame_index / fps
    sample audio features
    advance deterministic simulation
    render offscreen
    read back final frame
    stream frame to FFmpeg
```

Do not retain rendered frames in memory.

Audio duration should affect render time and output size, not steady-state memory usage.

---

## 16. Headless Renderer

Provide a separate executable:

```text
particle-render
```

Example:

```bash
particle-render project.json \
  --audio track.wav \
  --output output.mp4 \
  --width 3840 \
  --height 2160 \
  --fps 60 \
  --quality ultra
```

The CLI must work without opening a window.

The engine should create a wgpu device without a presentation surface for headless rendering.

---

## 17. Render Configuration

Rendering settings should be declarative.

Example:

```json
{
  "resolution": [3840, 2160],
  "fps": 60,
  "start_seconds": 0,
  "end_seconds": 240,

  "simulation": {
    "particle_multiplier": 8,
    "substeps": 4
  },

  "rendering": {
    "motion_blur_samples": 8,
    "volumetric_quality": "ultra",
    "bloom": "full",
    "supersampling": 1.5
  },

  "encoder": {
    "format": "prores",
    "profile": "422_hq"
  }
}
```

Useful render presets:

- Draft
- Preview
- 4K
- 4K Ultra
- 4K Insane
- 8K Why

---

## 18. Project Format

Projects should serialize all creative state.

Initial format:

```text
project.json
audio reference
asset references
seed
visual preset
parameter overrides
audio profile
reaction profile
modulation mappings
camera settings
timeline automation
render defaults
```

Use serde.

Version the project schema from the beginning.

Example:

```json
{
  "project_version": 1,
  "engine_version": "0.1.0",
  "seed": 93128
}
```

---

## 19. Portable Render Package

For cloud rendering, support a self-contained project package.

Example:

```text
my-track.particleproject/

  project.json
  render.json
  audio.flac

  assets/
    milky-way.exr
    mask.png
    object.glb
```

Local and cloud rendering should consume the same package.

No cloud-specific project representation.

---

## 20. Cloud / RunPod Architecture

Target deployment:

```text
Local Editor
    |
    v
Render Package
    |
    v
GPU Pod / Container
    |
    v
particle-render
    |
    v
wgpu + Vulkan
    |
    v
FFmpeg
    |
    v
output.mp4
```

Initial cloud workflow can be manual:

1. Build Docker image.
2. Start GPU pod.
3. Upload render package.
4. Run `particle-render`.
5. Retrieve output.
6. Terminate pod.

Later, automate job submission from the editor.

The Docker image should contain:

- `particle-render`
- shaders
- FFmpeg
- required Vulkan userspace dependencies
- application assets needed at runtime

Do not couple the core renderer to RunPod APIs.

---

## 21. Local / Cloud Consistency

The same core engine and shaders should run:

```text
Windows:
  wgpu -> DX12 or Vulkan

Linux / RunPod:
  wgpu -> Vulkan
```

Render results should remain visually consistent enough for preview-to-cloud workflows.

The engine should log:

- GPU adapter
- backend
- available limits
- selected render settings
- particle count
- VRAM-relevant allocations where practical

---

## 22. Quality Scaling

Quality should be explicit and deterministic.

Example:

```text
Preview:
  first 500k deterministic particles

Final:
  first 8M deterministic particles
```

Additional particles should extend the preview population rather than regenerate an unrelated simulation.

Potential quality controls:

- particle count
- simulation substeps
- volumetric resolution
- bloom resolution
- trail history
- raymarch steps
- motion blur samples
- supersampling factor

---

## 23. Camera System

Support:

- Orbit
- Dolly
- Static
- Look-at
- Spline path
- Slow procedural drift
- Audio-reactive shake
- Audio-reactive FOV
- Depth of field

Camera parameters should be modulatable.

---

## 24. Environment / Asset Inputs

Eventually support images, HDRIs, video, and meshes as simulation/render inputs.

Possible roles:

```text
environment reflection
background
particle color source
emission mask
attraction map
displacement source
lighting source
surface emitter
shape target
```

Initial mesh formats:

- GLTF
- GLB

---

## 25. Advanced Future Systems

Do not implement initially, but keep architecture compatible with:

- SPH fluids
- Position-based fluids
- Marching cubes
- Metaballs
- Signed distance fields
- Reaction-diffusion
- GPU spatial hashing
- Boids
- Volumetric raymarching
- Particle trails
- GPU sorting
- Density grids
- Mesh dissolution
- Shape targets
- Multi-layer simulations
- Preset morphing
- Parameter mutation / randomization
- Seamless loop generation
- Render passes:
  - beauty
  - alpha
  - depth
  - normals
  - motion vectors
  - emission
  - IDs

---

## 26. Coding Rules

These rules are important for both humans and coding agents.

### Engine

- Rust owns simulation state.
- GPU owns particle state whenever practical.
- Avoid CPU/GPU readback except when required.
- Never represent large particle collections as heap objects.
- Simulation must be deterministic.
- Simulation time must not use wall-clock time.
- Simulation must not depend on render resolution.
- Preview and export use the same render core.

### Architecture

- Tauri UI must not contain engine logic.
- Renderer must work without a window.
- Headless CLI must use the same engine as the editor.
- Presets must be serialized data where possible.
- Audio analysis must be precomputed.
- Cloud rendering must consume the same project format as local rendering.

### Performance

- Reuse GPU resources.
- Avoid per-frame buffer/texture creation.
- Avoid unnecessary GPU readback.
- Profile before optimizing.
- Prefer compute shaders for massively parallel work.
- Avoid naive O(N^2) simulation algorithms.

### Code Quality

- Keep modules small and explicit.
- Prefer readable Rust over clever Rust.
- Avoid `unsafe` unless clearly justified.
- Use `Result` and structured errors.
- Document GPU buffer layouts.
- Keep Rust and WGSL structs visibly synchronized.
- Add comments explaining non-obvious GPU synchronization or memory layout decisions.

---

## 27. Agent Usage Rules

To conserve coding-agent tokens:

1. Treat this document as the architectural source of truth.
2. Give the agent one milestone or subsystem at a time.
3. Avoid prompts like:
   - "build the whole app"
   - "finish the renderer"
   - "make this production ready"
4. Prefer prompts like:
   - "Implement milestone 1 only."
   - "Add one compute shader that updates particle position from velocity."
   - "Do not add audio, presets, post-processing, or export yet."
5. Require the agent to:
   - inspect existing architecture first
   - make the smallest coherent change
   - avoid unrelated refactors
   - run tests/checks after changes
   - summarize changed files
6. Keep stable design decisions in this document rather than repeating them in every prompt.
7. Add short subsystem-specific docs when complexity grows.

Useful supporting files later:

```text
DESIGN.md
AGENTS.md
docs/
  GPU.md
  PROJECT_FORMAT.md
  AUDIO.md
  RENDERING.md
```

`AGENTS.md` should contain concise coding-agent instructions, while `DESIGN.md` contains the long-form architecture.

---

## 28. Development Milestones

### Milestone 0 — Environment

Goal:

```text
Rust compiler works
Node works
Tauri app launches
wgpu detects RTX 4070
```

No particles.

### Milestone 1 — GPU Smoke Test

Render a triangle or clear color using wgpu.

Verify:

- GPU adapter
- backend
- swapchain
- shader compilation

### Milestone 2 — First Compute Particles

Goal:

```text
10,000 particles
position + velocity
one compute update
one render pass
```

No audio.

No preset system.

No post-processing.

### Milestone 3 — Scale Test

Benchmark:

```text
100k
1M
2M
5M
```

Record:

- FPS
- GPU utilization
- memory use
- simulation time
- render time

### Milestone 4 — Basic Forces

Add:

- gravity
- point attractor
- vortex/orbit force
- noise field

### Milestone 5 — Deterministic Timeline

Implement:

```text
frame_index
fps
fixed simulation timestep
seeded initialization
```

Verify repeatable frames.

### Milestone 6 — Audio Analysis

Load a WAV/FLAC file.

Compute:

- RMS
- several frequency bands
- transient strength

Cache analysis.

### Milestone 7 — Modulation

Map:

```text
bass -> force strength
highs -> color/emission
transient -> burst
```

### Milestone 8 — Project Serialization

Save/load:

```text
seed
particle system
parameters
audio path
modulation mappings
```

### Milestone 9 — Headless Still Render

Render one offscreen image from CLI.

Example:

```bash
particle-render project.json --frame 1000 --output frame.png
```

### Milestone 10 — Video Export

Render frame sequence directly into FFmpeg.

First target:

```text
1920x1080
30 fps
H.264
```

### Milestone 11 — 4K Export

Add:

```text
3840x2160
60 fps
HDR intermediate
high-quality bloom
```

### Milestone 12 — Visual Preset Framework

Implement first presets:

1. Star System
2. Nebula
3. Liquid Chrome
4. Green Slime
5. Water Droplets

They may initially use simplified visual approximations.

### Milestone 13 — Audio Profiles

Implement:

- Techno
- DnB
- Ambient
- Cinematic

### Milestone 14 — Docker / Headless Cloud

Build Linux container.

Verify:

```text
wgpu Vulkan adapter
offscreen render
FFmpeg export
```

Then test on a rented GPU.

---

## 29. First Success Definition

The first meaningful end-to-end version is complete when:

1. A song can be loaded.
2. Audio features are analyzed.
3. One particle scene can be previewed.
4. Bass and transients can modulate two visual parameters.
5. The project can be saved.
6. A deterministic 1080p video can be rendered via CLI.
7. The same project renders through the desktop app and the headless renderer.

Everything else is iteration.

---

## 30. Guiding Philosophy

This project should optimize for:

```text
creative experimentation
GPU scale
deterministic rendering
portable headless execution
learnable architecture
```

Not:

```text
premature abstraction
enterprise patterns
cross-platform perfection on day one
commercial UX polish
```

When uncertain, prefer the design that makes it easier to:

1. Add a new visual system.
2. Add a new modulation source.
3. Render the same project headlessly.
4. Scale particle count upward.
5. Understand what the GPU is doing.
