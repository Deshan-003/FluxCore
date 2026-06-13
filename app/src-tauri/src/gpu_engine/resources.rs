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

    /// Budget from a known dedicated-VRAM total. Percent semantics per freeze
    /// §0bis C3: `vram_budget_percent` is 1.0..=90.0, default 50.0.
    fn budget_from_total(total: u64, config: &AccelerationConfig) -> u64 {
        let percent_budget = (total as f64 * config.vram_budget_percent as f64 / 100.0) as u64;
        if config.vram_budget_cap_mb > 0 {
            percent_budget.min(config.vram_budget_cap_mb.saturating_mul(MIB))
        } else {
            percent_budget
        }
    }

    pub fn budget_bytes(&self, config: &AccelerationConfig) -> u64 {
        let total = self.effective_total(
            self.snapshot()
                .and_then(|s| s.gpus.first().and_then(|g| g.dedicated_total_bytes)),
        )
        .unwrap_or(0);
        Self::budget_from_total(total, config)
    }

    pub fn remaining_bytes(&self, config: &AccelerationConfig) -> u64 {
        self.budget_bytes(config)
            .saturating_sub(self.reserved_bytes())
    }

    /// Number of GPU tasks currently holding a reservation (running, plus the
    /// benchmark slot). Queued-but-not-started tasks hold no reservation.
    pub fn running_gpu_tasks(&self) -> usize {
        self.reserved.lock().map(|map| map.len()).unwrap_or(0)
    }

    /// Additive helper: the scheduler publishes its GPU queue depth so the UI
    /// can show queued vs running truthfully (the frozen surface had no setter).
    pub fn set_queued_gpu_tasks(&self, count: usize) {
        self.queued_gpu.store(count, Ordering::Relaxed);
    }

    fn gates_from(config: &AccelerationConfig, snapshot: Option<&SystemSnapshot>) -> Vec<String> {
        let mut gates = Vec::new();
        let Some(snapshot) = snapshot else {
            return gates;
        };
        if let Some(gpu) = snapshot.gpus.first() {
            if let Some(util) = gpu.utilization_percent {
                if util > config.max_gpu_utilization_percent {
                    gates.push(format!(
                        "GPU utilisation {util:.0}% above limit {:.0}%",
                        config.max_gpu_utilization_percent
                    ));
                }
            }
            if let Some(temp) = gpu.temperature_c {
                if temp > config.max_temperature_c {
                    gates.push(format!(
                        "GPU temperature {temp:.0}C above limit {:.0}C",
                        config.max_temperature_c
                    ));
                }
            }
        }
        if snapshot.memory.total_bytes > 0 {
            let available =
                snapshot.memory.available_bytes as f32 * 100.0 / snapshot.memory.total_bytes as f32;
            if available < config.min_system_ram_available_percent {
                gates.push(format!(
                    "System RAM available {available:.0}% below minimum {:.0}%",
                    config.min_system_ram_available_percent
                ));
            }
        }
        gates
    }

    pub fn gates(&self, config: &AccelerationConfig) -> Vec<String> {
