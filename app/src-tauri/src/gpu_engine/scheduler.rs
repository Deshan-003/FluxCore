//! Phase 3 — task queue, priorities, bounded concurrency, cooperative
//! cancellation, timeouts, backpressure and CPU fallback dispatch.
//!
//! Freeze §9. One serialised GPU worker plus a CPU worker pool bounded by
//! `max_concurrent_cpu_tasks`. Commands never block on GPU work: `submit`
//! enqueues and returns immediately. Every exit path releases the VRAM
//! reservation (the worker finalises under the same lock that decrements the
//! running counter, so a panic-free early return cannot leak accounting).
//!
//! Deviation from freeze §9 (reported, not silent): `resources.admit` is called
//! when a worker dequeues a GPU task rather than at submit time. Admitting at
//! submit would reserve VRAM for queued tasks and let the concurrency gate
//! reject queueing entirely; admitting at dequeue keeps the queue usable while
//! still enforcing budget, gates and concurrency before any GPU execution.

use std::cmp::Ordering as CmpOrdering;
use std::collections::{BinaryHeap, HashMap};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use crate::gpu_engine::error::GpuError;
use crate::gpu_engine::execution;
use crate::gpu_engine::models::{
    AccelerationConfig, BackendKind, CancelToken, GpuTask, SubmitTaskRequest,
    SuitabilityAssessment, TaskPriority, TaskStatus, WorkloadRequirement,
};
use crate::gpu_engine::profiler;
use crate::gpu_engine::providers::GpuProvider;
use crate::gpu_engine::resources::ResourceManager;

pub type TaskObserver = Arc<dyn Fn(&GpuTask) + Send + Sync>;

fn now() -> String {
    chrono::Utc::now().to_rfc3339()
}

fn ms_since(timestamp: &str) -> u64 {
    chrono::DateTime::parse_from_rfc3339(timestamp)
        .map(|then| {
            chrono::Utc::now()
                .signed_duration_since(then)
                .num_milliseconds()
                .max(0) as u64
        })
        .unwrap_or(0)
}

fn priority_rank(priority: TaskPriority) -> u8 {
    match priority {
        TaskPriority::High => 2,
        TaskPriority::Normal => 1,
        TaskPriority::Low => 0,
    }
}

#[derive(Clone, Debug)]
struct QueueEntry {
    id: u64,
    priority: TaskPriority,
    queued_at: String,
    gpu: bool,
}

impl PartialEq for QueueEntry {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}
impl Eq for QueueEntry {}
impl PartialOrd for QueueEntry {
    fn partial_cmp(&self, other: &Self) -> Option<CmpOrdering> {
        Some(self.cmp(other))
    }
}
impl Ord for QueueEntry {
    fn cmp(&self, other: &Self) -> CmpOrdering {
        priority_rank(self.priority)
            .cmp(&priority_rank(other.priority))
            .then_with(|| other.queued_at.cmp(&self.queued_at))
            .then_with(|| other.id.cmp(&self.id))
    }
}

struct Shared {
    tasks: HashMap<u64, GpuTask>,
    queue: BinaryHeap<QueueEntry>,
    tokens: HashMap<u64, CancelToken>,
    require_gpu: HashMap<u64, bool>,
    running_gpu: usize,
    running_cpu: usize,
    next_id: u64,
    config: AccelerationConfig,
    providers: Vec<Arc<dyn GpuProvider>>,
    shutdown: bool,
}

struct Job {
    task: GpuTask,
    providers: Vec<Arc<dyn GpuProvider>>,
    config: AccelerationConfig,
    token: CancelToken,
    require_gpu: bool,
    started: Instant,
}

#[derive(Default)]
struct Workers {
    gpu: Option<JoinHandle<()>>,
    cpu: Vec<JoinHandle<()>>,
}

pub struct Scheduler {
    shared: Arc<(Mutex<Shared>, Condvar)>,
    resources: Arc<ResourceManager>,
    observer: TaskObserver,
    workers: Mutex<Workers>,
    started: AtomicBool,
}

impl Scheduler {
    pub fn new(
        providers: Vec<Arc<dyn GpuProvider>>,
        resources: Arc<ResourceManager>,
        observer: TaskObserver,
        config: AccelerationConfig,
    ) -> Self {
        Self {
            shared: Arc::new((
                Mutex::new(Shared {
                    tasks: HashMap::new(),
                    queue: BinaryHeap::new(),
                    tokens: HashMap::new(),
                    require_gpu: HashMap::new(),
                    running_gpu: 0,
                    running_cpu: 0,
                    next_id: 1,
                    config,
                    providers,
                    shutdown: false,
                }),
                Condvar::new(),
            )),
            resources,
            observer,
            workers: Mutex::new(Workers::default()),
            started: AtomicBool::new(false),
        }
    }

