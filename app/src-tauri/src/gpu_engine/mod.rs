//! Phase 3: GPU workload acceleration & intelligent CPU/GPU offloading.
//!
//! Main-agent-owned integration layer. It owns `GpuEngine`, state assembly,
//! configuration, provider probing, benchmark orchestration and the observer
//! that connects the scheduler to SQLite + Tauri events. It deliberately does
//! NOT implement providers, scheduling, safety or benchmarking itself.
//!
//! Behaviour: `PHASE3_ARCHITECTURE.md`. Exact signatures: `PHASE3_CONTRACT_FREEZE.md`.

pub mod benchmark;
pub mod capability;
pub mod error;
pub mod execution;
pub mod models;
pub mod persistence;
pub mod profiler;
pub mod providers;
pub mod resources;
pub mod scheduler;

pub use error::{GpuError, GpuResult};

use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, RwLock,
};
use std::time::Duration;
use tauri::{AppHandle, Emitter};

use crate::database::Database;
use crate::monitoring::MonitorEngine;

use models::*;
use providers::GpuProvider;
use resources::ResourceManager;
use scheduler::{Scheduler, TaskObserver};

/// Synthetic reservation id used while a benchmark holds the GPU slot.
const BENCHMARK_SLOT_ID: u64 = u64::MAX;

/// Owns all Phase 3 state. Constructed once in `lib.rs::run()`.
pub struct GpuEngine {
    database: Arc<Database>,
    #[allow(dead_code)]
    monitor: Arc<MonitorEngine>,
    providers: Vec<Arc<dyn GpuProvider>>,
    capabilities: RwLock<CapabilityReport>,
    resources: Arc<ResourceManager>,
    scheduler: Scheduler,
    config: RwLock<AccelerationConfig>,
    app: Arc<RwLock<Option<AppHandle>>>,
    emit_events: Arc<AtomicBool>,
}

impl GpuEngine {
    pub fn new(database: Arc<Database>, monitor: Arc<MonitorEngine>) -> Self {
        let config = persistence::load_config(&database)
            .ok()
            .flatten()
            .unwrap_or_default();
        let providers = providers::initialize_all();
        let capabilities = capability::detect_providers(&providers);
        let resources = Arc::new(ResourceManager::new(Arc::clone(&monitor)));
        resources.set_device_vram_total(Self::provider_vram_total(&providers));

        let app: Arc<RwLock<Option<AppHandle>>> = Arc::new(RwLock::new(None));
        let emit_events = Arc::new(AtomicBool::new(config.emit_events));
        let app_for_observer = Arc::clone(&app);
        let db_for_observer = Arc::clone(&database);
        let emit_for_observer = Arc::clone(&emit_events);
        let observer: TaskObserver = Arc::new(move |task| {
            // History is written on completion only, never on the monitor tick.
            if matches!(
                task.status,
                TaskStatus::Succeeded
                    | TaskStatus::Failed
                    | TaskStatus::Cancelled
                    | TaskStatus::TimedOut
                    | TaskStatus::Rejected
            ) {
                let _ = persistence::record_task(&db_for_observer, task);
            }
            if emit_for_observer.load(Ordering::Relaxed) {
                if let Ok(slot) = app_for_observer.read() {
                    if let Some(handle) = slot.as_ref() {
                        let _ = handle.emit("fluxcore-gpu-task", task);
                    }
                }
            }
        });

        let scheduler = Scheduler::new(
            providers.clone(),
            Arc::clone(&resources),
            observer,
            config.clone(),
        );

        Self {
            database,
            monitor,
            providers,
            capabilities: RwLock::new(capabilities),
            resources,
            scheduler,
            config: RwLock::new(config),
            app,
            emit_events,
        }
    }

    pub fn set_app_handle(&self, app: AppHandle) {
        if let Ok(mut slot) = self.app.write() {
            if slot.is_none() {
                *slot = Some(app);
            }
        }
    }

    fn provider_vram_total(providers: &[Arc<dyn GpuProvider>]) -> Option<u64> {
        providers
            .iter()
            .find(|provider| provider.kind() != ProviderKind::Cpu && provider.is_available())
            .and_then(|provider| provider.memory_info())
            .and_then(|memory| memory.total_bytes)
    }

    fn emit_capabilities(&self, statuses: &[ProviderStatus]) {
        if !self.emit_events.load(Ordering::Relaxed) {
            return;
        }
        if let Ok(slot) = self.app.read() {
            if let Some(handle) = slot.as_ref() {
                let _ = handle.emit("fluxcore-gpu-capabilities", statuses);
            }
        }
    }

    fn read_config(&self) -> AccelerationConfig {
        self.config
            .read()
            .map(|guard| guard.clone())
            .unwrap_or_default()
    }

    fn read_capabilities(&self) -> CapabilityReport {
        self.capabilities
            .read()
            .map(|guard| guard.clone())
            .unwrap_or_else(|_| capability::detect_providers(&self.providers))
    }

    fn benchmark_summaries(&self) -> Vec<BenchmarkSummary> {
        persistence::recent_benchmarks(&self.database, 200, None)
            .unwrap_or_default()
            .iter()
            .map(|record| BenchmarkSummary {
                id: record.id,
                created_at: record.created_at.clone(),
                workload_id: record.workload_id.clone(),
                input_size: record.params.input_size(),
                backend: record.backend,
                cpu_median_ms: record.cpu.median_ms,
                gpu_median_ms: record.gpu.map(|stats| stats.median_ms),
                transfer_ms: record.transfer_ms,
                speedup: record.speedup,
                verdict: record.verdict.clone(),
            })
            .collect()
    }

    fn classify_inputs<'a>(
        config: &'a AccelerationConfig,
        capabilities: &'a CapabilityReport,
        resources: &'a ResourceBudgetStatus,
        summaries: &'a [BenchmarkSummary],
    ) -> profiler::ClassifyInputs<'a> {
        let gpu_enabled = config.mode != AccelerationMode::CpuOnly
            && capabilities.providers.iter().any(|provider| {
                provider.available
                    && provider.compute_ready
                    && provider.kind != ProviderKind::Cpu
            });
        profiler::ClassifyInputs {
            config,
            providers: &capabilities.providers,
            resources,
            benchmarks: summaries,
            gpu_enabled,
        }
    }

    fn build_workloads(
        config: &AccelerationConfig,
        capabilities: &CapabilityReport,
        resources: &ResourceBudgetStatus,
        summaries: &[BenchmarkSummary],
    ) -> Vec<WorkloadView> {
        let inputs = Self::classify_inputs(config, capabilities, resources, summaries);
        profiler::registry()
            .iter()
            .map(|def| {
                let setting = config
                    .workload_settings
                    .iter()
                    .find(|item| item.workload_id == def.id);
                let selected_size = setting
                    .map(|item| item.selected_size)
                    .filter(|size| def.sizes.contains(size))
                    .unwrap_or(def.default_size);
                let params = models::default_params(def.id, selected_size)
                    .unwrap_or_else(|| models::default_params(def.id, def.default_size).unwrap());
                let assessment = profiler::classify(&params, &inputs);
                let last_benchmark = summaries
                    .iter()
                    .find(|item| item.workload_id == def.id && item.input_size == selected_size)
                    .cloned();
