//! Capability / environment reporting for Phase 3.
//!
//! Owned by Subagent 2. Records ONLY detected facts: provider state, a
//! one-time `sysinfo` snapshot for CPU/RAM, the CUDA toolkit / DLL probes, the
//! CUDA driver API version, and DX12/Vulkan presence as actually probed by the
//! wgpu provider's adapter discovery. No value is hardcoded to "true" and no
//! `MonitorEngine` is spawned (this free function has no monitor handle).

use crate::gpu_engine::models::{
    AccelerationEnvironment, CapabilityReport, ProviderKind, ProviderStatus,
};
use crate::gpu_engine::providers::{self, GpuProvider};
use std::sync::Arc;

/// Build one `ProviderStatus`. `detail` is a truthful, user-presentable
/// reason; when the provider is unavailable it is taken from `last_error()`.
pub fn status_of(provider: &dyn GpuProvider) -> ProviderStatus {
    let available = provider.is_available();
    let capabilities = provider.capabilities();
    let compute_ready = capabilities.compute_ready;
    let last_error = provider.last_error();
    let devices = provider.devices();
    let supported_workloads = provider.supported_workloads();
    let selected_device = devices
        .first()
        .map(|d| d.device_id.clone())
        .unwrap_or_default();

    let detail = if available {
        if compute_ready {
            format!(
                "{} ready: {} device(s), {} workload(s) with a compiled kernel",
                provider.label(),
                devices.len(),
                supported_workloads.len()
            )
        } else {
            capabilities.unavailable_reason.clone().unwrap_or_else(|| {
                format!("{} is present but not compute-ready", provider.label())
            })
        }
    } else {
        // Honest reason from the driver/adapter failure, surfaced in the UI.
        last_error
            .clone()
            .or_else(|| capabilities.unavailable_reason.clone())
            .unwrap_or_else(|| {
                "provider unavailable (driver library or GPU adapter missing)".to_string()
            })
    };

    ProviderStatus {
        id: provider.id().to_string(),
        label: provider.label().to_string(),
        kind: provider.kind(),
        available,
        compute_ready,
        detail,
        devices,
        capabilities,
        supported_workloads,
        selected_device,
        last_error,
    }
}

/// Assemble the full capability report. `initialize_all()` keeps unavailable
/// providers too, so they must still appear here for the UI.
pub fn detect_providers(providers: &[Arc<dyn GpuProvider>]) -> CapabilityReport {
    let statuses: Vec<ProviderStatus> = providers
        .iter()
        .map(|p| status_of(p.as_ref()))
        .collect();

    let mut notes = vec![
        "Capability report assembled from each provider's own detected state.".to_string(),
        "Environment uses a one-time sysinfo snapshot, not a spawned MonitorEngine.".to_string(),
        format!(
            "nvml.dll present: {} (Phase 1 telemetry owns NVML; providers do not call it)",
            dll_present("nvml.dll")
        ),
    ];
    let cuda_available = statuses
        .iter()
