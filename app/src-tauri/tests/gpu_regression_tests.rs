//! Phase 1 / Phase 2 regression guards: adding the GPU engine must not break
//! the existing monitor loop, memory intelligence or database schema.

use std::sync::Arc;
use std::time::{Duration, Instant};

use fluxcore_lib::database::Database;
use fluxcore_lib::memory_engine::MemoryEngine;
use fluxcore_lib::monitoring::MonitorEngine;

/// The engines initialize asynchronously; poll until ready instead of assuming
/// the first call succeeds.
fn wait_ready<T>(mut probe: impl FnMut() -> Result<T, String>, secs: u64) -> T {
    let deadline = Instant::now() + Duration::from_secs(secs);
    loop {
        match probe() {
            Ok(value) => return value,
            Err(error) => {
                assert!(Instant::now() < deadline, "engine never became ready: {error}");
                std::thread::sleep(Duration::from_millis(100));
            }
        }
    }
}

#[test]
fn phase1_monitor_still_produces_a_snapshot() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("regression.sqlite3");
    std::mem::forget(dir);
    let db = Arc::new(Database::open_at_path(path).unwrap());
    let memory = Arc::new(MemoryEngine::new(Arc::clone(&db)));
    let monitor = MonitorEngine::new(Arc::clone(&db), Arc::clone(&memory));
    let snapshot = wait_ready(|| monitor.snapshot(), 15);
    assert!(snapshot.cpu.logical_processors > 0);
    assert!(snapshot.memory.total_bytes > 0);
}

#[test]
fn phase2_memory_intelligence_still_works() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("regression2.sqlite3");
    std::mem::forget(dir);
    let db = Arc::new(Database::open_at_path(path).unwrap());
    let memory = Arc::new(MemoryEngine::new(Arc::clone(&db)));
    // The memory engine is ticked by the monitor loop; without a monitor it
    // never leaves the initializing state.
    let _monitor = MonitorEngine::new(Arc::clone(&db), Arc::clone(&memory));
    let intelligence = wait_ready(|| memory.intelligence(), 15);
    assert!(intelligence.analysis.total_bytes > 0);
}

#[test]
fn gpu_engine_construction_does_not_break_phase1_or_phase2() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("regression3.sqlite3");
    std::mem::forget(dir);
    let db = Arc::new(Database::open_at_path(path).unwrap());
    let memory = Arc::new(MemoryEngine::new(Arc::clone(&db)));
    let monitor = Arc::new(MonitorEngine::new(Arc::clone(&db), Arc::clone(&memory)));
    let gpu = fluxcore_lib::gpu_engine::GpuEngine::new(Arc::clone(&db), Arc::clone(&monitor));
    let state = gpu.state();
    // The CPU provider is always present, so the engine must never panic and
    // must always report at least one provider.
    assert!(!state.providers.is_empty());
    assert!(state.providers.iter().any(|p| p.id == "cpu"));
    wait_ready(|| monitor.snapshot(), 15);
    wait_ready(|| memory.intelligence(), 15);
}