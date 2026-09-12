# Volumetric rendering

Projects select the GPU volumetric path with `"render_mode": "volumetric"`. The Nebula preset enables it by default. Particle positions are splatted into a reusable atomic 3D density grid, then a fullscreen raymarch integrates colored emission and exponential light absorption. Sequence and video rendering reuse the existing deterministic frame history for temporal accumulation.

The CLI `--post-quality` option also selects volumetric quality:

| Quality | Density grid | Ray steps | Sampling |
| --- | ---: | ---: | --- |
| `draft` | 32³ | 24 | half-resolution |
| `preview` | 48³ | 48 | half-resolution |
| `final` | 96³ | 128 | full-resolution |

Render a comparison:

```powershell
cargo run -p particle-render -- still examples/nebula.rustique.json --output nebula-preview.png --width 1280 --height 720 --post-quality preview
cargo run -p particle-render -- still examples/nebula.rustique.json --output nebula-final.png --width 1280 --height 720 --post-quality final
```

For temporal accumulation and deterministic animation, render a sequence with an audio file:

```powershell
cargo run -p particle-render -- sequence examples/nebula.rustique.json --audio path/to/track.wav --output-dir nebula-frames --frames 60 --post-quality preview
```
