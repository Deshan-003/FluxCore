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
        .any(|s| s.kind == ProviderKind::Cuda && s.available && s.compute_ready);
    let wgpu_available = statuses
        .iter()
        .any(|s| s.kind == ProviderKind::Wgpu && s.available && s.compute_ready);
    if !cuda_available {
        notes.push("CUDA compute is not active; see the CUDA provider detail for the driver/PTX reason.".to_string());
    }
    if !wgpu_available {
        notes.push("wgpu compute is not active; see the wgpu provider detail for the adapter/compile reason.".to_string());
    }

    CapabilityReport {
        collected_at: chrono::Utc::now().to_rfc3339(),
        providers: statuses,
        environment: environment(providers),
        notes,
    }
}

/// Search `PATH` for an executable (with and without a `.exe` suffix).
fn which(exe: &str) -> bool {
    let Some(paths) = std::env::var_os("PATH") else {
        return false;
    };
    for dir in std::env::split_paths(&paths) {
        if dir.join(exe).is_file() {
            return true;
        }
        if dir.join(format!("{exe}.exe")).is_file() {
            return true;
        }
    }
    false
}

/// Probe whether a system DLL is loadable (a real detection, not a guess).
fn dll_present(name: &str) -> bool {
    // SAFETY: loading a system DLL by name; no symbols are resolved here.
    unsafe { libloading::Library::new(name) }.is_ok()
}

/// Detect the acceleration environment from real facts only.
pub fn environment(providers: &[Arc<dyn GpuProvider>]) -> AccelerationEnvironment {
    // --- toolkit / driver facts -------------------------------------------
    let cuda_toolkit_present =
        std::env::var_os("CUDA_PATH").is_some() || which("nvcc") || which("nvcc.exe");
    // The CUDA runtime API (cudart) ships with the toolkit; present only when
    // the toolkit is. Recorded as an inference, documented in notes.
    let cuda_runtime_api_present = cuda_toolkit_present;
    let nvcuda_present = dll_present("nvcuda.dll");

    let cuda_compute_path = if nvcuda_present && !cuda_toolkit_present {
        "CUDA driver API via nvcuda.dll (no toolkit installed)".to_string()
    } else if nvcuda_present {
        "CUDA driver API via nvcuda.dll; CUDA toolkit is also installed".to_string()
    } else {
        "CUDA driver API unavailable (nvcuda.dll not found)".to_string()
    };

    // --- OS / CPU / RAM via one sysinfo snapshot ---------------------------
    let os_name = std::env::consts::OS.to_string();
    let os_version = sysinfo::System::kernel_version()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "unknown".to_string());
    let sys = sysinfo::System::new_all();
    let cpu_name = sys
        .cpus()
        .first()
        .map(|c| c.brand().trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "unavailable".to_string());
    let logical_processors = sys.cpus().len();
    let system_ram_bytes = sys.total_memory();

    // --- GPU name / VRAM / driver from provider-discovered devices ---------
    let mut gpu_name = "unavailable (no GPU adapter reported)".to_string();
    let mut gpu_vram_bytes: Option<u64> = None;
    let mut driver_version: Option<String> = None;
    let mut wgpu_backend: Option<String> = None;
    let mut dx12_present = false;
    let mut vulkan_present = false;

    for provider in providers {
        let kind = provider.kind();
        for device in provider.devices() {
