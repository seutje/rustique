//! Shared, window-independent rendering infrastructure.

mod camera;
mod fluid;
mod gpu;
mod liquid_chrome;
mod offscreen;
mod particles;
mod post_process;
mod spatial_grid;
mod surface;
mod volumetrics;
mod water_droplets;

pub use camera::PerspectiveCamera;
pub use fluid::{FluidConfig, FluidError, FluidFrameStats, FluidRenderer};
pub use gpu::{BackendPreference, GpuConfig, GpuContext, GpuInfo, GpuInitError};
pub use liquid_chrome::{LiquidChromeConfig, LiquidChromeError, LiquidChromeRenderer};
pub use offscreen::{OffscreenError, OffscreenRenderTarget, RgbaColor};
pub use particles::{
    BenchmarkConfig, FrameTiming, FrameUniforms, ParticleRenderError, ParticleRenderer,
};
pub use post_process::{PostProcessConfig, PostProcessQuality};
pub use spatial_grid::{SpatialGrid, SpatialGridConfig, SpatialGridError, SpatialGridStats};
pub use surface::SurfacePresenter;
pub use volumetrics::{VolumetricConfig, VolumetricQuality, VolumetricRenderer};
pub use water_droplets::{WaterDropletConfig, WaterDropletError, WaterDropletRenderer};
