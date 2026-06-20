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
