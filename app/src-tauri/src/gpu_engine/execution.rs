//! Phase 3 — one task attempt: backend selection, provider dispatch, timing,
//! validation reporting and CPU fallback.
//!
//! Owned entirely by Subagent 3 (freeze §1.3, §10). Called ONLY from the
//! scheduler worker threads. Mutates `GpuTask` result fields in place and
//! returns the `ExecutionRecord` of the backend that actually produced the
//! result.
//!
//! GPU providers already validate their output against the CPU reference
//! internally (see `providers::validate_gpu`); this module NEVER duplicates
//! that computation. It only maps `ExecutionOutcome` -> `ExecutionRecord` and
//! reports the timings the provider actually measured. No GPU timing is ever
//! fabricated: when no GPU path ran, `gpu_time_ms` stays `None`.

use crate::gpu_engine::error::{GpuError, GpuResult};
use crate::gpu_engine::models::{
    AccelerationConfig, BackendKind, CancelToken, ExecutionOutcome, ExecutionRecord,
    ExecutionRequest, GpuTask, TimingBreakdown,
};
use crate::gpu_engine::providers::{self, GpuProvider};
use std::sync::Arc;
use std::time::Duration;

const NS_PER_MS: f64 = 1_000_000.0;

fn ns_to_ms(ns: u64) -> f64 {
    ns as f64 / NS_PER_MS
}

/// Find the provider whose id matches a backend ("cpu"|"cuda"|"wgpu").
fn find_provider(
    providers: &[Arc<dyn GpuProvider>],
    backend: BackendKind,
) -> Option<Arc<dyn GpuProvider>> {
    providers
        .iter()
        .find(|p| p.id() == backend.as_str())
        .cloned()
        .or_else(|| {
            providers
                .iter()
                .find(|p| providers::backend_for_provider(p.id()) == backend)
                .cloned()
        })
}

fn cpu_provider(providers: &[Arc<dyn GpuProvider>]) -> Option<Arc<dyn GpuProvider>> {
    providers.iter().find(|p| p.id() == "cpu").cloned()
}

fn first_device_id(provider: &dyn GpuProvider) -> String {
    provider
        .devices()
        .first()
        .map(|d| d.device_id.clone())
        .unwrap_or_default()
}

fn build_request(
    task: &GpuTask,
    provider: &dyn GpuProvider,
    config: &AccelerationConfig,
    cancel: &CancelToken,
) -> ExecutionRequest {
    ExecutionRequest {
        request_id: task.id,
        workload_id: task.workload_id.clone(),
        device_id: first_device_id(provider),
        params: task.params, // WorkloadParams is Copy
        timeout: Duration::from_millis(config.task_timeout_ms),
        cancel: cancel.clone(),
        validate: config.validate_results,
    }
}

fn outcome_backend(outcome: &ExecutionOutcome) -> BackendKind {
    BackendKind::parse(&outcome.backend)
        .unwrap_or_else(|| providers::backend_for_provider(&outcome.provider_id))
}

fn to_record(outcome: &ExecutionOutcome) -> ExecutionRecord {
    ExecutionRecord {
        backend: outcome_backend(outcome),
        provider_id: outcome.provider_id.clone(),
        device_id: outcome.device_id.clone(),
        timing: outcome.timing.clone(),
        peak_vram_bytes: outcome.peak_device_bytes,
        max_abs_error: outcome.max_abs_error,
        validated: outcome.validated,
        notes: outcome.notes.clone(),
    }
}

fn timing_of(outcome: &ExecutionOutcome) -> &TimingBreakdown {
    &outcome.timing
}

/// Apply a successful provider outcome to the task's result fields. `is_gpu`
/// is determined from the backend that ACTUALLY produced the outcome (not from
/// the requested backend), so a CPU fallback never reports a GPU timing.
fn apply_outcome(task: &mut GpuTask, outcome: &ExecutionOutcome, is_gpu: bool) {
    let t = timing_of(outcome);
    if is_gpu {
        task.gpu_time_ms = Some(ns_to_ms(t.compute_ns));
        task.transfer_time_ms = Some(ns_to_ms(
            t.transfer_in_ns.saturating_add(t.transfer_out_ns),
        ));
        // A GPU attempt measures no standalone CPU-total path. The host-side
        // setup/validation work is NOT a CPU reference run, so it is not
        // reported as `cpu_time_ms` and never used to fabricate a speedup.
        task.cpu_time_ms = None;
        task.total_time_ms = Some(ns_to_ms(t.total_ns));
    } else {
        task.cpu_time_ms = Some(ns_to_ms(t.total_ns));
        task.gpu_time_ms = None;
        task.transfer_time_ms = None;
        task.total_time_ms = Some(ns_to_ms(t.total_ns));
    }
    task.vram_used_bytes = Some(outcome.peak_device_bytes);
    task.max_abs_error = outcome.max_abs_error;
    task.validated = outcome.validated;
    task.progress_percent = Some(100.0);
    task.error = None;
    for note in &outcome.notes {
        if !task.warnings.contains(note) {
            task.warnings.push(note.clone());
        }
    }
}

/// Honest speedup: only computed when BOTH a CPU-total and a GPU-total were
/// actually measured in the same `execute_task` invocation. A single attempt
/// runs at most one successful backend, so in normal operation this is `None`
/// — never invented. A value < 1.0 (a slowdown) is reported as-is.
fn maybe_speedup(cpu_total_ms: Option<f64>, gpu_total_ms: Option<f64>) -> Option<f64> {
    match (cpu_total_ms, gpu_total_ms) {
        (Some(c), Some(g))
            if c.is_finite() && g.is_finite() && g > 0.0 =>
        {
            Some(c / g)
        }
        _ => None,
    }
}

/// One task attempt. Selects the provider for `task.selected_backend`, builds
/// the `ExecutionRequest`, dispatches, maps `ExecutionOutcome` ->
/// `ExecutionRecord`, fills the task result fields, and (unless `require_gpu`)
/// re-runs on the CPU provider after a recoverable GPU failure.
///
/// `Cancelled` and `Timeout` are propagated unchanged (they must surface as
/// cancellations/timeouts, never as a CPU fallback).
#[allow(clippy::too_many_arguments)]
pub fn execute_task(
    task: &mut GpuTask,
    providers: &[Arc<dyn GpuProvider>],
    config: &AccelerationConfig,
    cancel: &CancelToken,
    require_gpu: bool,
) -> GpuResult<ExecutionRecord> {
    let selected = task.selected_backend.unwrap_or(BackendKind::Cpu);
    let attempt_gpu = selected != BackendKind::Cpu;

    // Truthful per-attempt defaults. `warnings` is preserved (the scheduler
    // may already have recorded an admission-denial warning).
    task.error = None;
    task.fell_back = false;
    task.cpu_time_ms = None;
    task.gpu_time_ms = None;
    task.transfer_time_ms = None;
    task.total_time_ms = None;
    task.speedup = None;
    task.vram_used_bytes = None;
    task.max_abs_error = None;
    task.validated = false;

    let mut cpu_total_ms: Option<f64> = None;
    let mut gpu_total_ms: Option<f64> = None;

    let cpu = cpu_provider(providers).ok_or_else(|| GpuError::Internal {
        provider: "scheduler".to_string(),
        detail: "no CPU provider (id \"cpu\") is available for execution".to_string(),
    })?;

    let primary = find_provider(providers, selected).unwrap_or_else(|| {
        if attempt_gpu {
            task.warnings.push(format!(
                "No provider implements backend {}; falling back to the CPU provider",
                selected.as_str()
            ));
            task.fell_back = true;
        }
