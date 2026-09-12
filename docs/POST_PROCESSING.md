# Post-processing

`OffscreenRenderTarget` owns the shared post-processing pipeline used by still,
PNG-sequence, and video rendering. Particle color is first rendered into an
`Rgba16Float` scene texture. A temporal pass combines that scene with one of
two reusable HDR history textures, and a final pass applies bloom, exposure,
tone mapping, vignette, optional chromatic aberration, and the gamma transform.

No textures are allocated per frame. At 3840x2160, the three HDR textures and
one RGBA8 output texture use about 221.5 MiB. The separate readback buffer uses
about 31.6 MiB.

Use `--post-quality draft|preview|final` with project still, sequence, and video
commands. Draft disables optional effects, Preview enables tone mapping and
bloom, and Final also enables temporal trails and vignette. Library callers can
start with `PostProcessConfig::for_quality` and toggle each effect independently.

Timeline renders clear temporal history at frame zero and whenever the particle
renderer seeks backward. Forward frame sequences retain history deterministically.
