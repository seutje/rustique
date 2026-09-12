# Production render resume investigation

The current raw-frame-to-FFmpeg stream cannot safely resume inside an unfinished container. A crash may leave an undecodable partial GOP or incomplete container index.

The recommended future strategy is segmented rendering: encode independently reproducible, frame-aligned chunks with a manifest, validate every chunk, then concatenate them without re-encoding. Resume can skip chunks whose settings and project fingerprints match. This should be implemented after render packages provide stable asset fingerprints; Phase 21 deliberately does not present an unsafe resume button.
