//! Uniform GPU error reporting. Contract: PHASE3_ARCHITECTURE.md §3.2.
//!
//! Providers must never `unwrap()` driver calls and must never report success
//! for a backend that did not actually run. Every nonzero CUDA/NVML/wgpu return
//! code maps to exactly one variant below.

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum GpuError {
    /// Provider library/driver missing (e.g. nvcuda.dll not found).
    DriverUnavailable { provider: String, detail: String },
    /// Driver present but initialization failed (CUDA_ERROR_NO_DEVICE, etc.).
    InitializationFailed { provider: String, code: Option<i32>, detail: String },
    /// Device exists but cannot run this workload (CC too low, missing PTX, limits).
    Unsupported { provider: String, detail: String },
    /// Allocation or module load failed.
    ResourceExhausted { provider: String, detail: String },
    /// Kernel launch / execution error, incl. WDDM TDR (CUDA_ERROR_LAUNCH_TIMEOUT).
    ExecutionFailed { provider: String, code: Option<i32>, detail: String },
    /// Output did not match the CPU reference within tolerance.
    CorrectnessFailed { provider: String, max_abs_error: f64, tolerance: f64 },
    /// Task cancelled by the user or by timeout.
    Cancelled { provider: String },
    /// Timeout expired while waiting for the device.
    Timeout { provider: String },
    /// Anything else; keep `detail` user-presentable.
    Internal { provider: String, detail: String },
}

impl GpuError {
    /// The provider id that reported the failure (empty for internal errors
    /// raised outside a provider).
    pub fn provider(&self) -> &str {
        match self {
            GpuError::DriverUnavailable { provider, .. }
