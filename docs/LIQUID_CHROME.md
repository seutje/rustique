# Liquid Chrome

Liquid Chrome is a headless GPU ray-marched, smoothly blended metaball surface. Normals are generated from the signed-distance field and feed metallic environment shading. It is selected with `"render_mode": "liquid_chrome"`.

The `liquid_chrome` project/preset object supports:

- `environment`: optional equirectangular `.hdr`, PNG, or JPEG path (relative to the project for `particle-render still`)
- `environment_preset`: `"milky_way"` when no image is supplied
- `roughness`: 0–1
- `reflection_intensity`: non-negative
- `metallic`: 0–1
- `surface_scale`: positive metaball scale

Material automation/audio mappings use `material_roughness`, `reflection_intensity`, and `surface_scale`. Offline sequence rendering evaluates these through the existing deterministic precomputed-audio modulation path.

Render the bundled preset:

```powershell
cargo run -p particle-render -- still examples/liquid-chrome.rustique.json --output liquid-chrome.png --width 1280 --height 720 --frame 30 --post-quality preview
```

To test an HDRI, copy the bundled preset, set its `liquid_chrome.environment` to an equirectangular image, and point the example's `visual_preset.source` at that copy. Frame time is derived from `frame / fps`; repeated renders of the same inputs are deterministic to the practical limits of the selected GPU backend.
