# Rustique GPU Benchmarks

## Baseline — 2026-09-11

Hardware and backend:

- NVIDIA GeForce RTX 4070 12 GB
- DX12
- Driver 32.0.15.9186
- 1920x1080 RGBA8 target
- 20 measured frames, fixed 60 Hz simulation step
- 2 px camera-facing particle quads

Reproduce with:

```powershell
cargo run -p particle-render --release -- benchmark --frames 20 --width 1920 --height 1080
```

| Particles | Ping-pong buffers | CPU prepare | GPU compute | GPU render | Total GPU |
|---:|---:|---:|---:|---:|---:|
| 100k | 12.21 MiB | 0.1035 ms | 0.0245 ms | 0.0388 ms | 0.0633 ms |
| 500k | 61.04 MiB | 0.0678 ms | 0.1155 ms | 0.1736 ms | 0.2891 ms |
| 1M | 122.07 MiB | 0.0796 ms | 0.2555 ms | 0.3359 ms | 0.5914 ms |
| 2M | 244.14 MiB | 0.1149 ms | 0.5564 ms | 0.6615 ms | 1.2179 ms |
| 5M | 610.35 MiB | 0.1558 ms | 1.3897 ms | 1.6419 ms | 3.0317 ms |
| 10M | 1220.70 MiB | 0.1767 ms | 2.8956 ms | 3.2754 ms | 6.1710 ms |
| 20M | 2441.41 MiB | 0.1859 ms | 5.7086 ms | 5.9035 ms | 11.6122 ms |

These timings exclude deterministic initialization and final texture readback. A
separate 1M test with `--readback` measured 2.4970 ms for the complete render and
RGBA8 readback path, versus 0.5912 ms of GPU compute plus raster work.

## Particle size and overdraw

Reproduce a row by adding `--count 1000000 --particle-size N`; add `--overdraw`
to concentrate particles into two percent of the normal screen area.

| Scenario | GPU compute | GPU render | Total GPU |
|---|---:|---:|---:|
| 1 px, spread | 0.2656 ms | 0.2362 ms | 0.5018 ms |
| 4 px, spread | 0.2515 ms | 0.3820 ms | 0.6335 ms |
| 16 px, spread | 0.2566 ms | 2.2128 ms | 2.4694 ms |
| 16 px, concentrated overdraw | 0.2545 ms | 3.9478 ms | 4.2023 ms |

## Interpretation

The baseline is roughly balanced between compute and raster work at high counts.
Particle-size scaling and the concentrated test show that fragment overdraw
becomes the clear bottleneck for large particles. Buffer bandwidth and vertex
processing scale close to linearly in this deliberately simple pipeline. Final
RGBA8 readback is a significant offline cost but is excluded from interactive
preview. One million particles is comfortably below a 16.67 ms interactive
frame budget, and the 20M case remains viable for offline experimentation while
using about 2.38 GiB for simulation state.

Results are workload- and driver-specific and should be rerun after material,
post-processing, or force complexity changes.
