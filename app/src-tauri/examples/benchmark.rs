//! FluxCore Phase 2 benchmark harness.
//!
//! Measures, with real system data only:
//!   1. Phase 1 baseline — average cost of one full collector pass.
//!   2. First memory-engine tick — cold path: PDH counter initialization,
//!      full process scan, prediction and persistence.
//!   3. Steady-state tick — hot path average over N iterations (periodic
//!      scans and persistence still fire on their configured intervals).
//!   4. Combined pipeline cost and the engine overhead percentage.
//!   5. FluxCore's own footprint (private bytes / working set), read with
//!      the same documented APIs the engine uses for other processes.
//!
//! Usage: cargo run --release --example benchmark [iterations]
//! The benchmark never modifies system state; its database is a throwaway
//! file in the temp directory and is removed on exit.

use std::sync::Arc;

use fluxcore_lib::database::Database;
use fluxcore_lib::memory_engine::process_analyzer::read_process;
use fluxcore_lib::memory_engine::MemoryEngine;
use fluxcore_lib::monitoring::{benchmark_collect, benchmark_collect_and_tick};

fn mib(bytes: u64) -> f64 {
    bytes as f64 / (1024.0 * 1024.0)
}

/// Resident/peak footprint from /proc on non-Windows platforms, in MiB.
#[cfg(not(windows))]
fn linux_footprint_mib() -> Option<(f64, f64)> {
    let status = std::fs::read_to_string("/proc/self/status").ok()?;
    let kb = |key: &str| {
        status
            .lines()
            .find(|line| line.starts_with(key))
            .and_then(|line| line.split_whitespace().nth(1))
            .and_then(|value| value.parse::<u64>().ok())
    };
    Some((kb("VmRSS:")? as f64 / 1024.0, kb("VmHWM:")? as f64 / 1024.0))
}

#[cfg(windows)]
fn linux_footprint_mib() -> Option<(f64, f64)> {
    None
}

fn main() {
    let iterations: u32 = std::env::args()
        .nth(1)
        .and_then(|value| value.parse().ok())
        .unwrap_or(25)
        .clamp(5, 500);

    println!("FluxCore Phase 2 benchmark — {iterations} steady-state iterations");
    println!("-------------------------------------------------------------------");

    // 1. Phase 1 collector baseline.
    let collect_only_us = benchmark_collect(iterations);
    println!("[phase 1] collector pass          avg {collect_only_us:>8} µs");

    // Engine setup against a throwaway database.
    let db_path = std::env::temp_dir().join(format!("fluxcore-benchmark-{}.sqlite", std::process::id()));
    let database = Arc::new(
        Database::open_at_path(db_path.clone()).expect("failed to open benchmark database"),
    );
    let memory = MemoryEngine::new(Arc::clone(&database));
    memory.set_dashboard_active(true);
