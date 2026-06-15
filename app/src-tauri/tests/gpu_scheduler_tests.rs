//! Phase 3 scheduler tests using a mock GPU provider (mocks live only in tests).
//! Covers backend selection, CPU fallback, require_gpu, cancellation, timeout,
//! backpressure and GPU concurrency limits without any real GPU.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use fluxcore_lib::database::Database;
use fluxcore_lib::gpu_engine::error::{GpuError, GpuResult};
use fluxcore_lib::gpu_engine::models::{
    AccelerationConfig, AccelerationMode, BackendKind, ExecutionOutcome,
    ExecutionRequest, GpuDeviceInfo, GpuMemoryInfo, GpuTask, ProviderCapabilities, ProviderKind,
    SubmitTaskRequest, SuitabilityAssessment, TaskPriority, TaskStatus, TimingBreakdown,
    WorkloadParams, WorkloadRequirement, WorkloadSupport,
};
use fluxcore_lib::gpu_engine::providers::{CpuProvider, GpuProvider};
use fluxcore_lib::gpu_engine::resources::ResourceManager;
use fluxcore_lib::gpu_engine::scheduler::{Scheduler, TaskObserver};
use fluxcore_lib::gpu_engine::{capability, profiler};
use fluxcore_lib::memory_engine::MemoryEngine;
use fluxcore_lib::monitoring::MonitorEngine;

const GIB: u64 = 1024 * 1024 * 1024;

#[derive(Clone, Copy, PartialEq)]
enum Mode {
    Success,
    Fail,
    Hold,
}

struct MockState {
    mode: Mutex<Mode>,
    executes: AtomicUsize,
}

struct MockProvider {
    state: Arc<MockState>,
}

impl GpuProvider for MockProvider {
    fn id(&self) -> &'static str {
        "cuda"
    }
    fn label(&self) -> &'static str {
        "Mock CUDA provider (tests only)"
    }
    fn kind(&self) -> ProviderKind {
        ProviderKind::Cuda
    }
    fn initialize(&self) -> GpuResult<Vec<GpuDeviceInfo>> {
        Ok(vec![GpuDeviceInfo {
            device_id: "0".to_string(),
            name: "Mock GPU".to_string(),
            vendor: "Mock".to_string(),
            backend: "cuda-driver".to_string(),
            monitor_gpu_index: None,
            dedicated_total_bytes: Some(4 * GIB),
            dedicated_free_bytes: Some(4 * GIB),
            compute_capability: Some("8.6".to_string()),
            driver_version: Some("mock".to_string()),
            max_workgroup_size: Some(1024),
            max_storage_buffer_bytes: Some(1 << 30),
            is_discrete: true,
            notes: Vec::new(),
        }])
    }
    fn is_available(&self) -> bool {
        true
    }
    fn devices(&self) -> Vec<GpuDeviceInfo> {
        self.initialize().unwrap_or_default()
    }
    fn capabilities(&self) -> ProviderCapabilities {
        ProviderCapabilities {
            compute_ready: true,
            unavailable_reason: None,
            max_threads_per_workgroup: Some(1024),
            max_workgroup_size_x: Some(1024),
            max_workgroup_size_y: Some(1024),
            max_workgroup_size_z: Some(64),
            max_buffer_bytes: Some(1 << 30),
            allocation_alignment: 256,
            backend_notes: Vec::new(),
        }
    }
    fn supported_workloads(&self) -> Vec<WorkloadSupport> {
        ["vector_axpy", "vector_scale", "matmul", "row_sum"]
            .iter()
            .map(|id| WorkloadSupport {
                workload_id: id.to_string(),
                max_elements: 64 * 1024 * 1024,
                notes: Vec::new(),
            })
            .collect()
    }
    fn memory_info(&self) -> Option<GpuMemoryInfo> {
        Some(GpuMemoryInfo {
            total_bytes: Some(4 * GIB),
            free_bytes: Some(4 * GIB),
            used_bytes: Some(0),
            allocated_bytes: 0,
        })
    }
    fn can_execute(&self, _requirement: &WorkloadRequirement) -> GpuResult<()> {
        Ok(())
    }
    fn execute(&self, request: &ExecutionRequest) -> GpuResult<ExecutionOutcome> {
        self.state.executes.fetch_add(1, Ordering::SeqCst);
        let mode = *self.state.mode.lock().unwrap();
        let started = Instant::now();
        match mode {
            Mode::Fail => Err(GpuError::ExecutionFailed {
                provider: "cuda".to_string(),
                code: Some(999),
                detail: "injected mock failure".to_string(),
            }),
            Mode::Hold => loop {
                if request.cancel.is_cancelled() {
                    return Err(GpuError::Cancelled {
                        provider: "cuda".to_string(),
                    });
                }
                if started.elapsed() >= request.timeout {
                    return Err(GpuError::Timeout {
                        provider: "cuda".to_string(),
                    });
                }
                std::thread::sleep(Duration::from_millis(10));
            },
            Mode::Success => {
                let inputs = request.params.make_inputs();
                let mut out = vec![0.0f32; request.params.output_len()];
                request
                    .params
                    .run_cpu(&inputs, &mut out)
                    .map_err(|detail| GpuError::Internal {
                        provider: "cuda".to_string(),
                        detail,
                    })?;
                let total = started.elapsed().as_nanos() as u64;
                Ok(ExecutionOutcome {
                    backend: "cuda".to_string(),
                    provider_id: "cuda".to_string(),
                    device_id: "0".to_string(),
                    timing: TimingBreakdown {
                        setup_ns: 0,
                        transfer_in_ns: total / 4,
                        compute_ns: total / 2,
                        transfer_out_ns: total / 4,
                        validation_ns: 0,
                        total_ns: total,
                        transferred_bytes: request.params.transfer_bytes(),
                    },
                    peak_device_bytes: request.params.device_bytes(),
                    notes: Vec::new(),
                    output_digest: request.params.digest(&out),
                    max_abs_error: Some(0.0),
                    validated: true,
                })
            }
        }
    }
    fn shutdown(&self) {}
    fn last_error(&self) -> Option<String> {
        None
    }
}

