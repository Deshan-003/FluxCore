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
