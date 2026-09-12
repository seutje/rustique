# Multi-layer scenes

Projects may declare a `layers` array. Each layer owns its render mode, particle
count/substeps, forces, material settings, modulation mappings, visibility,
opacity, blend mode (`alpha`, `add`, or `screen`), depth, and quality scale.
Layers render in ascending `depth`; equal-depth layers retain JSON order.

`quality_scale` scales the deterministic particle prefix, so previews can reduce
individual systems without changing ordering or seeds. Hidden layers allocate no
renderer resources. An empty or absent array uses the legacy single-system path.

The initial headless compositor supports particle, volumetric, and water-droplet
layers. Layer mappings serialize with the layer; audio evaluation is used by the
timeline/export path as that path adopts layered rendering. The still command has
no audio feature input, so it renders base layer values.

Try the included four-layer scene:

```powershell
cargo run -p particle-render -- still examples/multi-layer.rustique.json --frame 90 --post-quality preview --output multi-layer.png
```
