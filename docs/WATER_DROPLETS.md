# Water Droplets

Use `render_mode: "water_droplets"` for the deterministic, headless condensation renderer.
The `water_droplets` object controls `density`, `size`, `size_variation`,
`refraction_strength`, `fresnel_strength`, `gravity`, and `emission`. Values are
validated by `project-format`; `size` must be positive, variation and emission are
0–1, and the remaining values are non-negative.

Droplets are procedurally instanced in screen-space cells. Their seeded size and
fall speed are stable, and their vertical position uses `frame / fps`, so a frame
does not depend on render history or wall-clock time. `render_frame` accepts an
additional emission modulation value intended for the existing `burst_emission`
audio target. The condensation preset recommends transient-driven emission.

Render the example at two moments:

```powershell
cargo run -p particle-render -- still examples/water-droplets.rustique.json --frame 0 --output droplets-0.png
cargo run -p particle-render -- still examples/water-droplets.rustique.json --frame 180 --output droplets-180.png
```
