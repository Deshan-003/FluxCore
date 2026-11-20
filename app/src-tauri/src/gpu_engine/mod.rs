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
                WorkloadView {
                    id: def.id.to_string(),
                    title: def.title.to_string(),
                    description: def.description.to_string(),
                    category: def.category.to_string(),
                    sizes: def.sizes.to_vec(),
                    selected_size,
                    mode: setting
                        .map(|item| item.mode)
                        .unwrap_or(AccelerationMode::IntelligentAuto),
                    pinned: setting.is_some(),
                    assessment,
                    last_benchmark,
                }
            })
            .collect()
    }

    // ---- command-facing API -------------------------------------------------

    pub fn state(&self) -> GpuAccelerationState {
        let config = self.read_config();
        let capabilities = self.read_capabilities();
        let resources = self.resources.status(&config);
        let summaries = self.benchmark_summaries();
        let compute_ready = capabilities.providers.iter().any(|provider| {
            provider.available && provider.compute_ready && provider.kind != ProviderKind::Cpu
        });
        let unavailable_reason = if !compute_ready {
            let reasons: Vec<String> = capabilities
                .providers
                .iter()
                .filter(|provider| provider.kind != ProviderKind::Cpu)
                .map(|provider| {
                    format!(
                        "{}: {}",
                        provider.label,
                        if provider.detail.is_empty() {
                            "not compute-ready"
                        } else {
                            provider.detail.as_str()
                        }
                    )
                })
                .collect();
            Some(if reasons.is_empty() {
                "No GPU compute provider was discovered".to_string()
            } else {
                reasons.join("; ")
            })
        } else if config.mode == AccelerationMode::CpuOnly {
            Some("GPU acceleration is disabled in CPU Only mode".to_string())
        } else {
            None
        };

        let mut safety_notes = vec![
            "Dedicated VRAM is GPU memory, not system RAM. FluxCore never converts VRAM into RAM."
                .to_string(),
            "GPU workloads are explicit, user-started computations; FluxCore does not accelerate other applications."
                .to_string(),
        ];
        safety_notes.extend(resources.gates.clone());
        safety_notes.extend(capabilities.notes.clone());
        if config.mode == AccelerationMode::CpuOnly {
            safety_notes.push("CPU Only mode: all supported workloads execute on the CPU.".to_string());
        }

        GpuAccelerationState {
            collected_at: chrono::Utc::now().to_rfc3339(),
            mode: config.mode,
            available: compute_ready && config.mode != AccelerationMode::CpuOnly,
            unavailable_reason,
            providers: capabilities.providers.clone(),
            workloads: Self::build_workloads(&config, &capabilities, &resources, &summaries),
            resources,
            tasks: self.scheduler.list(100, None),
            safety_notes,
            environment: capabilities.environment.clone(),
        }
    }

    pub fn providers(&self) -> Vec<ProviderStatus> {
        self.read_capabilities().providers
    }

    pub fn probe(&self, force: bool) -> Vec<ProviderStatus> {
        if force {
            for provider in &self.providers {
                // A failed initialize is a reported state, never an error here.
                let _ = provider.initialize();
            }
        }
        let report = capability::detect_providers(&self.providers);
        let statuses = report.providers.clone();
        self.resources
            .set_device_vram_total(Self::provider_vram_total(&self.providers));
        if let Ok(mut slot) = self.capabilities.write() {
            *slot = report;
        }
        for status in &statuses {
            let _ = persistence::upsert_provider(&self.database, status);
        }
        self.emit_capabilities(&statuses);
        statuses
    }

    pub fn config(&self) -> AccelerationConfig {
        self.read_config()
    }

    pub fn update_config(&self, config: AccelerationConfig) -> Result<AccelerationConfig, String> {
        resources::ResourceManager::validate_config(&config)?;
        persistence::save_config(&self.database, &config)?;
        persistence::save_workload_settings(&self.database, &config.workload_settings)?;
        self.emit_events.store(config.emit_events, Ordering::Relaxed);
        self.scheduler.set_config(config.clone());
        if let Ok(mut slot) = self.config.write() {
            *slot = config.clone();
        }
        Ok(config)
    }

    pub fn set_mode(&self, mode: AccelerationMode) -> Result<GpuAccelerationState, String> {
        let mut config = self.read_config();
        config.mode = mode;
        self.update_config(config)?;
        Ok(self.state())
    }

    pub fn workloads(&self) -> Vec<WorkloadView> {
        let config = self.read_config();
        let capabilities = self.read_capabilities();
        let resources = self.resources.status(&config);
        let summaries = self.benchmark_summaries();
        Self::build_workloads(&config, &capabilities, &resources, &summaries)
    }

    pub fn recommendations(
        &self,
        size: Option<usize>,
        workload_id: Option<String>,
    ) -> Result<Vec<SuitabilityAssessment>, String> {
        let config = self.read_config();
        let capabilities = self.read_capabilities();
        let resources = self.resources.status(&config);
        let summaries = self.benchmark_summaries();
        let inputs = Self::classify_inputs(&config, &capabilities, &resources, &summaries);

        let defs: Vec<&WorkloadDef> = match workload_id {
            Some(id) => vec![profiler::find(&id)
                .ok_or_else(|| format!("unknown workload id: {id}"))?],
            None => profiler::registry().iter().collect(),
        };
        let mut out = Vec::with_capacity(defs.len());
        for def in defs {
            let chosen = size.unwrap_or(def.default_size);
            let params = models::default_params(def.id, chosen)
                .ok_or_else(|| format!("unsupported size {chosen} for {}", def.id))?;
            out.push(profiler::classify(&params, &inputs));
        }
        Ok(out)
    }

    pub fn submit(&self, mut request: SubmitTaskRequest) -> Result<GpuTask, String> {
        profiler::validate_params(&request.params)?;
        let config = self.read_config();
        let capabilities = self.read_capabilities();
        let resources = self.resources.status(&config);
        let summaries = self.benchmark_summaries();
        let inputs = Self::classify_inputs(&config, &capabilities, &resources, &summaries);
        let assessment = profiler::classify(&request.params, &inputs);
        let gpu_enabled = inputs.gpu_enabled;

        if config.mode == AccelerationMode::CpuOnly && request.require_gpu {
            return Err("GPU acceleration is disabled (CPU Only mode)".to_string());
        }
        if request.require_gpu && !gpu_enabled {
            return Err("no usable GPU compute backend is available".to_string());
        }

        // Manual backend pin / per-workload override, otherwise use the decision
        // engine's own selected backend. The scheduler reads `request.backend`.
        let decided = if let Some(backend) = request.backend {
            backend
        } else if config.mode == AccelerationMode::Manual {
            config
                .workload_settings
                .iter()
                .find(|item| item.workload_id == request.params.workload_id())
                .map(|item| item.backend)
                .unwrap_or(assessment.selected_backend)
        } else {
            assessment.selected_backend
        };
        request.backend = Some(if decided != BackendKind::Cpu && !gpu_enabled {
            // A CPU-only decision must never be silently upgraded to a GPU path.
            BackendKind::Cpu
        } else {
            decided
        });

        self.scheduler.submit(request, assessment)
    }

    pub fn cancel(&self, task_id: u64) -> Result<GpuTask, String> {
        self.scheduler.cancel(task_id)
    }

    pub fn list_tasks(&self, limit: Option<usize>, status: Option<TaskStatus>) -> Vec<GpuTask> {
        self.scheduler.list(limit.unwrap_or(100).clamp(1, 500), status)
    }

    pub fn clear_finished(&self) -> usize {
        self.scheduler.clear_finished()
    }

    pub fn task_history(&self, limit: Option<usize>, workload: Option<String>) -> Vec<GpuTask> {
        persistence::recent_tasks(
            &self.database,
            limit.unwrap_or(100).clamp(1, 1000),
            workload.as_deref(),
        )
        .unwrap_or_default()
    }

    pub fn benchmark_history(
        &self,
        limit: Option<usize>,
        workload: Option<String>,
    ) -> Vec<BenchmarkRecord> {
        persistence::recent_benchmarks(
            &self.database,
            limit.unwrap_or(100).clamp(1, 1000),
            workload.as_deref(),
        )
        .unwrap_or_default()
    }

    pub fn clear_benchmarks(&self) -> usize {
        persistence::clear_benchmarks(&self.database).unwrap_or(0)
    }

    pub fn environment(&self) -> AccelerationEnvironment {
        self.read_capabilities().environment
    }

    pub fn run_benchmark(&self, request: BenchmarkRequest) -> Result<BenchmarkRecord, String> {
        let config = self.read_config();
        let capabilities = self.read_capabilities();
        let warmup = request.warmup_runs.clamp(0, 10);
        let measured = request.measured_runs.clamp(1, 50);
        let normalized = BenchmarkRequest {
            workload_id: request.workload_id.clone(),
            params: request.params.clone(),
            warmup_runs: warmup,
            measured_runs: measured,
            validate: request.validate,
        };
        profiler::validate_params(&normalized.params)?;

        // Benchmarks bypass the task queue but still take the same GPU budget and
        // safety gates. The guard releases the slot on every exit path.
        let requirement = WorkloadRequirement {
            workload_id: normalized.params.workload_id().to_string(),
            input_size: normalized.params.input_size(),
            device_bytes: normalized.params.device_bytes(),
            transfer_bytes: normalized.params.transfer_bytes(),
            timeout: Duration::from_millis(config.task_timeout_ms),
        };
        let admission = self.resources.admit(BENCHMARK_SLOT_ID, &requirement, &config);
        if !admission.allowed {
            return Err(admission
                .reason
                .unwrap_or_else(|| "GPU resource budget unavailable".to_string()));
        }
        struct BenchGuard<'a> {
            manager: &'a ResourceManager,
        }
        impl Drop for BenchGuard<'_> {
            fn drop(&mut self) {
                self.manager.release(BENCHMARK_SLOT_ID);
            }
        }
        let _guard = BenchGuard {
            manager: &self.resources,
        };

        let preferred = config
            .provider_priority
            .iter()
            .filter_map(|id| BackendKind::parse(id))
            .filter(|backend| *backend != BackendKind::Cpu)
            .find(|backend| {
                capabilities.providers.iter().any(|provider| {
                    provider.available
                        && provider.compute_ready
                        && BackendKind::parse(&provider.id) == Some(*backend)
                })
            });

        let context = benchmark::BenchmarkContext {
            providers: self.providers.clone(),
            environment: capabilities.environment.clone(),
            config: config.clone(),
            timeout: Duration::from_millis(config.task_timeout_ms),
            cancel: CancelToken::new(),
            preferred_backend: preferred,
        };
        let mut record = benchmark::run_benchmark(&normalized, &context)?;
        if let Ok(id) = persistence::record_benchmark(&self.database, &record) {
            record.id = id;
        }
        Ok(record)
    }
}

impl Drop for GpuEngine {
    fn drop(&mut self) {
        // Scheduler::Drop cancels and joins its worker threads first.
        for provider in &self.providers {
            provider.shutdown();
        }
    }
}
