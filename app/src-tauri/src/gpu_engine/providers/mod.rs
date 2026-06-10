//! Concrete execution backends for Phase 3 (`gpu_engine/providers`).
//!
//! Owned by Subagent 2. Exact public surface: `PHASE3_CONTRACT_FREEZE.md` §4.
//! This module is deliberately a thin seam: it owns the `GpuProvider` trait,
//! the provider factory functions and the small shared helpers the three
//! backends use (CPU-reference validation and cooperative cancel/timeout
//! checks). All real compute lives in `cpu.rs`, `cuda.rs` and `wgpu.rs`.

pub mod cpu;
pub mod cuda;
pub mod wgpu;

pub use cpu::CpuProvider;
pub use cuda::CudaProvider;
pub use wgpu::WgpuProvider;

// Provider-side shared data lives in `models.rs` (leaf module). Re-export the
// subset the backends and the rest of the engine need so callers can do
// `use crate::gpu_engine::providers::{GpuProvider, ExecutionRequest, ...}`.
pub use crate::gpu_engine::models::{
    AccelerationEnvironment, BackendKind, CancelToken, CapabilityReport, ExecutionOutcome,
    ExecutionRequest, GpuDeviceInfo, GpuMemoryInfo, ProviderCapabilities, ProviderKind,
    ProviderStatus, TimingBreakdown, WorkloadInputs, WorkloadParams, WorkloadRequirement,
    WorkloadSupport,
};
pub use crate::gpu_engine::error::{GpuError, GpuResult};

use std::sync::Arc;
use std::time::{Duration, Instant};

/// A concrete GPU (or CPU) execution backend.
///
/// Implementations must be cheap to construct, safe to drop at any time, and
/// must never panic across the trait boundary: every failure is a `GpuError`.
/// All state is behind interior mutability because providers are shared as
/// `Arc<dyn GpuProvider>` across scheduler worker threads.
pub trait GpuProvider: Send + Sync {
    /// Stable id persisted in SQLite and shown in the UI ("cpu"|"cuda"|"wgpu").
    fn id(&self) -> &'static str;
    /// Human label.
    fn label(&self) -> &'static str;
    /// Coarse kind for UI grouping.
    fn kind(&self) -> ProviderKind;
    /// One-time initialisation. Must be idempotent and must not block > ~2 s.
    fn initialize(&self) -> GpuResult<Vec<GpuDeviceInfo>>;
    /// True when `initialize` succeeded and at least one device is usable.
    fn is_available(&self) -> bool;
    /// Devices from the most recent successful `initialize`.
    fn devices(&self) -> Vec<GpuDeviceInfo>;
    /// Live capability matrix.
    fn capabilities(&self) -> ProviderCapabilities;
    /// Workload ids this provider can actually execute, with per-impl limits.
    fn supported_workloads(&self) -> Vec<WorkloadSupport>;
    /// Memory snapshot as the provider sees it (may be `None` for CPU).
    fn memory_info(&self) -> Option<GpuMemoryInfo>;
    /// Optimistic admission check against real device limits.
    fn can_execute(&self, requirement: &WorkloadRequirement) -> GpuResult<()>;
    /// Execute one workload on one device. Blocking; called from a worker thread.
    fn execute(&self, request: &ExecutionRequest) -> GpuResult<ExecutionOutcome>;
    /// Release device contexts/buffers. Safe to call repeatedly.
    fn shutdown(&self);
    /// Last unrecoverable error, if any (surfaced in the UI).
    fn last_error(&self) -> Option<String>;
}

/// Constructs CPU + CUDA + wgpu providers (not yet initialized), in that order.
pub fn all_providers() -> Vec<Arc<dyn GpuProvider>> {
    let providers: Vec<Arc<dyn GpuProvider>> = vec![
        Arc::new(CpuProvider::new()),
        Arc::new(CudaProvider::new()),
        Arc::new(WgpuProvider::new()),
    ];
    providers
}

/// Calls `initialize()` on each provider and returns ALL of them, including
/// unavailable ones (which must still be reported in the UI). Never panics:
/// a failed initialize is a reported state, not a propagated error.
pub fn initialize_all() -> Vec<Arc<dyn GpuProvider>> {
    let providers = all_providers();
    for provider in &providers {
        // `initialize` itself must be panic-free; guard the call anyway so a
        // third-party driver surprise cannot take down `GpuEngine::new`.
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _ = provider.initialize();
        }));
    }
    providers
}

