//! Phase 3 — honest CPU-vs-GPU benchmarking.
//!
//! Freeze §11. Warm-up runs are discarded; N measured runs produce
//! min/median/mean/max/stddev. The headline number is the median. `speedup` is
//! CPU median / GPU end-to-end median (transfer + launch included); a value
//! below 1.0 is reported as a slowdown with equal prominence. When no GPU path
//! actually ran, `gpu` is `None` with a reason — a GPU time is never invented.

use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::gpu_engine::error::GpuResult;
use crate::gpu_engine::models::{
    AccelerationConfig, AccelerationEnvironment, BackendKind, BenchmarkRecord, BenchmarkRequest,
    CancelToken, ExecutionOutcome, ExecutionRequest, TimingStats, WorkloadParams,
};
use crate::gpu_engine::profiler;
use crate::gpu_engine::providers::GpuProvider;

const NS_PER_MS: f64 = 1_000_000.0;

pub struct BenchmarkContext {
    pub providers: Vec<Arc<dyn GpuProvider>>,
    pub environment: AccelerationEnvironment,
    pub config: AccelerationConfig,
    pub timeout: Duration,
    pub cancel: CancelToken,
    pub preferred_backend: Option<BackendKind>,
}

fn stats(samples: &[f64]) -> TimingStats {
    let runs = samples.len() as u32;
    if samples.is_empty() {
        return TimingStats {
            runs: 0,
            min_ms: 0.0,
            median_ms: 0.0,
            mean_ms: 0.0,
            max_ms: 0.0,
            stddev_ms: 0.0,
        };
    }
    let mut sorted = samples.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let min_ms = sorted[0];
    let max_ms = sorted[sorted.len() - 1];
    let median_ms = if sorted.len() % 2 == 1 {
        sorted[sorted.len() / 2]
    } else {
        (sorted[sorted.len() / 2 - 1] + sorted[sorted.len() / 2]) / 2.0
    };
    let mean_ms = samples.iter().sum::<f64>() / samples.len() as f64;
    let variance = samples.iter().map(|v| (v - mean_ms).powi(2)).sum::<f64>() / samples.len() as f64;
    TimingStats {
        runs,
        min_ms,
        median_ms,
        mean_ms,
        max_ms,
        stddev_ms: variance.sqrt(),
    }
}

fn median(samples: &[f64]) -> Option<f64> {
    if samples.is_empty() {
        None
    } else {
        Some(stats(samples).median_ms)
    }
}

/// One timing sample on one backend. Used by the hot loops and the standalone
/// example. Never substitutes another backend.
pub fn run_once(
    provider: &dyn GpuProvider,
    params: &WorkloadParams,
    device_id: &str,
    timeout: Duration,
    cancel: &CancelToken,
    validate: bool,
) -> GpuResult<ExecutionOutcome> {
