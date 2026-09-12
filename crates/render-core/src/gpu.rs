use std::fmt;

use thiserror::Error;

/// Graphics APIs that Rustique can select explicitly.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum BackendPreference {
    /// Let wgpu use all backends supported by the current platform.
    #[default]
    Auto,
    Dx12,
    Vulkan,
}

impl BackendPreference {
    #[must_use]
    pub const fn backends(self) -> wgpu::Backends {
        match self {
            Self::Auto => wgpu::Backends::all(),
            Self::Dx12 => wgpu::Backends::DX12,
            Self::Vulkan => wgpu::Backends::VULKAN,
        }
    }
}

/// Configuration for creating a headless GPU context.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct GpuConfig {
    pub backend: BackendPreference,
}

/// Adapter information useful for diagnostics and render manifests.
#[derive(Clone, Debug)]
pub struct GpuInfo {
    pub adapter: wgpu::AdapterInfo,
    pub limits: wgpu::Limits,
    pub features: wgpu::Features,
}

impl fmt::Display for GpuInfo {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(formatter, "Adapter: {}", self.adapter.name)?;
        writeln!(formatter, "Backend: {:?}", self.adapter.backend)?;
        writeln!(formatter, "Device type: {:?}", self.adapter.device_type)?;
        writeln!(
            formatter,
            "Driver: {}",
            display_unknown(&self.adapter.driver)
        )?;
        writeln!(
            formatter,
            "Driver info: {}",
            display_unknown(&self.adapter.driver_info)
        )?;
        writeln!(formatter, "Vendor ID: {:#06x}", self.adapter.vendor)?;
        writeln!(formatter, "Device ID: {:#06x}", self.adapter.device)?;
        writeln!(formatter, "Features: {:?}", self.features)?;
        write!(formatter, "Limits: {:#?}", self.limits)
    }
}

fn display_unknown(value: &str) -> &str {
    if value.is_empty() { "unknown" } else { value }
}

/// A device and queue together with the objects used to select them.
///
/// No surface or window is needed to construct this context.
#[derive(Debug)]
pub struct GpuContext {
    pub instance: wgpu::Instance,
    pub adapter: wgpu::Adapter,
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
}

#[derive(Debug, Error)]
pub enum GpuInitError {
    #[error("no compatible GPU adapter was found for backends {backends:?}")]
    NoAdapter { backends: wgpu::Backends },
    #[error("failed to request a logical device from adapter {adapter_name}: {source}")]
    RequestDevice {
        adapter_name: String,
        #[source]
        source: wgpu::RequestDeviceError,
    },
}

impl GpuContext {
    /// Selects a discrete GPU when available, then requests its default device.
    ///
    /// # Errors
    ///
    /// Returns [`GpuInitError::NoAdapter`] when the selected backend exposes no
    /// compatible adapter, or [`GpuInitError::RequestDevice`] when the adapter
    /// cannot create a logical device.
    pub async fn new(config: GpuConfig) -> Result<Self, GpuInitError> {
        let backends = config.backend.backends().with_env();
        log::info!("requested GPU backend set: {backends:?}");
        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
            backends,
            ..Default::default()
        });

        let adapter = instance
            .enumerate_adapters(backends)
            .into_iter()
            .max_by_key(adapter_score)
            .ok_or(GpuInitError::NoAdapter { backends })?;
        let adapter_info = adapter.get_info();
        log::info!(
            "selected GPU adapter '{}' using backend {:?}",
            adapter_info.name,
            adapter_info.backend
        );
        let adapter_name = adapter_info.name;
        let optional_features =
            wgpu::Features::TIMESTAMP_QUERY | wgpu::Features::TIMESTAMP_QUERY_INSIDE_PASSES;
        let required_features = adapter.features() & optional_features;
        let required_limits = adapter.limits();
        let (device, queue) = adapter
            .request_device(
                &wgpu::DeviceDescriptor {
                    label: Some("rustique-render-device"),
                    required_features,
                    required_limits,
                    memory_hints: wgpu::MemoryHints::Performance,
                },
                None,
            )
            .await
            .map_err(|source| GpuInitError::RequestDevice {
                adapter_name,
                source,
            })?;

        Ok(Self {
            instance,
            adapter,
            device,
            queue,
        })
    }

    #[must_use]
    pub fn info(&self) -> GpuInfo {
        GpuInfo {
            adapter: self.adapter.get_info(),
            limits: self.adapter.limits(),
            features: self.adapter.features(),
        }
    }
}

fn adapter_score(adapter: &wgpu::Adapter) -> u8 {
    match adapter.get_info().device_type {
        wgpu::DeviceType::DiscreteGpu => 4,
        wgpu::DeviceType::IntegratedGpu => 3,
        wgpu::DeviceType::VirtualGpu => 2,
        wgpu::DeviceType::Cpu => 1,
        wgpu::DeviceType::Other => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backend_preferences_map_to_expected_wgpu_backends() {
        assert_eq!(BackendPreference::Dx12.backends(), wgpu::Backends::DX12);
        assert_eq!(BackendPreference::Vulkan.backends(), wgpu::Backends::VULKAN);
        assert_eq!(BackendPreference::Auto.backends(), wgpu::Backends::all());
    }

    #[test]
    fn gpu_config_defaults_to_automatic_backend_selection() {
        assert_eq!(GpuConfig::default().backend, BackendPreference::Auto);
    }
}