pub fn backend_for_provider(id: &str) -> BackendKind {
    match id {
        "cuda" => BackendKind::Cuda,
        "wgpu" => BackendKind::Wgpu,
        _ => BackendKind::Cpu,
    }
}

pub fn provider_for_backend(backend: BackendKind) -> &'static str {
    match backend {
        BackendKind::Cpu => "cpu",
        BackendKind::Cuda => "cuda",
        BackendKind::Wgpu => "wgpu",
    }
}

// ---------------------------------------------------------------------------
// Shared helpers used by the GPU backends.
// ---------------------------------------------------------------------------

/// Cooperative cancel/timeout checkpoint. Providers call this between chunks.
/// Returns `Cancelled` first (a user cancellation outranks a timeout).
pub(crate) fn check_interrupt(
    provider: &str,
    cancel: &CancelToken,
    started: Instant,
    timeout: Duration,
) -> GpuResult<()> {
    if cancel.is_cancelled() {
        return Err(GpuError::Cancelled {
            provider: provider.to_string(),
        });
    }
    if started.elapsed() >= timeout {
        return Err(GpuError::Timeout {
            provider: provider.to_string(),
        });
    }
    Ok(())
}

/// Validate a GPU result against the CPU reference (R16 tolerance rule).
///
/// Runs `params.run_cpu` (the authoritative reference) and `params.validate`
/// with `tolerance = 1e-3`. On success returns `(max_abs_error, validation_ns)`
/// where `max_abs_error` is the REAL measured maximum absolute difference. On
/// failure returns `GpuError::CorrectnessFailed` carrying that same real error.
pub(crate) fn validate_gpu(
    provider: &str,
    params: &WorkloadParams,
    inputs: &WorkloadInputs,
    out: &[f32],
) -> GpuResult<(f32, u64)> {
    let start = Instant::now();
    let mut reference = vec![0.0f32; params.output_len()];
    params.run_cpu(inputs, &mut reference).map_err(|e| {
        GpuError::Internal {
            provider: provider.to_string(),
            detail: format!("CPU reference failed: {e}"),
        }
    })?;

    // Compute the real measured error ourselves so it is available even when
    // validate() rejects (validate returns only a String on failure).
    let mut max_err = 0.0f32;
    for (&r, &c) in reference.iter().zip(out.iter()) {
        if r.is_finite() && c.is_finite() {
            let err = (r - c).abs();
            if err > max_err {
                max_err = err;
            }
        } else {
            max_err = f32::INFINITY;
            break;
        }
    }

    let tolerance = 1.0e-3f32;
    match params.validate(&reference, out, tolerance) {
        Ok(_) => Ok((max_err, start.elapsed().as_nanos() as u64)),
        Err(_detail) => Err(GpuError::CorrectnessFailed {
            provider: provider.to_string(),
            max_abs_error: max_err as f64,
            tolerance: tolerance as f64,
        }),
    }
}

/// Build an `ExecutionOutcome` from measured pieces, computing the stable
/// output digest and assembling the `TimingBreakdown`.
#[allow(clippy::too_many_arguments)]
pub(crate) fn build_outcome(
    backend: &str,
    provider_id: &str,
    device_id: String,
    params: &WorkloadParams,
    out: &[f32],
    timing: TimingBreakdown,
    peak_device_bytes: u64,
    notes: Vec<String>,
    validated: bool,
    max_abs_error: Option<f32>,
) -> ExecutionOutcome {
    ExecutionOutcome {
        backend: backend.to_string(),
        provider_id: provider_id.to_string(),
        device_id,
        timing,
        peak_device_bytes,
        notes,
        output_digest: params.digest(out),
        max_abs_error,
        validated,
    }
}

/// Resolve which device a request targets, falling back to the first available
/// device id. `request.device_id` may be empty (scheduler does not always set
/// it); providers honour it when it matches one of their devices.
pub(crate) fn pick_device_id(devices: &[GpuDeviceInfo], requested: &str) -> (String, bool) {
    if !requested.is_empty() && devices.iter().any(|d| d.device_id == requested) {
        (requested.to_string(), false)
    } else if let Some(first) = devices.first() {
        (first.device_id.clone(), !requested.is_empty())
    } else {
        (requested.to_string(), false)
    }
}
