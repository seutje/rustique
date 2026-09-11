# Universal Modulation

Projects store mappings independently from presets and UI code. Each mapping has
an audio source, typed visual target, amount, offset, output range, polarity,
curve, and attack/release durations.

At each fixed analysis step, the source is smoothed with deterministic
attack/release coefficients, optionally inverted, transformed by a linear or
quadratic exponential curve, scaled, offset, and clamped. Mappings are evaluated
in serialized list order.

The first GPU targets are force scale, particle size, brightness, and active
particle count for burst emission. The sample project demonstrates bass to
gravity, highs to brightness, and transients to burst emission.

Inspect evaluated values without rendering:

```bash
cargo run -p particle-render --release -- modulation-info examples/star-orbit.rustique.json track.flac --time 12.5
```
