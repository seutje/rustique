//! Shared, window-independent rendering infrastructure.

mod camera;
mod fluid;
mod gpu;
mod offscreen;
mod particles;
mod post_process;
mod spatial_grid;
mod surface;

pub use camera::PerspectiveCamera;
pub use fluid::{FluidConfig, FluidError, FluidFrameStats, FluidRenderer};
pub use gpu::{BackendPreference, GpuConfig, GpuContext, GpuInfo, GpuInitError};
pub use offscreen::{OffscreenError, OffscreenRenderTarget, RgbaColor};
pub use particles::{
    BenchmarkConfig, FrameTiming, FrameUniforms, ParticleRenderError, ParticleRenderer,
};
pub use post_process::{PostProcessConfig, PostProcessQuality};
pub use spatial_grid::{SpatialGrid, SpatialGridConfig, SpatialGridError, SpatialGridStats};
pub use surface::SurfacePresenter;
