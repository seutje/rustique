# Interactive Viewport

The Windows editor embeds a native child `HWND` over the viewport rectangle in
the Tauri WebView2 layout. wgpu presents directly to that child window. Frames
remain on the GPU: `render-core` renders through the same particle simulation
and post-processing path used for offline output, then blits the final texture
to the surface swapchain.

The Win32 handle code is isolated in `apps/desktop/src-tauri/src/viewport.rs`.
Its unsafe invariants are documented next to the calls: the Tauri parent handle
outlives the child, and the render thread and wgpu surface are stopped before
the child handle is destroyed.

React reports the viewport's physical bounds after layout and resize. Native
commands provide play/pause, deterministic seek/reset, preview quality, cached
audio analysis, and diagnostics. Draft and Preview allocate deterministic
prefixes of at most 100,000 and 500,000 particles; Final uses the project count.

GPU timestamp values remain unavailable in the initial surface path. The UI
shows this explicitly as `GPU — ms`; FPS, CPU frame time, particle count, and
timeline frame are available from the preview status command.
