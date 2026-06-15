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