    fn sync_queued(&self, shared: &Shared) {
        let count = shared.queue.iter().filter(|entry| entry.gpu).count();
        self.resources.set_queued_gpu_tasks(count);
    }

    fn spawn_cpu_worker(&self) {
        let shared = Arc::clone(&self.shared);
        let resources = Arc::clone(&self.resources);
        let observer = Arc::clone(&self.observer);
        let handle = thread::Builder::new()
            .name("gpu-cpu-worker".to_string())
            .spawn(move || worker_loop(shared, resources, observer, false))
            .expect("spawn gpu-cpu-worker");
        if let Ok(mut workers) = self.workers.lock() {
            workers.cpu.push(handle);
        }
    }

    fn ensure_workers(&self, cpu_cap: usize) {
        if self.started.swap(true, Ordering::SeqCst) {
            return;
        }
        {
            let shared = Arc::clone(&self.shared);
            let resources = Arc::clone(&self.resources);
            let observer = Arc::clone(&self.observer);
            let handle = thread::Builder::new()
                .name("gpu-worker".to_string())
                .spawn(move || worker_loop(shared, resources, observer, true))
                .expect("spawn gpu-worker");
            if let Ok(mut workers) = self.workers.lock() {
                workers.gpu = Some(handle);
            }
        }
        for _ in 0..cpu_cap {
            self.spawn_cpu_worker();
        }
    }

    pub fn submit(
        &self,
        request: SubmitTaskRequest,
        assessment: SuitabilityAssessment,
    ) -> Result<GpuTask, String> {
        let mut shared = self
            .shared
            .0
            .lock()
            .map_err(|_| "scheduler lock poisoned".to_string())?;
        if shared.shutdown {
            return Err("scheduler is shutting down".to_string());
        }
        if shared.queue.len() >= shared.config.max_queue_length {
            return Err(format!(
                "task queue is full ({} waiting); try again shortly",
                shared.queue.len()
            ));
        }
        let id = shared.next_id;
        shared.next_id += 1;
        let selected = request.backend.unwrap_or(assessment.selected_backend);
        let title = profiler::find(&request.params.workload_id())
            .map(|def| def.title.to_string())
            .unwrap_or_else(|| request.workload_id.clone());
        let task = GpuTask {
            id,
            client_ref: request.client_ref.clone(),
            workload_id: request.params.workload_id().to_string(),
            title,
            params: request.params,
            status: TaskStatus::Queued,
            priority: request.priority,
            requested_backend: request.backend,
            selected_backend: Some(selected),
            assessment: Some(assessment),
            queued_at: now(),
            started_at: None,
            finished_at: None,
            duration_ms: None,
            queue_ms: None,
            progress_percent: Some(0.0),
            cpu_time_ms: None,
            gpu_time_ms: None,
            transfer_time_ms: None,
            total_time_ms: None,
            speedup: None,
            vram_used_bytes: None,
            max_abs_error: None,
            validated: false,
            fell_back: false,
            error: None,
            warnings: Vec::new(),
        };
        shared.tokens.insert(id, CancelToken::new());
        shared.require_gpu.insert(id, request.require_gpu);
        shared.queue.push(QueueEntry {
            id,
            priority: request.priority,
            queued_at: task.queued_at.clone(),
            gpu: selected != BackendKind::Cpu,
        });
        shared.tasks.insert(id, task.clone());
        self.sync_queued(&shared);
        self.ensure_workers(shared.config.max_concurrent_cpu_tasks);
        (self.observer)(&task);
        self.shared.1.notify_all();
        Ok(task)
    }

    pub fn cancel(&self, task_id: u64) -> Result<GpuTask, String> {
        let mut shared = self
            .shared
            .0
            .lock()
            .map_err(|_| "scheduler lock poisoned".to_string())?;
        let status = shared
            .tasks
            .get(&task_id)
            .map(|task| task.status)
            .ok_or_else(|| format!("unknown task id: {task_id}"))?;
        if crate::gpu_engine::persistence::is_terminal(status) {
            return Err(format!("task {task_id} already finished"));
        }