struct Harness {
    scheduler: Scheduler,
    resources: Arc<ResourceManager>,
    events: Arc<Mutex<Vec<GpuTask>>>,
    mock: Arc<MockState>,
    config: AccelerationConfig,
    providers: Vec<Arc<dyn GpuProvider>>,
}

fn harness(mode: Mode, mutate: impl FnOnce(&mut AccelerationConfig)) -> Harness {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("scheduler-test.sqlite3");
    std::mem::forget(dir);
    let db = Arc::new(Database::open_at_path(path).unwrap());
    let memory = Arc::new(MemoryEngine::new(Arc::clone(&db)));
    let monitor = Arc::new(MonitorEngine::new(Arc::clone(&db), Arc::clone(&memory)));
    let resources = Arc::new(ResourceManager::new(Arc::clone(&monitor)));
    // The Linux verification host has no monitor-visible GPU; publish the mock
    // device's VRAM so admission behaves as it does on real hardware.
    resources.set_device_vram_total(Some(4 * GIB));
    let events = Arc::new(Mutex::new(Vec::new()));
    let sink = Arc::clone(&events);
    let observer: TaskObserver = Arc::new(move |task| sink.lock().unwrap().push(task.clone()));
    let mock = Arc::new(MockState {
        mode: Mutex::new(mode),
        executes: AtomicUsize::new(0),
    });
    let providers: Vec<Arc<dyn GpuProvider>> = vec![
        Arc::new(CpuProvider::new()),
        Arc::new(MockProvider {
            state: Arc::clone(&mock),
        }),
    ];
    for provider in &providers {
        let _ = provider.initialize();
    }
    let mut config = AccelerationConfig::default();
    config.mode = AccelerationMode::GpuAcceleration;
    mutate(&mut config);
    let scheduler = Scheduler::new(
        providers.clone(),
        Arc::clone(&resources),
        observer,
        config.clone(),
    );
    Harness {
        scheduler,
        resources,
        events,
        mock,
        config,
        providers,
    }
}

fn assessment(h: &Harness, params: &WorkloadParams) -> SuitabilityAssessment {
    let statuses = capability::detect_providers(&h.providers).providers;
    let resources = h.resources.status(&h.config);
    profiler::classify(
        params,
        &profiler::ClassifyInputs {
            config: &h.config,
            providers: &statuses,
            resources: &resources,
            benchmarks: &[],
            gpu_enabled: true,
        },
    )
}

fn request(params: WorkloadParams, backend: Option<BackendKind>, require_gpu: bool) -> SubmitTaskRequest {
    SubmitTaskRequest {
        workload_id: params.workload_id().to_string(),
        params,
        priority: TaskPriority::Normal,
        backend,
        client_ref: None,
        require_gpu,
        timeout_ms: None,
    }
}

