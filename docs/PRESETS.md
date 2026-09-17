# Visual presets

Visual presets are versioned JSON files under `presets/`. Each preset contains
particle/emitter defaults, an ordered force stack, camera and render defaults,
macro parameter definitions, and recommended universal modulation mappings.

A project selects a preset with a path relative to the project file:

```json
"visual_preset": {
  "source": "../presets/nebula.json",
  "overrides": {
    "macros": { "turbulence": 1.4, "particle_size": 5.0 }
  }
}
```

Loading the project validates the preset and override names/ranges, applies its
defaults, then applies project-local macro values. Presets use the same emitter,
force, camera, modulation, particle-render, and post-processing systems.

Particle presets can add camera-space depth cues through four render defaults:
`particle_depth_near` and `particle_depth_far` define the response range,
`particle_depth_size_strength` controls an art-directed near-large/far-small
scale, and `particle_depth_brightness_strength` dims particles across that
range. Atmospheric tint/desaturation, additive glow, focus distance/range, and
subtle particle bokeh are configured by the other `particle_depth_*`,
`particle_glow_strength`, and `particle_*focus*`/`particle_dof_strength`
fields. Normalized strengths use `0` to disable the effect. Omitted fields
retain the original constant-size, constant-color behavior.

Non-translucent particle systems render circular alpha-tested cores into a
reusable depth buffer. Optional glow is a second additive, read-only depth pass,
so distant particles cannot overwrite nearer cores while halos remain soft.

The initial set is Star System, Nebula, Liquid Chrome, Green Slime, and Water
Droplets. The material-heavy presets are particle-based visual prototypes until
later material and surface-rendering phases.
