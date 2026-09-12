# Render Packages

Rustique render packages are portable directories conventionally named
`*.rustiqueproject`:

```text
example.rustiqueproject/
  manifest.json
  project.json
  render.json
  assets/
    audio/
    project/
    textures/
    hdris/
    meshes/
```

`project.json` contains package-relative references to any selected visual,
analysis, and reaction profiles. `render.json` contains the package-relative
audio path and default output dimensions. `manifest.json` versions the package
layout and records a SHA-256 fingerprint for every other file.

Create a package:

```powershell
cargo run -p particle-render -- package-create examples/star-orbit.rustique.json `
  --audio C:\media\track.wav `
  --output C:\renders\star-orbit.rustiqueproject
```

Optional `--width` and `--height` values override the project's render
defaults. Repeat `--texture`, `--hdri`, or `--mesh` to bundle assets that are
not yet represented by the version-1 project schema.

Validate after copying or uploading:

```powershell
cargo run -p particle-render -- package-validate C:\renders\star-orbit.rustiqueproject
```

Render directly from the package (no original project or audio path is used):

```powershell
cargo run -p particle-render -- still C:\renders\star-orbit.rustiqueproject `
  --output frame.png --frame 0

cargo run -p particle-render -- sequence C:\renders\star-orbit.rustiqueproject `
  --output-dir frames --frames 60

cargo run -p particle-render -- video C:\renders\star-orbit.rustiqueproject `
  --output render.mp4 --frames 60
```

Package loading rejects absolute paths, path traversal, missing files,
unmanifested dependencies, and files whose fingerprints have changed.
