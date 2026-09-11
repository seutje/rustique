# Offline Audio Analysis

Rustique decodes complete tracks with Symphonia and converts channel-interleaved
samples to deterministic mono input for analysis. Enabled formats are WAV, FLAC,
MP3, and AAC in MP4. Symphonia is pure Rust and keeps the core audio crate
cross-platform without an FFmpeg runtime dependency.

The default analyzer uses a 2048-sample Hann window and 512-sample hop. It stores
waveform min/max buckets plus normalized RMS, six frequency bands, spectral
centroid, positive spectral flux, and transient strength for every window.
Feature lookup selects the nearest precomputed window by timestamp.

Analysis is stored beside the source as `<audio>.rustique-analysis.json`. Cache
freshness includes a cache-format version, source byte size, source modification
timestamp, and analysis configuration. A stale cache is recomputed.

Inspect a track and a timestamp with:

```bash
cargo run -p particle-render --release -- audio-info track.flac --time 12.5
```
