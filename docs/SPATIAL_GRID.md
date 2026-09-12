# GPU Spatial Grid

Phase 27 adds a reusable, headless uniform grid in `render-core`. Particles are assigned to fixed-size 3D cells on the GPU. A query checks the particle's cell and its 26 adjacent cells, returning a bounded neighbor count without reading particle state back during the computation.

## Complexity

Grid clearing is `O(C)`, insertion is `O(N)`, and queries are approximately `O(N * K)`, where `C` is the number of cells and `K` is the number of candidates in 27 nearby cells. For a reasonably distributed population and grid resolution, `K` remains far below `N`; this avoids the `O(N^2)` all-pairs approach.

## Limits

- Each cell has fixed storage. Particles beyond `cell_capacity` are counted as overflow and omitted from queries. A nonzero overflow metric means capacity or grid resolution should be increased.
- Neighbor results are capped by `max_neighbors`, suitable for later boid/fluid kernels with bounded work.
- Cell size is derived from the X extent. Current bounds should therefore be cubic.
- Atomic insertion order is not deterministic across GPU invocations. Neighbor counts are deterministic when cells do not overflow and counts do not hit their cap, but future order-dependent solvers must sort or use order-independent accumulation.
- Memory is dominated by `cells_per_axis^3 * cell_capacity * 4` bytes. The constructor rejects an entries buffer beyond the adapter's storage-binding limit.
- The benchmark includes GPU submission, completion, and diagnostic readback. It is intended as an end-to-end stress measurement, not a pure timestamp-query measurement.

## Testing

Run a basic query and create a debug heatmap:

```powershell
cargo run -p particle-render -- spatial-benchmark --count 100000 --cells 32 --iterations 3 --debug-output grid.png
```

Then stress memory and scaling with the same grid density:

```powershell
cargo run -p particle-render --release -- spatial-benchmark --count 1000000 --cells 64 --cell-capacity 64 --max-neighbors 128 --iterations 5
```

Watch `overflow`: it should be zero for complete neighbor coverage. Compare 100k, 500k, and 1M runs; elapsed time should grow much closer to linearly than quadratically when occupancy remains controlled.
