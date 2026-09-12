# syntax=docker/dockerfile:1

FROM rust:1.88-bookworm AS builder

WORKDIR /build
COPY Cargo.toml Cargo.lock ./
COPY crates ./crates
COPY tools ./tools
COPY apps/desktop/src-tauri ./apps/desktop/src-tauri
COPY gpu-smoke ./gpu-smoke
COPY shaders ./shaders

RUN cargo build --locked --release -p particle-render

FROM debian:bookworm-slim AS runtime

RUN apt-get update \
    && apt-get install --yes --no-install-recommends \
        ca-certificates \
        ffmpeg \
        libvulkan1 \
        mesa-vulkan-drivers \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /opt/rustique
COPY --from=builder /build/target/release/particle-render /usr/local/bin/particle-render
COPY --chmod=755 docker/entrypoint.sh /usr/local/bin/rustique-entrypoint
COPY shaders ./shaders
COPY presets ./presets
COPY profiles ./profiles

ENV RUSTIQUE_INPUT_DIR=/input \
    RUSTIQUE_OUTPUT_DIR=/output \
    WGPU_BACKEND=vulkan

VOLUME ["/input", "/output"]
ENTRYPOINT ["/usr/local/bin/rustique-entrypoint"]
CMD ["--help"]
