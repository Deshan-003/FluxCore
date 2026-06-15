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
