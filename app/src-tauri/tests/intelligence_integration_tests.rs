//! Phase 4 integration guard: the existing Phase 1 monitor loop must feed the
//! intelligence engine without any additional collector or background thread.

use std::sync::Arc;
use std::time::{Duration, Instant};

use fluxcore_lib::database::Database;
use fluxcore_lib::intelligence::IntelligenceEngine;
use fluxcore_lib::memory_engine::MemoryEngine;
use fluxcore_lib::monitoring::MonitorEngine;

#[test]
