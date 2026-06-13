//! Phase 3 — GPU resource budget, safety gates and admission control.
//!
//! Reads exactly ONE `MonitorEngine` snapshot per public call (freeze §6); it
//! never spawns a polling thread and never queries hardware itself. All
//! telemetry comes from the Phase 1 monitor loop; all budget maths is computed
//! here. VRAM is GPU memory: nothing in this module converts it to system RAM.

use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, RwLock};

use crate::gpu_engine::models::{
    AccelerationConfig, ResourceBudgetStatus, SafetyLevel, WorkloadRequirement,
};
use crate::monitoring::{MonitorEngine, SystemSnapshot};

const MIB: u64 = 1024 * 1024;

#[derive(Clone, Debug)]
pub struct AdmissionDecision {
    pub allowed: bool,
    pub reason: Option<String>,
    pub reserved_bytes: u64,
}

pub struct ResourceManager {
    monitor: Arc<MonitorEngine>,
    reserved: Mutex<HashMap<u64, u64>>,
    queued_gpu: AtomicUsize,
    /// Provider-reported dedicated VRAM total, used only when the Phase 1
    /// monitor snapshot has no GPU (e.g. NVML absent or a non-Windows host).
    device_vram_total: RwLock<Option<u64>>,
}

fn denied(reason: String) -> AdmissionDecision {
    AdmissionDecision {
        allowed: false,
        reason: Some(reason),
        reserved_bytes: 0,
    }
}

impl ResourceManager {
    pub fn new(monitor: Arc<MonitorEngine>) -> Self {
        Self {
            monitor,
            reserved: Mutex::new(HashMap::new()),
            queued_gpu: AtomicUsize::new(0),
            device_vram_total: RwLock::new(None),
        }
    }

    /// Additive helper: publish the compute provider's dedicated VRAM total so
    /// the budget stays meaningful when the monitor cannot see a GPU.
    pub fn set_device_vram_total(&self, bytes: Option<u64>) {
        if let Ok(mut slot) = self.device_vram_total.write() {
            *slot = bytes;
        }
    }

    fn effective_total(&self, monitor_total: Option<u64>) -> Option<u64> {
        monitor_total.or_else(|| self.device_vram_total.read().ok().and_then(|slot| *slot))
    }

    fn snapshot(&self) -> Option<SystemSnapshot> {
        self.monitor.snapshot().ok()
    }

    fn reserved_bytes(&self) -> u64 {
        self.reserved
            .lock()
            .map(|map| map.values().copied().sum())
            .unwrap_or(0)
    }
