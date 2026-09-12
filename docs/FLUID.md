# GPU Fluid / Slime Prototype

Phase 28 uses a weakly-compressible SPH-style solver. Particle positions and
velocities remain GPU-resident. Each fixed step clears and builds a 3D uniform
grid, reconstructs a scalar density from nearby particles, then applies a
density-pressure term, velocity viscosity, and neighborhood-center cohesion.
The solve is bounded by `cell_capacity` and `max_neighbors`, so it does not
devolve into an O(N^2) all-pairs pass.

`audio_pressure` multiplies the density response and `audio_turbulence` adds a
deterministic per-particle impulse. They are explicit samples rather than audio
decoding in the renderer, allowing the existing offline audio cache to drive
them later without introducing wall-clock state.

## Rendering experiments

The diagnostic renderer draws density-shaded green particles directly from the
solved GPU buffer. This is deliberately the usable fallback for 4K offline
output. The density buffer is also the first 3D density-reconstruction
experiment: it reconstructs density at every moving sample rather than
allocating a dense volume texture.

Two surface approaches were evaluated for the next rendering step:

- GPU marching cubes/metaballs can produce a true mesh but requires a second
  3D density lattice, prefix-sum/compaction, generated vertex buffers, and a
  topology budget. It is a poor default for one million particles.
- Screen-space fluid rendering (depth/thickness splats, bilateral smoothing,
  and normal reconstruction) scales with pixels and is the preferred next
  production surface path. It should remain optional because it adds several
  full-frame passes at 4K.

## Measured smoke profile

Measured on the development RTX 4070 using debug builds. Times include GPU
submission, density diagnostic readback, final image readback, and PNG-related
frame preparation, so they are conservative rather than pure compute timings.

| Particles | Grid | Output | Final frame | GPU buffers |
| ---: | ---: | ---: | ---: | ---: |
| 100,000 | 32³ | 3840×2160 | 13.318 ms | 14.61 MiB |
| 500,000 | 48³ | 640×360 | 21.383 ms | 59.85 MiB |
| 1,000,000 | 64³ | 640×360 | 35.916 ms | 129.85 MiB |

The 100k 4K smoke test demonstrates that the headless fallback is usable for
offline rendering. Production exports should disable per-frame diagnostics;
the prototype currently reads density and final pixels back for measurement.

## CLI

```powershell
cargo run -p particle-render -- fluid --output target/slime.png --count 100000 --frames 120 --width 1920 --height 1080
```

Use `--pressure`, `--viscosity`, and `--cohesion` for the material response.
Use `--audio-pressure` and `--audio-turbulence` to emulate cached audio feature
samples. `--cells` controls the 3D grid resolution.

On Windows, automatic GPU selection prefers DX12. The Vulkan driver on the
development Windows machine exits with `STATUS_ACCESS_VIOLATION` during device
use; Vulkan remains the preferred backend on Linux/WSL. Pass `--backend dx12`
explicitly when diagnosing a Windows driver setup.
