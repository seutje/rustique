//! Shared, window-independent rendering infrastructure.

mod camera;
mod dispatch;
mod flocking;
mod fluid;
mod gpu;
mod layers;
mod liquid_chrome;
mod offscreen;
mod particles;
mod passes;
mod post_process;
mod spatial_grid;
mod surface;
mod targets;
mod volumetrics;
mod water_droplets;

pub use camera::PerspectiveCamera;
pub use flocking::FlockingModulation;
pub use fluid::{FluidConfig, FluidError, FluidFrameStats, FluidRenderer};
pub use gpu::{BackendPreference, GpuConfig, GpuContext, GpuInfo, GpuInitError};
pub use layers::{LayerBlendMode, composite_rgba8};
pub use liquid_chrome::{LiquidChromeConfig, LiquidChromeError, LiquidChromeRenderer};
pub use offscreen::{OffscreenError, OffscreenRenderTarget, RgbaColor};
pub use particles::{
    BenchmarkConfig, FrameTiming, FrameUniforms, ParticleRenderError, ParticleRenderer,
};
pub use passes::{PassError, RenderPass, save_exr, save_render_passes};
pub use post_process::{PostProcessConfig, PostProcessQuality};
pub use spatial_grid::{SpatialGrid, SpatialGridConfig, SpatialGridError, SpatialGridStats};
pub use surface::SurfacePresenter;
pub use targets::{
    PrimitiveTarget, TargetError, load_gltf_points, load_svg_points, load_text_points,
    particles_from_target, primitive_points,
};
pub use volumetrics::{
    VolumetricConfig, VolumetricModulation, VolumetricQuality, VolumetricRenderer,
};
pub use water_droplets::{
    WaterDropletConfig, WaterDropletError, WaterDropletModulation, WaterDropletRenderer,
};
