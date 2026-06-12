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
