# Docker Renderer

The Docker image runs the same `particle-render` binary used locally. It has no
RunPod-specific API or configuration. Render packages and results are passed
through the conventional `/input` and `/output` mount points.

## Build

From the repository root:

```bash
docker build --tag rustique-renderer:local .
```

The image is built in two stages with Rust 1.88, the minimum supported by the
current locked dependency set. The final Debian image contains the Vulkan
loader, Mesa's Vulkan drivers (useful for software or non-NVIDIA adapters), and
FFmpeg. An NVIDIA container runtime supplies the NVIDIA driver and Vulkan ICD
from the host; the image does not bundle a GPU driver.

## Verify GPU passthrough

Install a current NVIDIA driver, Docker, and the NVIDIA Container Toolkit on the
Linux or WSL host. Then run:

```bash
docker run --rm --gpus all rustique-renderer:local --backend vulkan --gpu-info
```

The output should name a Vulkan GPU rather than a CPU/software adapter. GPU
passthrough is a host prerequisite; `--gpus all` is intentionally not baked
into the image.

## Mounted render package

The examples below assume the host has a portable package at
`$PWD/input/demo.rustiqueproject`. Paths after the image name are container
paths.

Render a still:

```bash
docker run --rm --gpus all \
  --mount type=bind,src="$PWD/input",dst=/input,readonly \
  --mount type=bind,src="$PWD/output",dst=/output \
  rustique-renderer:local --backend vulkan \
  still /input/demo.rustiqueproject --output /output/frame.png --frame 0
```

Render a short video using the audio embedded in the package:

```bash
docker run --rm --gpus all \
  --mount type=bind,src="$PWD/input",dst=/input,readonly \
  --mount type=bind,src="$PWD/output",dst=/output \
  rustique-renderer:local --backend vulkan \
  video /input/demo.rustiqueproject --output /output/clip.mp4 --frames 60
```

Validate the results on the host:

```bash
test -s output/frame.png
ffprobe -v error -show_entries stream=codec_name,width,height \
  -of default=noprint_wrappers=1 output/clip.mp4
```

Use an absolute host path in `src=` if the shell does not expand `$PWD` as
expected. The mounted output directory must be writable by Docker.
