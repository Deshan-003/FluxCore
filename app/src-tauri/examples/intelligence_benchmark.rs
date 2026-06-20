//! FluxCore Phase 4 benchmark harness.
//!
//! Measures the cost the intelligence engine adds to the existing Phase 1/2
//! monitor loop, using the real OS collector:
//!   1. In-memory Phase 4 sample: analysis, forecasts, anomalies, findings.
//!   2. Full Phase 4 sample: the same work plus the SQLite transaction.
//!   3. Persistence delta: the storage-write share of a sample.
//!   4. Gated tick: the per-monitoring-tick cost inside the 15-second window.
//!   5. Process footprint and throwaway-database size.
//!
//! Every metric value is a live reading. Only the snapshot timestamp is shifted
//! by the sampling cadence so each iteration performs a full sample instead of
//! the early return. The benchmark never modifies system state; its database is
//! a temporary file that is removed on exit.
//!
//! Usage: cargo run --release --example intelligence_benchmark [iterations]

use std::sync::Arc;

use fluxcore_lib::database::Database;
use fluxcore_lib::intelligence::IntelligenceEngine;
use fluxcore_lib::memory_engine::process_analyzer::read_process;
use fluxcore_lib::memory_engine::MemoryEngine;
use fluxcore_lib::monitoring::benchmark_intelligence;

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
        .unwrap_or(20)
        .clamp(2, 40);

