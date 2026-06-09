# FluxCore — Phase 3 Architecture: GPU Workload Acceleration & Intelligent CPU/GPU Workload Offloading

Status: **contract baseline** for parallel subagent development.
Owner: Subagent 1 (Architecture).
Applies to: `app/` (Tauri v2 + React 19 + Rust 2021).

This document is the single source of truth for Phase 3 shapes. Every type, enum variant, command name,
JSON field name and file path below is a **contract**: other subagents implement against it. If an
implementation needs a deviation, it must be raised to the main agent, which updates this file first.

---

## 1. Overview

Phase 1 (`monitoring.rs`) collects CPU/RAM/GPU/storage/process telemetry through NVML + Windows APIs and
persists history. Phase 2 (`memory_engine/`) layers analysis, pressure detection, prediction, policies,
actions and auditing on top of each Phase 1 snapshot. Both engines are `Arc`-held in `AppState` and driven
by the single monitor loop in `monitoring::monitor_loop`.

Phase 3 adds a **GPU acceleration engine** that:

1. Discovers GPU **compute** providers (not just display adapters) and reports their real capabilities.
2. Executes a small, explicit registry of **supported workloads** (axpy, scale, matmul, row-sum reduction)
   on CPU and on GPU, with identical semantics and **correctness validation against the CPU reference**.
3. Decides per-workload whether GPU execution is genuinely beneficial, using input-size-based models and
   measured history — never arbitrary utilization thresholds.
4. Schedules tasks with bounded concurrency, priorities, cooperative cancellation, timeouts, backpressure
   and a VRAM budget.
5. Enforces safety: VRAM budget (default 50 % of dedicated VRAM), thermal/power checks from NVML, utilization
   caps, guaranteed cleanup.
6. Benchmarks CPU vs GPU honestly (warm-up + N runs, medians, reports slowdowns), records the environment,
   persists to SQLite.
7. Exposes all of it to React through ~14 Tauri commands with camelCase JSON.

**Non-goals (explicit):** no interception of other applications, no migration of arbitrary process memory to
VRAM, no overclocking, no kernel drivers, no admin requirement, no claims that Phase 3 turns VRAM into RAM.
GPU acceleration is opt-in per workload and user-controlled.

### Environment facts that shape the design

| Fact | Consequence |
| --- | --- |
| CUDA **toolkit** not installed (no `nvcc`, no `CUDA_PATH`) | No build-time CUDA compilation, no Runtime API/NVRTC. CUDA compute must be **embedded PTX** loaded at runtime. |
| `nvcuda.dll` (driver API) + `nvml.dll` present, driver 591.59, CC 7.5 | Driver API (`cuInit`, `cuDeviceGet`, `cuModuleLoadData`, `cuLaunchKernel`) works via `libloading`. NVML works for telemetry. |
| Vulkan/DX12 present (WDDM 591.59) | **wgpu** (WGSL) is a vendor-neutral compute path needing no toolkit. |
| RTX 2050 4 GB VRAM, 8 GB system RAM | Small budgets; WDDM TDR risk on long kernels; keep all kernels short and bounded. |
| `cargo` not on PATH | Use `C:\Users\hdgan\.cargo\bin\cargo.exe` (rustc 1.99.1) for all builds/tests. |

Latest `wgpu` on crates.io is **30.0.2** (MSRV 1.87). `pollster` latest is **1.0.1**. We use
`wgpu = "30"` with `default-features = false` (see §11).

---

## 2. Architecture

### 2.1 Component diagram

