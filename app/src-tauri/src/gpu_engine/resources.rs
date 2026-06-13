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
        Self::gates_from(config, self.snapshot().as_ref())
    }

    pub fn status(&self, config: &AccelerationConfig) -> ResourceBudgetStatus {
        let snapshot = self.snapshot();
        let gpu = snapshot.as_ref().and_then(|s| s.gpus.first());
        let total = self.effective_total(gpu.and_then(|g| g.dedicated_total_bytes));
        let free = gpu.and_then(|g| g.dedicated_available_bytes);
        let budget = Self::budget_from_total(total.unwrap_or(0), config);
        let reserved = self.reserved_bytes();
        let gates = Self::gates_from(config, snapshot.as_ref());
        let utilization = gpu.and_then(|g| g.utilization_percent);
        let safety_level = if !gates.is_empty() {
            SafetyLevel::Blocked
        } else if utilization
            .map(|u| u > config.max_gpu_utilization_percent * 0.8)
            .unwrap_or(false)
        {
            SafetyLevel::Caution
        } else {
            SafetyLevel::Ok
        };
        let system_ram_available_percent = snapshot
            .as_ref()
            .filter(|s| s.memory.total_bytes > 0)
            .map(|s| s.memory.available_bytes as f32 * 100.0 / s.memory.total_bytes as f32);
        ResourceBudgetStatus {
            total_vram_bytes: total,
            free_vram_bytes: free,
            budget_bytes: budget,
            reserved_bytes: reserved,
            // Reservations are released exactly when the provider frees the
            // matching buffers, so reserved == allocated; counting both would
            // double-subtract the same bytes (architecture R9).
            allocated_bytes: reserved,
            remaining_budget_bytes: budget.saturating_sub(reserved),
            running_gpu_tasks: self.running_gpu_tasks(),
            queued_gpu_tasks: self.queued_gpu.load(Ordering::Relaxed),
            max_concurrent_gpu_tasks: config.max_concurrent_gpu_tasks,
            gpu_utilization_percent: utilization,
            temperature_c: gpu.and_then(|g| g.temperature_c),
            power_watts: gpu.and_then(|g| g.power_watts),
            safety_level,
            gates,
            system_ram_available_percent,
        }
    }

    /// Idempotent per task id: a double reserve must not double-count.
    pub fn reserve(&self, task_id: u64, bytes: u64) -> bool {
        match self.reserved.lock() {
            Ok(mut map) => {
                map.entry(task_id).or_insert(bytes);
                true
            }
            Err(_) => false,
        }
    }

    /// Idempotent per task id: a double release must not corrupt accounting.
    pub fn release(&self, task_id: u64) {
        if let Ok(mut map) = self.reserved.lock() {
            map.remove(&task_id);
        }
    }

    pub fn admit(
        &self,
        task_id: u64,
        requirement: &WorkloadRequirement,
        config: &AccelerationConfig,
    ) -> AdmissionDecision {
        let running = self.running_gpu_tasks();
        if running >= config.max_concurrent_gpu_tasks {
            return denied(format!(
                "GPU concurrency limit reached ({running}/{} tasks hold the GPU budget)",
                config.max_concurrent_gpu_tasks
            ));
        }
        let snapshot = self.snapshot();
        let gates = Self::gates_from(config, snapshot.as_ref());
        if !gates.is_empty() {
            return denied(gates.join("; "));
        }
        let remaining = self
            .budget_bytes(config)
            .saturating_sub(self.reserved_bytes());
        if requirement.device_bytes > remaining {
            return denied(format!(
                "workload needs {} MiB of VRAM but the FluxCore budget leaves {} MiB",
                requirement.device_bytes / MIB,
                remaining / MIB
            ));
        }
        if let Some(free) = snapshot
            .as_ref()
            .and_then(|s| s.gpus.first())
            .and_then(|g| g.dedicated_available_bytes)
        {
            if requirement.device_bytes > free {
                return denied(format!(
                    "GPU reports {} MiB free VRAM; workload needs {} MiB",
                    free / MIB,
                    requirement.device_bytes / MIB
                ));
            }
        }
        self.reserve(task_id, requirement.device_bytes);
        AdmissionDecision {
            allowed: true,
            reason: None,
            reserved_bytes: requirement.device_bytes,
        }
    }

    pub fn validate_config(config: &AccelerationConfig) -> Result<(), String> {
        if !(1.0..=90.0).contains(&config.vram_budget_percent) {
            return Err("vramBudgetPercent must be between 1.0 and 90.0".to_string());
        }
        if config.vram_budget_cap_mb > 4096 {
            return Err("vramBudgetCapMb must be at most 4096".to_string());
        }
        if !(10.0..=100.0).contains(&config.max_gpu_utilization_percent) {
            return Err("maxGpuUtilizationPercent must be between 10.0 and 100.0".to_string());
        }
        if !(40.0..=110.0).contains(&config.max_temperature_c) {
            return Err("maxTemperatureC must be between 40.0 and 110.0".to_string());
        }
        if !(1..=2).contains(&config.max_concurrent_gpu_tasks) {
            return Err("maxConcurrentGpuTasks must be 1 or 2".to_string());
        }
        if !(1..=4).contains(&config.max_concurrent_cpu_tasks) {
            return Err("maxConcurrentCpuTasks must be between 1 and 4".to_string());
        }
        if !(1..=256).contains(&config.max_queue_length) {
            return Err("maxQueueLength must be between 1 and 256".to_string());
        }
        if !(500..=60000).contains(&config.task_timeout_ms) {
            return Err("taskTimeoutMs must be between 500 and 60000".to_string());
        }
        if !(1.0..=50.0).contains(&config.min_system_ram_available_percent) {
            return Err("minSystemRamAvailablePercent must be between 1.0 and 50.0".to_string());
        }
        if !(1..=90).contains(&config.retention_days) {
            return Err("retentionDays must be between 1 and 90".to_string());
        }
        Ok(())
    }
}