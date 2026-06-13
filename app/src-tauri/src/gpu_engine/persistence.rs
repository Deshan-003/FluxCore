//! Phase 3 — SQLite persistence for migration 0004 tables.
//!
//! Thin wrappers over `Database::with_connection` (freeze §7). The JSON
//! `payload` column is authoritative; the narrow columns exist only for
//! indexing/filtering. Writes happen on task completion, never per monitor tick.

use crate::database::Database;
use crate::gpu_engine::models::{
    AccelerationConfig, AccelerationMode, BackendKind, BenchmarkRecord, GpuTask, ProviderStatus,
    TaskStatus, WorkloadSetting,
};

fn enum_string<T: serde::Serialize>(value: &T) -> String {
    serde_json::to_string(value)
        .unwrap_or_else(|_| "null".to_string())
        .trim_matches('"')
        .to_string()
}

fn json<T: serde::Serialize>(value: &T) -> Result<String, String> {
    serde_json::to_string(value).map_err(|error| format!("serialize gpu payload: {error}"))
}

fn now() -> String {
    chrono::Utc::now().to_rfc3339()
}

/// The backend that actually produced the result: a fell-back task ran on CPU.
fn effective_backend(task: &GpuTask) -> BackendKind {
    if task.fell_back {
        BackendKind::Cpu
    } else {
        task.selected_backend.unwrap_or(BackendKind::Cpu)
    }
}

pub fn record_task(db: &Database, task: &GpuTask) -> Result<i64, String> {
    let payload = json(task)?;
    let warnings = json(&task.warnings)?;
    let backend = effective_backend(task);
    db.with_connection(|connection| {
        connection.execute(
            "INSERT INTO gpu_task_history (
               task_ref, workload_id, backend, provider_id, device_id, status, priority,
               queued_at, started_at, finished_at, duration_ms, queue_ms,
               cpu_time_ms, gpu_time_ms, transfer_time_ms, total_time_ms, speedup,
               vram_used_bytes, max_abs_error, validated, fell_back, error, warnings, payload
             ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19,?20,?21,?22,?23,?24)",
            rusqlite::params![
                task.id.to_string(),
                task.workload_id,
                backend.as_str(),
                crate::gpu_engine::providers::provider_for_backend(backend),
                "",
                enum_string(&task.status),
                enum_string(&task.priority),
                task.queued_at,
                task.started_at,
                task.finished_at,
                task.duration_ms,
                task.queue_ms,
                task.cpu_time_ms,
                task.gpu_time_ms,
                task.transfer_time_ms,
                task.total_time_ms,
                task.speedup,
                task.vram_used_bytes.map(|v| v as i64),
                task.max_abs_error,
                task.validated as i64,
                task.fell_back as i64,
                task.error,
                warnings,
                payload,
            ],
        )?;
        Ok(connection.last_insert_rowid())
    })
}

pub fn recent_tasks(
    db: &Database,
    limit: usize,
    workload: Option<&str>,
) -> Result<Vec<GpuTask>, String> {
    db.with_connection(|connection| {
        let mut out = Vec::new();
