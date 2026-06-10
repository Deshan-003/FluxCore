//! CPU provider: the authoritative reference and the CPU fallback.
//!
//! Implements all four workloads with plain `f32` loops. To honour cooperative
//! cancellation and the wall-clock timeout, the outer loop is split into
//! chunks and each chunk is executed through `WorkloadParams::run_cpu` on a
//! sub-problem (sub-params + sliced inputs). This reproduces `run_cpu`'s exact
//! per-element arithmetic (identical operation order, so bit-identical output)
//! while giving us cancellation checkpoints between chunks.

use super::{
    build_outcome, check_interrupt, pick_device_id, CancelToken, ExecutionOutcome,
    ExecutionRequest, GpuDeviceInfo, GpuError, GpuMemoryInfo, GpuProvider, GpuResult,
    ProviderCapabilities, ProviderKind, TimingBreakdown, WorkloadInputs, WorkloadParams,
    WorkloadRequirement, WorkloadSupport,
};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

/// Work-element budget per chunk (used to derive chunk sizes for each shape).
const CHUNK: usize = 1 << 18; // 262144

const SUPPORTED: [&str; 4] = ["vector_axpy", "vector_scale", "matmul", "row_sum"];

/// Max host bytes a single CPU attempt may allocate for its buffers.
const MAX_CPU_BYTES: u64 = 2 * 1024 * 1024 * 1024;

pub struct CpuProvider {
    ready: AtomicBool,
}

impl CpuProvider {
    pub fn new() -> Self {
        CpuProvider {
            ready: AtomicBool::new(true),
        }
    }

    fn device() -> GpuDeviceInfo {
        GpuDeviceInfo {
            device_id: "cpu".to_string(),
            name: "CPU (scalar reference / fallback)".to_string(),
            vendor: std::env::consts::ARCH.to_string(),
            backend: "cpu".to_string(),
            monitor_gpu_index: None,
            dedicated_total_bytes: None,
            dedicated_free_bytes: None,
            compute_capability: None,
            driver_version: None,
            max_workgroup_size: None,
            max_storage_buffer_bytes: Some(MAX_CPU_BYTES),
            is_discrete: false,
            notes: vec![
                "Runs on worker threads; no device context is held.".to_string(),
                "This is the reference used to validate every GPU result.".to_string(),
            ],
        }
    }
}

impl Default for CpuProvider {
    fn default() -> Self {
        Self::new()
    }
}

/// Compute `params` into `out` via chunked `run_cpu` sub-problems, checking
/// cancel/timeout between chunks. `out.len()` must equal `params.output_len()`.
fn run_chunked(
    params: &WorkloadParams,
    inputs: &WorkloadInputs,
    out: &mut [f32],
    provider: &str,
    cancel: &CancelToken,
    timeout: Duration,
    started: Instant,
) -> GpuResult<()> {
    match *params {
        WorkloadParams::VectorAxpy { n, a } => {
            let step = n.min(CHUNK).max(1);
            let mut s = 0usize;
            while s < n {
                check_interrupt(provider, cancel, started, timeout)?;
                let e = (s + step).min(n);
                let sub = WorkloadParams::VectorAxpy { n: e - s, a };
                let sub_inputs = WorkloadInputs {
                    x: inputs.x[s..e].to_vec(),
                    y: inputs.y[s..e].to_vec(),
                };
                sub.run_cpu(&sub_inputs, &mut out[s..e]).map_err(|err| {
                    GpuError::Internal {
                        provider: provider.to_string(),
                        detail: err,
                    }
                })?;
                s = e;
            }
        }
        WorkloadParams::VectorScale { n, a } => {
            let step = n.min(CHUNK).max(1);
            let mut s = 0usize;
            while s < n {
                check_interrupt(provider, cancel, started, timeout)?;
                let e = (s + step).min(n);
                let sub = WorkloadParams::VectorScale { n: e - s, a };
                let sub_inputs = WorkloadInputs {
                    x: inputs.x[s..e].to_vec(),
                    y: Vec::new(),
                };
                sub.run_cpu(&sub_inputs, &mut out[s..e]).map_err(|err| {
                    GpuError::Internal {
                        provider: provider.to_string(),
                        detail: err,
                    }
                })?;
                s = e;
            }
        }
        WorkloadParams::Matmul { m, k, n } => {
            let rows_per_chunk = (CHUNK / n.max(1)).max(1);
            let mut rs = 0usize;
            while rs < m {
                check_interrupt(provider, cancel, started, timeout)?;
                let re = (rs + rows_per_chunk).min(m);
                let sub = WorkloadParams::Matmul { m: re - rs, k, n };
                let sub_inputs = WorkloadInputs {
                    x: inputs.x[rs * k..re * k].to_vec(),
                    y: inputs.y.clone(),
                };
                sub.run_cpu(&sub_inputs, &mut out[rs * n..re * n])
                    .map_err(|err| GpuError::Internal {
                        provider: provider.to_string(),
                        detail: err,
                    })?;
                rs = re;
            }
        }
        WorkloadParams::RowSum { rows, cols } => {
            let rows_per_chunk = (CHUNK / cols.max(1)).max(1);
            let mut rs = 0usize;
            while rs < rows {
                check_interrupt(provider, cancel, started, timeout)?;
                let re = (rs + rows_per_chunk).min(rows);
                let sub = WorkloadParams::RowSum {
                    rows: re - rs,
                    cols,
                };
                let sub_inputs = WorkloadInputs {
                    x: inputs.x[rs * cols..re * cols].to_vec(),
                    y: Vec::new(),
                };
                sub.run_cpu(&sub_inputs, &mut out[rs..re]).map_err(|err| {
                    GpuError::Internal {
                        provider: provider.to_string(),
                        detail: err,
                    }
                })?;
                rs = re;
            }
        }
    }
    check_interrupt(provider, cancel, started, timeout)?;
    Ok(())
}

impl GpuProvider for CpuProvider {
    fn id(&self) -> &'static str {
        "cpu"
    }
