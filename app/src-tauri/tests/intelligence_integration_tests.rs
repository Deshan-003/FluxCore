//! Phase 4 integration guard: the existing Phase 1 monitor loop must feed the
//! intelligence engine without any additional collector or background thread.

use std::sync::Arc;
use std::time::{Duration, Instant};

use fluxcore_lib::database::Database;
use fluxcore_lib::intelligence::IntelligenceEngine;
use fluxcore_lib::memory_engine::MemoryEngine;
use fluxcore_lib::monitoring::MonitorEngine;

#[test]
fn monitor_loop_feeds_the_intelligence_engine() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("phase4-integration.sqlite");
    // The monitor thread keeps running until the engine is dropped; keep the
    // directory alive for the duration rather than deleting it under SQLite.
    std::mem::forget(dir);
    let db = Arc::new(Database::open_at_path(path).unwrap());
    let memory = Arc::new(MemoryEngine::new(Arc::clone(&db)));
    let intelligence = Arc::new(IntelligenceEngine::new(
        Arc::clone(&db),
        Arc::clone(&memory),
    ));
    let monitor = MonitorEngine::with_intelligence(
        Arc::clone(&db),
        Arc::clone(&memory),
        Some(Arc::clone(&intelligence)),
    );

    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        if let Ok(state) = intelligence.state() {
            if state.monitoring {
                if let Some(latest) = state.latest.as_ref() {
                    assert!(!latest.values.is_empty(), "observation has no metrics");
                    assert!(latest.at > 0, "observation must carry a UTC timestamp");
                    assert!(!state.health.is_empty());
