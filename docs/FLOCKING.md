# GPU Flocking

Rustique's flocking system is an optional particle-system feature. It keeps the
64-byte particle state GPU-resident and approximates neighborhoods with a coarse
3D aggregate field.

## GPU passes

Every simulation substep performs four flocking compute passes before the
shared force/integration pass:

1. Clear the aggregate cells.
2. Deterministically claim sparse hash buckets for logical cells.
3. Atomically splat particle count, position, and velocity into winning cells.
4. Sample the current cell and six axial neighbors to derive separation,
   alignment, cohesion, and density-gradient steering.

The field uses conservative fixed-point signed integer sums because WebGPU does
not require floating-point atomics. A 32³ grid occupies 1 MiB. Logical grids up
to 1024³ use keyed sparse hash buckets capped at 64 MiB; colliding distant
cells are dropped rather than mixed. Work is O(particles + buckets), and no
particle data is read back to the CPU.

Above roughly 900K particles, a deterministic subset is accumulated as field
leaders while every particle continues to sample and follow the field. This
hierarchical mode bounds fixed-point atomic sums and contention without changing
the visible particle count.

`grid_resolution` is the main quality control. The default 32³ grid is intended
for 100K–2M particles. Higher logical resolutions give finer local motion; once
the dense grid would exceed the bounded bucket capacity, sparse hashing keeps
clear cost and memory stable. `neighborhood_radius` selects a one- or two-cell
sampling offset; it does not trigger variable-length neighbor scans.

## Motion model

The local field provides:

- separation from excess local density and the local density gradient;
- alignment toward average local velocity;
- cohesion toward average local position.

These independent weights are combined with an analytic divergence-free curl
field, deterministic per-particle variation, directional flow, moving global or
per-particle attractors, optional repulsors, soft boundary steering, inertia,
drag, and velocity/steering clamps. Setting any strength to zero disables that
component without changing the pipeline or project representation.

Murmuration mode cycles through cohesive flock, elongated stream, turbulent
vortex, expansion, multi-attractor split, and reconvergence profiles. State
weights use smooth interpolation during the configured transition interval and
derive entirely from deterministic simulation time.

The cinematic preset starts face-on but uses a wider initial Z distribution,
Z-separated attractors, and a small Z directional bias. As its orbit progresses,
depth-tested particle cores, atmospheric color, and particle bokeh reveal the
flock's volume without changing the initial camera pose.

## Audio mappings

The preset maps audio features through the existing attack/release envelope
system:

| Audio feature | Flocking target | Effect |
| --- | --- | --- |
| Bass | `flocking_separation` | short outward pressure |
| Low mids | `flocking_cohesion` | contraction and expansion |
| Mids | `flocking_turbulence` | curl-field energy |
| RMS | `flocking_speed` | overall velocity ceiling |
| Highs | `flocking_randomness` | rapid individual steering variation |
| Transient | `flocking_impulse` | outward travelling impulse wave |

See `presets/murmuration.json` for every exposed parameter and
`examples/murmuration.rustique.json` for a directly renderable project.
