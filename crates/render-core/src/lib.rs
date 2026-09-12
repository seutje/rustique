//! Shared, window-independent rendering infrastructure.

mod camera;
mod gpu;
mod offscreen;
mod particles;
mod post_process;

pub use camera::PerspectiveCamera;
pub use gpu::{BackendPreference, GpuConfig, GpuContext, GpuInfo, GpuInitError};
pub use offscreen::{OffscreenError, OffscreenRenderTarget, RgbaColor};
pub use particles::{
    BenchmarkConfig, FrameTiming, FrameUniforms, ParticleRenderError, ParticleRenderer,
};
pub use post_process::{PostProcessConfig, PostProcessQuality};
