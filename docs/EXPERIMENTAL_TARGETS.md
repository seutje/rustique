# Experimental targets, inputs, and render passes

The headless `target` command turns a procedural shape, GLTF/GLB vertex stream,
SVG image, or font-rendered text into deterministic GPU particle state. The
`--dissolution` value blends the target toward Rustique's seeded default cloud.

```powershell
cargo run -p particle-render -- target --shape sphere --output target.png --count 100000
cargo run -p particle-render -- target --mesh model.glb --output mesh.png --dissolution 0.35
cargo run -p particle-render -- target --svg logo.svg --output logo.png
cargo run -p particle-render -- target --text RUSTIQUE --font C:\Windows\Fonts\arial.ttf --output text.png
```

Add `--passes` to write `.alpha.png`, `.depth.png`, `.normals.png`,
`.motion-vectors.png`, and `.emission.png`. These experimental passes are
deterministic screen-space reconstructions from the rendered particle field;
they are useful for compositing but are not geometric G-buffer passes. Supply
`--exr output.exr` for a linear floating-point RGBA OpenEXR copy.

The desktop inspector's **Live input** section scans MIDI inputs and reports
normalized control-change values as `midi.cc.N`. The OSC listener accepts UDP
messages with a numeric first argument and reports them by address, such as
`osc/energy`. Live input intentionally affects preview/editor state only;
offline determinism requires baking values into automation tracks.

Transparent video uses `prores4444`. Set the project background alpha to zero,
disable effects that intentionally accumulate opaque content, and select
**ProRes 4444 + alpha** in Export or use `--codec prores4444` in the CLI.
