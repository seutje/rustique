//! Shared, window-independent rendering infrastructure.

mod gpu;

pub use gpu::{BackendPreference, GpuConfig, GpuContext, GpuInfo, GpuInitError};