```
+------------------------------------------------------------------------------------------------------+
| REACT FRONTEND  (app/src)                                                                            |
|  pages/GpuAcceleration.tsx  (tabs: Overview | Workloads | Tasks | Benchmark | Settings)           |
|    components/gpu/{GpuStatusCard, WorkloadRecommendationRow, TaskMonitorTable,                       |
|                     BenchmarkComparisonChart, ResourceBudgetPanel, BackendBadge}.tsx                |
|  stores/gpu.ts (zustand, localStorage 'fluxcore.gpu.preferences.v1')                                 |
|  services/gpu.ts (invoke wrappers)                                                                   |
|  types/gpu.ts (serde camelCase mirrors)                                                              |
+------------------------------------------|-----------------------------------------------------------+
                                           | invoke(...)  /  listen('fluxcore-gpu-task')
+------------------------------------------v-----------------------------------------------------------+
| TAURI COMMANDS  (app/src-tauri/src/lib.rs -> gpu_engine commands, registered in generate_handler!)   |
+------------------------------------------|-----------------------------------------------------------+
| GpuEngine  (gpu_engine/mod.rs) — Arc<GpuEngine> in AppState { engine, memory, gpu }                  |
|  +-- capability.rs   provider discovery + capability matrix + environment recording                   |
|  +-- providers/      trait GpuProvider -> CpuProvider, CudaProvider, WgpuProvider                    |
|  +-- resources.rs    VRAM budget, thermal/power gate, utilization cap, admission control              |
|  +-- profiler.rs     workload metadata, size->cost models, decision classification                   |
|  +-- scheduler.rs    queue, priorities, concurrency, cancellation, timeouts, backpressure            |
|  +-- execution.rs    task lifecycle, timing breakdown, correctness validation, fallback               |
|  +-- benchmark.rs    warm-up + N runs, medians, honest speedup, environment record                    |
|  +-- persistence.rs  migration 0004 DDL/DML helpers (thin; SQL lives in database.rs)                 |
+------------------------------------------|-----------------------------------------------------------+
            | reads telemetry (no polling of its own)          | writes
            v                                                  v
+──────────────────────────────+                    +────────────────────────────────────────────+
| monitoring::MonitorEngine    |                    | database::Database (rusqlite, WAL)                          |
|  .snapshot() -> SystemSnapshot (GpuMetrics)                 |  migration 0004_gpu_acceleration.sql                         |
|  (NVML: util, VRAM used/total/avail, temp, clock, power)    |  gpu_providers, gpu_acceleration_settings,                    |
|  (already running on the monitor thread)                    |  gpu_task_history, gpu_benchmark_results, gpu_provider_cache  |
+-------------------------------------------------------------+  gpu_workload_settings                            |
+──────────────────────────────+                              +────────────────────────────────────────────+
| memory_engine::MemoryEngine  |  (used for system-memory context in decisions; never duplicated)
|  .intelligence()             |
+------------------------------+
            |
            v
   GPU COMPUTE PROVIDERS:  CUDA (driver API + embedded PTX)   |   wgpu 30 (WGSL)   |   CPU (reference)
```

### 2.2 Module dependency rules (enforced by review)

```
mod.rs  ──owns──>  GpuEngine, AppState wiring, command impls
models  <────────  every module (leaf; no deps except serde/chrono)
capability ──>    models, providers (discovery only)
providers/* ──>   models, resources (types only), monitoring (GpuMetrics read)
profiler ──>      models, capability (read-only capability snapshot)
resources ──>     models, monitoring (snapshot), database (settings persist)
scheduler ──>     models, resources, execution
execution ──>     models, providers, profiler, resources, benchmark (timing), persistence
benchmark ──>     models, providers, execution (single-run helper)
persistence ──>   models, database
```

Hard rules:

- `models.rs` must not depend on any other gpu_engine module.
- `providers/*` must not depend on `scheduler`, `execution` or `mod`.
- `profiler.rs` and `resources.rs` must be pure/decision-only: no GPU calls.
- No module may call `MonitorEngine::snapshot()` in a tight loop; at most one snapshot per decision
  (§7.4). The engine never starts a second monitor thread.

### 2.3 Engine ownership and threading

- `GpuEngine` is constructed once in `lib.rs::run()`: `Arc::new(GpuEngine::new(Arc::clone(&database),
  Arc::clone(&monitor)))`, added to `AppState` as `gpu`.
- `GpuEngine` holds:
  - `providers: RwLock<Vec<Arc<dyn GpuProvider>>>` (CPU always present; CUDA/wgpu conditionally)
  - `capabilities: RwLock<CapabilityReport>`
  - `resources: ResourceManager` (Mutex-guarded accounting)
  - `scheduler: Scheduler` (own state machine, Mutex + Condvar)
  - `config: RwLock<AccelerationConfig>` (persisted via `settings` key `gpu_acceleration_config`)
  - `history: RwLock<TaskHistoryRing>` (bounded in-memory mirror of SQLite)
  - `benchmarks: RwLock<Vec<BenchmarkRecord>>` (latest N in memory; SQLite is source of truth)
- **Worker threads**: the scheduler owns a small fixed pool:
  - 1 GPU worker thread (`gpu-worker`) — executes GPU-eligible tasks, serialised.
  - 1 CPU worker thread (`gpu-cpu-worker`) — executes CPU-path tasks and CPU references.
  - Both are created lazily on first submit and joined in `Drop`.
