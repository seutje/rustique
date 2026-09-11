//! Shared, window-independent rendering infrastructure.

mod gpu;
mod offscreen;

pub use gpu::{BackendPreference, GpuConfig, GpuContext, GpuInfo, GpuInitError};
pub use offscreen::{OffscreenError, OffscreenRenderTarget, RgbaColor};