fn wait_terminal(h: &Harness, id: u64, secs: u64) -> GpuTask {
    let deadline = Instant::now() + Duration::from_secs(secs);
    loop {
        if let Some(task) = h.scheduler.list(500, None).into_iter().find(|t| t.id == id) {
            if matches!(
                task.status,
                TaskStatus::Succeeded
                    | TaskStatus::Failed
                    | TaskStatus::Cancelled
                    | TaskStatus::TimedOut
                    | TaskStatus::Rejected
            ) {
                return task;
            }
        }
        assert!(Instant::now() < deadline, "task {id} did not finish in {secs}s");
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn axpy() -> WorkloadParams {
    WorkloadParams::VectorAxpy { n: 65_536, a: 1.5 }
}

#[test]
fn cpu_only_mode_executes_on_cpu_and_reports_no_gpu_time() {
    let h = harness(Mode::Success, |c| c.mode = AccelerationMode::CpuOnly);
    let params = axpy();
    let task = h
        .scheduler
        .submit(request(params, None, false), assessment(&h, &params))
        .unwrap();
    let done = wait_terminal(&h, task.id, 10);
    assert_eq!(done.status, TaskStatus::Succeeded);
    assert_eq!(done.selected_backend, Some(BackendKind::Cpu));
    assert!(done.cpu_time_ms.is_some());
    assert_eq!(done.gpu_time_ms, None);
    assert_eq!(h.mock.executes.load(Ordering::SeqCst), 0);
}

#[test]
fn gpu_mode_uses_the_gpu_provider_and_reports_gpu_timing() {
    let h = harness(Mode::Success, |_| {});
    let params = axpy();
    // Pin CUDA: at 64K elements the Auto decision correctly prefers CPU.
    let task = h
        .scheduler
        .submit(request(params, Some(BackendKind::Cuda), false), assessment(&h, &params))
        .unwrap();
    let done = wait_terminal(&h, task.id, 10);
    assert_eq!(done.status, TaskStatus::Succeeded);
    assert_eq!(done.selected_backend, Some(BackendKind::Cuda));
    assert!(done.gpu_time_ms.is_some());
    assert!(done.validated);
    assert!(!done.fell_back);
    assert!(h.mock.executes.load(Ordering::SeqCst) >= 1);
}

#[test]
fn gpu_failure_falls_back_to_cpu_with_a_recorded_reason() {
    let h = harness(Mode::Fail, |_| {});
    let params = axpy();
    let task = h
        .scheduler
        .submit(request(params, Some(BackendKind::Cuda), false), assessment(&h, &params))
        .unwrap();
    let done = wait_terminal(&h, task.id, 10);
    assert_eq!(done.status, TaskStatus::Succeeded);
    assert!(done.fell_back);
    assert!(done.cpu_time_ms.is_some());
    assert_eq!(done.gpu_time_ms, None);
    assert!(done.warnings.iter().any(|w| w.contains("CPU")));
}

#[test]
fn require_gpu_never_falls_back() {
    let h = harness(Mode::Fail, |_| {});
    let params = axpy();
    let task = h
        .scheduler
        .submit(request(params, Some(BackendKind::Cuda), true), assessment(&h, &params))
        .unwrap();
    let done = wait_terminal(&h, task.id, 10);
    assert_eq!(done.status, TaskStatus::Failed);
    assert!(!done.fell_back);
    assert!(done.error.is_some());
}

#[test]
fn running_task_can_be_cancelled_cooperatively() {
    let h = harness(Mode::Hold, |_| {});
    let params = axpy();
    let task = h
        .scheduler
        .submit(request(params, Some(BackendKind::Cuda), true), assessment(&h, &params))
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    while h
        .scheduler
        .list(10, None)
        .iter()
        .all(|t| t.status != TaskStatus::Running)
    {
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(10));
    }
    h.scheduler.cancel(task.id).unwrap();
    let done = wait_terminal(&h, task.id, 10);
    assert_eq!(done.status, TaskStatus::Cancelled);
}

#[test]
fn wall_clock_timeout_marks_the_task_timed_out() {
    let h = harness(Mode::Hold, |c| c.task_timeout_ms = 500);
    let params = axpy();
    let task = h
        .scheduler