- Tasks are `Send` and run off the UI thread. Tauri commands never block on GPU work: `submit_workload_task`
  enqueues and returns a `GpuTask` immediately.
- Events are emitted with `tauri::Emitter` on `app_handle` (the engine receives `AppHandle` via
  `set_app_handle`, mirroring `MonitorEngine`): `fluxcore-gpu-task` with the full `GpuTask` payload on every
  state change; `fluxcore-gpu-capabilities` when the capability report changes.

---

## 3. The GPU provider contract

### 3.1 `GpuProvider` trait (full contract)

```rust
/// A concrete GPU (or CPU) execution backend. Implementations must be cheap to
/// construct, safe to drop at any time, and must never panic across the trait
/// boundary: report failures as `GpuError`.
pub trait GpuProvider: Send + Sync {
    /// Stable id persisted in SQLite and shown in the UI, e.g. "cuda", "wgpu", "cpu".
    fn id(&self) -> &'static str;
    /// Human label, e.g. "NVIDIA CUDA (driver API)".
    fn label(&self) -> &'static str;
    /// Coarse kind for UI grouping.
    fn kind(&self) -> ProviderKind;

    /// One-time initialisation. Must be idempotent and must not block for more
    /// than ~2 s. Returns the devices this provider can actually use.
    fn initialize(&self) -> Result<Vec<GpuDeviceInfo>, GpuError>;

    /// True when `initialize` has succeeded and at least one device is usable.
    fn is_available(&self) -> bool;

    /// Devices from the most recent successful `initialize`.
    fn devices(&self) -> Vec<GpuDeviceInfo>;

    /// Live capability matrix (compute caps, limits, backend/quirk notes).
    fn capabilities(&self) -> ProviderCapabilities;

    /// Workload ids this provider claims to implement, with per-impl limits.
    fn supported_workloads(&self) -> Vec<WorkloadSupport>;

    /// Memory snapshot as the provider sees it (may be `None` for CPU).
    /// Implementations SHOULD NOT call NVML themselves: prefer values handed in
    /// from the MonitorEngine snapshot. Providers that can measure their own
    /// allocations (wgpu/CUDA free-alloc queries) may refine `allocated_bytes`.
    fn memory_info(&self) -> Option<GpuMemoryInfo>;

    /// Optimistic admission check: can this provider run a task with the given
    /// requirement right now, without over-committing?
    fn can_execute(&self, requirement: &WorkloadRequirement) -> Result<(), GpuError>;

    /// Execute one workload on one device. Blocking; called from a worker thread.
    /// `params` are validated by the provider against its `WorkloadSupport`.
    /// `cancel` is checked cooperatively between chunks.
    fn execute(&self, request: &ExecutionRequest) -> Result<ExecutionOutcome, GpuError>;

    /// Release any device context held by this provider (called on shutdown and
    /// after unrecoverable errors). Must be safe to call repeatedly.
    fn shutdown(&self);

    /// Last unrecoverable error, if any (surfaced in the UI).
    fn last_error(&self) -> Option<String>;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ProviderKind { Cpu, Cuda, Wgpu, Other }

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GpuDeviceInfo {
    /// Provider-local stable identifier (e.g. CUDA device ordinal, wgpu adapter index).
    pub device_id: String,
    pub name: String,
    pub vendor: String,
    pub backend: String,              // "cuda-driver", "wgpu-dx12", "wgpu-vulkan", "cpu"
    /// Matched to `GpuMetrics.name` from the monitor snapshot when possible.
    pub monitor_gpu_index: Option<usize>,
    pub dedicated_total_bytes: Option<u64>,
    pub dedicated_free_bytes: Option<u64>,
    pub compute_capability: Option<String>, // "7.5" for CUDA
    pub driver_version: Option<String>,
    pub max_workgroup_size: Option<u32>,
    pub max_storage_buffer_bytes: Option<u64>,
    /// False for integrated GPUs / CPU: used to pick safe defaults.
    pub is_discrete: bool,
    pub notes: Vec<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderCapabilities {
    /// True only when a compute path is genuinely usable (context/module created).
    pub compute_ready: bool,
    /// Why compute is not ready, when it is not.
    pub unavailable_reason: Option<String>,
    pub max_threads_per_workgroup: Option<u32>,
    pub max_workgroup_size_x: Option<u32>,
    pub max_workgroup_size_y: Option<u32>,
    pub max_workgroup_size_z: Option<u32>,
    pub max_buffer_bytes: Option<u64>,
    /// Allocation granularity / alignment the provider needs (bytes).
