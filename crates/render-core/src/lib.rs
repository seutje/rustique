//! Shared, window-independent rendering infrastructure.

mod gpu;
mod offscreen;
mod particles;

pub use gpu::{BackendPreference, GpuConfig, GpuContext, GpuInfo, GpuInitError};
pub use offscreen::{OffscreenError, OffscreenRenderTarget, RgbaColor};
pub use particles::{FrameUniforms, ParticleRenderError, ParticleRenderer};
