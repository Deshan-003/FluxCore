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
            | GpuError::InitializationFailed { provider, .. }
            | GpuError::Unsupported { provider, .. }
            | GpuError::ResourceExhausted { provider, .. }
            | GpuError::ExecutionFailed { provider, .. }
            | GpuError::CorrectnessFailed { provider, .. }
            | GpuError::Cancelled { provider }
            | GpuError::Timeout { provider }
            | GpuError::Internal { provider, .. } => provider,
        }
    }

    /// Short machine-readable kind, for logs and UI badges.
    pub fn kind(&self) -> &'static str {
        match self {
            GpuError::DriverUnavailable { .. } => "driver-unavailable",
            GpuError::InitializationFailed { .. } => "initialization-failed",
            GpuError::Unsupported { .. } => "unsupported",
            GpuError::ResourceExhausted { .. } => "resource-exhausted",
            GpuError::ExecutionFailed { .. } => "execution-failed",
            GpuError::CorrectnessFailed { .. } => "correctness-failed",
            GpuError::Cancelled { .. } => "cancelled",
            GpuError::Timeout { .. } => "timeout",
            GpuError::Internal { .. } => "internal",
        }
    }

    /// True when retrying the same task could plausibly succeed later
    /// (execution failures, timeouts, transient resource exhaustion).
    pub fn is_retryable(&self) -> bool {
        matches!(
            self,
            GpuError::ExecutionFailed { .. } | GpuError::Timeout { .. } | GpuError::ResourceExhausted { .. }
        )
    }
}

impl std::fmt::Display for GpuError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            GpuError::DriverUnavailable { provider, detail } => {
                write!(f, "{provider}: driver/library unavailable: {detail}")
            }
            GpuError::InitializationFailed { provider, code, detail } => {
                write!(f, "{provider}: initialization failed{}: {detail}", code.map(|c| format!(" (code {c})")).unwrap_or_default())
            }
            GpuError::Unsupported { provider, detail } => write!(f, "{provider}: unsupported: {detail}"),
            GpuError::ResourceExhausted { provider, detail } => {
                write!(f, "{provider}: GPU resources exhausted: {detail}")
            }
            GpuError::ExecutionFailed { provider, code, detail } => {
                write!(f, "{provider}: execution failed{}: {detail}", code.map(|c| format!(" (code {c})")).unwrap_or_default())
            }
            GpuError::CorrectnessFailed { provider, max_abs_error, tolerance } => {
                write!(f, "{provider}: output validation failed: max abs error {max_abs_error:.3e} exceeds tolerance {tolerance:.3e}")
            }
            GpuError::Cancelled { provider } => write!(f, "{provider}: task cancelled"),
            GpuError::Timeout { provider } => write!(f, "{provider}: task timed out"),
            GpuError::Internal { provider, detail } => write!(f, "{provider}: internal error: {detail}"),
        }
    }
}

impl std::error::Error for GpuError {}

impl From<GpuError> for String {
    fn from(error: GpuError) -> String {
        error.to_string()
    }
}

/// Convenience alias used across the engine for results produced by providers
/// and by the execution path.
pub type GpuResult<T> = Result<T, GpuError>;