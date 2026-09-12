# Audio Profiles

Rustique keeps music interpretation separate from visual behavior. Analysis
profiles in `profiles/analysis` weight the normalized offline audio features.
Reaction profiles in `profiles/reaction` provide ordinary universal modulation
mappings plus shared attack and release defaults.

A project selects either profile by a path relative to the project file:

```json
"analysis_profile": {
  "source": "../profiles/analysis/techno.json",
  "overrides": { "sensitivity": 1.2 }
},
"reaction_profile": {
  "source": "../profiles/reaction/fluid.json",
  "overrides": { "release_seconds": 0.8 }
}
```

Analysis overrides may replace `sensitivity` or the complete
`frequency_weights` object. Reaction overrides may replace attack, release, or
the recommended mapping list. Profile mappings are copied into the project's
standard modulation list at load time, so render paths continue to use the
same deterministic modulation evaluator.
