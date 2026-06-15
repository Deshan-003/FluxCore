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
