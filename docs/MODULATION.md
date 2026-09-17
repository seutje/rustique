# Universal Modulation

Projects store mappings independently from presets and UI code. Each mapping has
an audio source, typed visual target, amount, offset, output range, polarity,
curve, combination mode, and attack/release durations. `replace` preserves the
original absolute behavior, `multiply` scales the current preset/macro value,
and `add` applies an offset. Multiple mappings still combine in serialized
order.

At each fixed analysis step, the source is smoothed with deterministic
attack/release coefficients, optionally inverted, transformed by a linear or
quadratic exponential curve, scaled, offset, and clamped. Mappings are evaluated
in serialized list order.

The first GPU targets are force scale, particle size, brightness, and active
particle count for burst emission. The sample project demonstrates bass to
gravity, highs to brightness, and transients to burst emission.

Star System and Murmuration use multiplicative particle size/brightness
mappings so audio changes the overall response without replacing per-particle
depth scaling.

Inspect evaluated values without rendering:

```bash
cargo run -p particle-render --release -- modulation-info examples/star-orbit.rustique.json track.flac --time 12.5
```
