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
    pub allocation_alignment: u64,
    pub backend_notes: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkloadSupport {
    pub workload_id: String,
    /// Max element count this provider will attempt for this workload.
    pub max_elements: u64,
    /// Extra per-workload notes, e.g. "matmul limited to N<=1024 due to 4 GB VRAM".
    pub notes: Vec<String>,
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GpuMemoryInfo {
    pub total_bytes: Option<u64>,
    pub free_bytes: Option<u64>,
    pub used_bytes: Option<u64>,
    /// Bytes currently reserved by this provider (tracked by resources.rs).
    pub allocated_bytes: u64,
}

/// One execution attempt. `request_id` links it to the owning task.
#[derive(Clone, Debug)]
pub struct ExecutionRequest {
    pub request_id: u64,
    pub workload_id: String,
    pub device_id: String,
    pub params: WorkloadParams,
    pub timeout: Duration,
    pub cancel: CancelToken,
}

/// Result of one execution attempt on one backend.
#[derive(Clone, Debug)]
pub struct ExecutionOutcome {
    pub backend: String,
    /// Provider-reported device time (may be an estimate for CPU).
    pub device_time: Duration,
    /// Host<->device transfer time measured around the copies.
    pub transfer_time: Duration,
    /// Total wall time inside the provider call.
    pub total_time: Duration,
    /// Bytes moved across PCIe (both directions).
    pub transferred_bytes: u64,
    /// Peak bytes this execution held on the device (for accounting).
    pub peak_device_bytes: u64,
    /// Human-readable notes (e.g. "PTX JIT compiled in 3 ms").
    pub notes: Vec<String>,
    /// Serialised output checksum/digest for logging; not used for correctness.
    pub output_digest: String,
}
```

### 3.2 `GpuError` (uniform error reporting)

```rust
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum GpuError {
    /// Provider library/driver missing (e.g. nvcuda.dll not found).
    DriverUnavailable { provider: String, detail: String },
    /// Driver present but initialization failed (CUDA_ERROR_NO_DEVICE, etc.).
    InitializationFailed { provider: String, code: Option<i32>, detail: String },
    /// Device exists but cannot run this workload (CC too low, missing PTX, limits).
    Unsupported { provider: String, detail: String },
    /// Allocation or module load failed.
    ResourceExhausted { provider: String, detail: String },
    /// Kernel launch / execution error, incl. WDDM TDR (CUDA_ERROR_LAUNCH_TIMEOUT).
    ExecutionFailed { provider: String, code: Option<i32>, detail: String },
    /// Output did not match the CPU reference within tolerance.
    CorrectnessFailed { provider: String, max_abs_error: f64, tolerance: f64 },
    /// Task cancelled by the user or by timeout.
    Cancelled { provider: String },
    /// Timeout expired while waiting for the device.
    Timeout { provider: String },
    /// Anything else; keep `detail` user-presentable.
    Internal { provider: String, detail: String },
}
```

Rules for providers: never `unwrap()` on driver calls; map every nonzero CUDA/NVML return code to a
`GpuError`; never report success for a backend that did not actually run; never silently substitute another
backend — substitution is the scheduler's job (§7).

---

## 4. File ownership map

`app/src-tauri/src/gpu_engine/` (new directory; every file owned by exactly one agent, except where noted).

| File | Owner | Contents | Depends on |
| --- | --- | --- | --- |
| `gpu_engine/mod.rs` | **Main agent** (integration) | `GpuEngine` struct, `new()`, `set_app_handle`, config get/set, state assembly (`GpuAccelerationState`), history/benchmark read APIs, `Drop`. Re-exports. | all |
| `gpu_engine/models.rs` | **Subagent 1 (contract)** | Every type in §5, §6, §7, §8. No logic beyond `Default`/`as_str`. | serde, chrono |
| `gpu_engine/error.rs` | Subagent 1 (contract) | `GpuError` + `GpuResult<T>` + `From<GpuError> for String`. | serde |
| `gpu_engine/capability.rs` | Subagent 2 | Provider discovery, `CapabilityReport` assembly, environment record (driver/toolkit/OS), `detect_providers()`. | providers, models |
| `gpu_engine/providers/mod.rs` | Subagent 2 | `GpuProvider` trait, shared helpers (`round_up`, `align`, `div_ceil`), registry `all_providers()`. | models, error |
| `gpu_engine/providers/cpu.rs` | Subagent 2 | `CpuProvider`: reference implementations of all workloads, single-threaded and rayon-free. | models, error |
| `gpu_engine/providers/cuda.rs` | Subagent 2 | CUDA driver API FFI (`nvcuda.dll`), embedded PTX kernels, module load, launch, buffers, `cuMemGetInfo_v2`. | models, error |
| `gpu_engine/providers/wgpu.rs` | Subagent 2 | wgpu 30 instance/adapter/device, WGSL kernels, buffer upload/download, `block_on`. | models, error |
| `gpu_engine/resources.rs` | **Subagent 4** | `ResourceManager`: budget accounting, thermal/power/utilisation gates, `AdmissionDecision`. | models, monitoring, database |
| `gpu_engine/profiler.rs` | **Subagent 3** | `WorkloadDef`, `WorkloadRegistry`, cost models, `classify()` -> `SuitabilityAssessment`. | models, capability |
| `gpu_engine/scheduler.rs` | **Subagent 3** | Queue, priorities, concurrency, cancellation, timeout, backpressure, worker threads, `CancelToken`. | models, resources, execution |
| `gpu_engine/execution.rs` | **Subagent 2 + 3** (shared) | Task lifecycle, backend selection, run + validate + time, fallback, `GpuTask` transitions, event emission. | providers, profiler, resources, benchmark, persistence |
| `gpu_engine/benchmark.rs` | **Subagent 6** | `BenchmarkPlan`/`BenchmarkReport`, warm-up, N runs, median, honest speedup, env record. | providers, execution |
| `gpu_engine/persistence.rs` | **Subagent 4** | Thin wrappers around `Database` for migration 0004 (records, pruning, provider cache). | models, database |
| `app/src-tauri/src/database.rs` | **Main agent** | Migration 0004 DDL + Phase 3 read/write methods (one block appended; no edits to existing SQL). | models |
| `app/src-tauri/src/lib.rs` | **Main agent** | `pub mod gpu_engine;`, `AppState.gpu`, ~14 commands, `generate_handler!`. | gpu_engine |
| `app/src/types/gpu.ts` | **Subagent 5** | TS mirrors of all camelCase JSON. | — |
| `app/src/services/gpu.ts` | Subagent 5 | invoke wrappers + `listen('fluxcore-gpu-task')` helper. | types |
| `app/src/stores/gpu.ts` | Subagent 5 | zustand store + localStorage `fluxcore.gpu.preferences.v1`. | services, types |
| `app/src/pages/GpuAcceleration.tsx` + `app/src/components/gpu/*.tsx` | Subagent 5 | 5 views, loading/error/empty/unsupported states. | stores, ui, recharts |
| `app/src/App.tsx` | **Main agent** | nav entry `{ label: 'GPU Acceleration', icon: MonitorCog, group: 'GPU INTELLIGENCE' }` + route. | page |
| `app/src-tauri/Cargo.toml` | **Main agent** | deps (§11). | — |
| `app/src-tauri/migrations/0004_gpu_acceleration.sql` | Subagent 4 (author) / Main (apply) | DDL text. **Not auto-loaded by database.rs** — pasted into `open_at` by main agent. | — |
| `app/src-tauri/tests/gpu_*.rs` | **Subagent 6** | Integration tests. | all |
| `app/src-tauri/examples/gpu_benchmark.rs` | Subagent 6 | `cargo run --release --example gpu_benchmark`. | gpu_engine |
| Docs `PHASE3_IMPLEMENTATION.md`, `GPU_COMPATIBILITY.md`, `GPU_ACCELERATION_GUIDE.md`, `GPU_SAFETY.md`, `PHASE3_TEST_REPORT.md` | Subagent 7 | — | — |

No two agents edit the same file. The only shared file, `gpu_engine/execution.rs`, is split by function:
Subagent 2 owns provider dispatch and timing; Subagent 3 owns decision/fallback control flow. They must
coordinate the `run_task()` signature before coding (see §8.4) — the main agent arbitrates.

---

## 5. Shared model contract — `gpu_engine/models.rs`

```rust
use serde::{Deserialize, Serialize};

/// Execution mode, persisted. Mirrors Phase 2's `OptimizationMode` style.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AccelerationMode {
    /// Every workload runs on the CPU. GPU is still monitored.
    CpuOnly,
    /// Supported workloads run on the GPU when it is available and safe.
    GpuAcceleration,
    /// The engine decides per workload from size/benchmark data (default).
    IntelligentAuto,
    /// The user picks the backend per workload (`WorkloadSetting.mode`).
    Manual,
}
impl Default for AccelerationMode { fn default() -> Self { AccelerationMode::IntelligentAuto } }
impl AccelerationMode { pub fn as_str(&self) -> &'static str { /* "cpuOnly" | ... */ } }

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum BackendKind { Cpu, Cuda, Wgpu }

impl BackendKind {
    pub fn as_str(&self) -> &'static str { /* "cpu" | "cuda" | "wgpu" */ }
    pub fn parse(value: &str) -> Option<BackendKind> { /* inverse */ }
}

/// Result of the CPU-vs-GPU decision for one workload at one input size.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Suitability {
    GpuRecommended,
    CpuRecommended,
    GpuAvailableNotBeneficial,
    InsufficientGpuResources,
    UnsupportedWorkload,
    AccelerationUnavailable,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TaskStatus { Queued, Running, Succeeded, Failed, Cancelled, TimedOut, Rejected }

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TaskPriority { Low, Normal, High }

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SafetyLevel { Ok, Caution, Blocked }
```

### 5.1 Capability / provider state

```rust
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GpuAccelerationState {
    pub collected_at: String,
    pub mode: AccelerationMode,
    /// Top-level verdict for the UI banner.
    pub available: bool,
    /// Null when acceleration is available; otherwise a user-presentable reason.
    pub unavailable_reason: Option<String>,
    pub providers: Vec<ProviderStatus>,
    /// Workload catalogue with live recommendations and per-workload settings.
    pub workloads: Vec<WorkloadView>,
    pub resources: ResourceBudgetStatus,
    pub tasks: Vec<GpuTask>,
    pub safety_notes: Vec<String>,
    pub environment: AccelerationEnvironment,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderStatus {
    pub id: String,                  // "cuda" | "wgpu" | "cpu"
    pub label: String,
    pub kind: ProviderKind,
    pub available: bool,
    pub compute_ready: bool,
    /// User-presentable explanation when not available/ready.
    pub detail: String,
    pub devices: Vec<GpuDeviceInfo>,
    pub capabilities: ProviderCapabilities,
    pub supported_workloads: Vec<WorkloadSupport>,
    /// Selected device for this provider ("" when none).
    pub selected_device: String,
    /// Errors observed during the last probe.
    pub last_error: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AccelerationEnvironment {
    pub os_name: String,
    pub os_version: String,
    pub cpu_name: String,
    pub logical_processors: usize,
    pub system_ram_bytes: u64,
    pub gpu_name: String,
    pub gpu_vram_bytes: Option<u64>,
    pub driver_version: Option<String>,
    pub cuda_toolkit_present: bool,
    pub cuda_runtime_api_present: bool,
    /// e.g. "CUDA driver API via nvcuda.dll (no toolkit)".
    pub cuda_compute_path: String,
    pub wgpu_version: String,
    pub wgpu_backend: Option<String>,   // adapter backend name from wgpu
    pub vulkan_present: bool,
    pub dx12_present: bool,
    pub recorded_at: String,
}
```

### 5.2 Workload catalogue and per-workload view

```rust
/// Immutable description of a supported workload. Declared once in
/// `profiler::registry()`; providers and the UI both read it.
#[derive(Clone, Debug)]
pub struct WorkloadDef {
    pub id: &'static str,              // "vector_axpy" | "vector_scale" | "matmul" | "row_sum"
    pub title: &'static str,
    pub description: &'static str,
    pub category: &'static str,        // "vector" | "matrix" | "reduction"
    pub element_type: &'static str,    // "f32"
    /// Element counts the UI offers (last entry is the hard maximum).
    pub sizes: &'static [usize],
    pub default_size: usize,
    /// Multi-input workloads (matmul) need a second dimension.
    pub secondary_sizes: &'static [usize],
    /// Human-readable VRAM formula, e.g. "3 * n * 4 bytes".
    pub memory_formula: &'static str,
    /// Roughly linear cost model coefficients (see §7.3).
    pub cpu_model: CostModel,
    pub gpu_model: CostModel,
    /// Minimum speedup before GPU is called beneficial (default 1.15).
    pub beneficial_speedup: f64,
    /// The provider must implement this id to claim support.
    pub requires_gpu_path: bool,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct CostModel {
    /// time ≈ intercept_ns + scale_ns * work_units
    pub intercept_ns: f64,
    pub scale_ns_per_unit: f64,
}
```

```rust
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkloadView {
    pub id: String,
    pub title: String,
    pub description: String,
    pub category: String,
    pub sizes: Vec<usize>,
    pub selected_size: usize,
    /// Per-workload execution mode override (Manual mode uses this).
    pub mode: AccelerationMode,
    /// True when the user explicitly pinned this workload.
    pub pinned: bool,
    /// The live recommendation for `selected_size`.
    pub assessment: SuitabilityAssessment,
    /// Last known measured numbers, if a benchmark exists for this size.
    pub last_benchmark: Option<BenchmarkSummary>,
}

/// Full decision record: inputs, both cost estimates, and the reason.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SuitabilityAssessment {
    pub workload_id: String,
    pub input_size: usize,
    pub secondary_size: Option<usize>,
    pub classification: Suitability,
    /// User-presentable one-liner, e.g. "GPU is ~3.4x faster at 4M elements (measured)".
    pub reason: String,
    pub estimated_cpu_ms: Option<f64>,
    pub estimated_gpu_ms: Option<f64>,
    /// Predicted total GPU time including transfer + launch overhead.
    pub estimated_gpu_total_ms: Option<f64>,
    pub transfer_overhead_ms: Option<f64>,
    /// Estimated bytes that must cross PCIe (both directions).
    pub transfer_bytes: u64,
    pub estimated_vram_bytes: u64,
    pub required_vram_bytes: u64,
    /// Free VRAM after the budget reserve, from the monitor snapshot.
    pub available_vram_bytes: Option<u64>,
    pub gpu_budget_bytes: u64,
    /// True when the estimate comes from a stored benchmark at this size
    /// rather than the analytic model.
    pub estimate_source: EstimateSource,
    /// Which backend the decision would select.
    pub selected_backend: BackendKind,
    /// False when the user disabled GPU acceleration globally.
    pub gpu_enabled: bool,
    /// Safety gates that blocked GPU execution, if any.
    pub blockers: Vec<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum EstimateSource { Model, Measured, Mixed }
```

### 5.3 Workload parameters, results, tasks

```rust
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum WorkloadParams {
    /// out = a*x + y
    VectorAxpy { n: usize, a: f32 },
    /// out = a*x
    VectorScale { n: usize, a: f32 },
    /// out = A(m,k) * B(k,n)
    Matmul { m: usize, k: usize, n: usize },
    /// out[i] = sum(row i of A(rows, cols))
    RowSum { rows: usize, cols: usize },
}

impl WorkloadParams {
    pub fn workload_id(&self) -> &'static str;
    pub fn input_size(&self) -> usize;          // primary element count
    pub fn secondary_size(&self) -> Option<usize>;
    pub fn total_elements(&self) -> u64;
    /// Bytes that must be transferred host->device and device->host.
    pub fn transfer_bytes(&self) -> u64;
    /// Bytes of device memory the workload needs (inputs + output).
    pub fn device_bytes(&self) -> u64;
    pub fn work_units(&self) -> f64;            // for cost models
    pub fn seed(&self) -> u64;                  // deterministic test data
    /// CPU reference implementation. Providers must match it.
    pub fn run_cpu(&self, out: &mut [f32]) -> Result<(), String>;
    /// Byte-exact-ish comparison used to validate GPU output.
    pub fn validate(&self, reference: &[f32], candidate: &[f32], tolerance: f32) -> Result<f32, String>;
    /// Stable digest of the output, for logging/history.
    pub fn digest(&self, values: &[f32]) -> String;
}
```

Layout rules the provider and CPU paths must agree on (contract):

- `VectorAxpy` / `VectorScale`: `x`, `y`, `out` are `f32` slices of length `n`.
- `Matmul`: `a` is row-major `m*k`, `b` is row-major `k*n`, `out` is row-major `m*n`.
- `RowSum`: `a` is row-major `rows*cols`, `out` has length `rows`.

```rust
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TimingBreakdown {
    /// Time to build/validate inputs on the host.
    pub setup_ns: u64,
    /// Host->device bytes transfer.
    pub transfer_in_ns: u64,
    /// Kernel/dispatch time (device-measured where possible).
    pub compute_ns: u64,
    /// Device->host transfer.
    pub transfer_out_ns: u64,
    /// Result validation against the CPU reference.
    pub validation_ns: u64,
    /// End-to-end wall time of the execution attempt.
    pub total_ns: u64,
    pub transferred_bytes: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecutionRecord {
    pub backend: BackendKind,
    pub provider_id: String,
    pub device_id: String,
    pub timing: TimingBreakdown,
    pub peak_vram_bytes: u64,
    /// Max absolute error vs the CPU reference, when validated.
    pub max_abs_error: Option<f32>,
    pub validated: bool,
    pub notes: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GpuTask {
    pub id: u64,
    /// Client-supplied id for idempotency (optional).
    pub client_ref: Option<String>,
    pub workload_id: String,
    pub title: String,
    pub params: WorkloadParams,
    pub status: TaskStatus,
    pub priority: TaskPriority,
    /// What the engine decided, and why.
    pub requested_backend: Option<BackendKind>,
    pub selected_backend: Option<BackendKind>,
    pub assessment: Option<SuitabilityAssessment>,
    pub queued_at: String,
    pub started_at: Option<String>,
    pub finished_at: Option<String>,
    /// Wall time in ms (queued time excluded).
    pub duration_ms: Option<u64>,
    /// Queue wait in ms.
    pub queue_ms: Option<u64>,
    pub progress_percent: Option<f32>,
    pub cpu_time_ms: Option<f64>,
    pub gpu_time_ms: Option<f64>,
    pub transfer_time_ms: Option<f64>,
    pub total_time_ms: Option<f64>,
    /// Speedup vs the measured CPU path; honest: <1.0 means the GPU was slower.
    pub speedup: Option<f64>,
    pub vram_used_bytes: Option<u64>,
    pub max_abs_error: Option<f32>,
    pub validated: bool,
    /// True when the GPU path failed and the CPU reference produced the result.
    pub fell_back: bool,
    pub error: Option<String>,
    /// Fields the user should know about (TDR risk, memory pressure, ...).
    pub warnings: Vec<String>,
}
```

### 5.4 Configuration (persisted)

```rust
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AccelerationConfig {
    pub mode: AccelerationMode,
    /// Per-workload override; only consulted when mode == Manual.
    pub workload_settings: Vec<WorkloadSetting>,
    /// Preferred provider order; the first available one is used.
    pub provider_priority: Vec<String>,     // e.g. ["cuda", "wgpu", "cpu"]
    /// Manual device selection per provider ("" = auto).
    pub preferred_devices: Vec<DevicePreference>,
    /// Fraction of dedicated VRAM FluxCore may use (0.1 - 0.8). Default 0.5.
    pub vram_budget_percent: f32,
    /// Hard cap in MiB on top of the percentage (0 = no extra cap). Default 1024.
    pub vram_budget_cap_mb: u64,
    /// Refuse to start a GPU task above this GPU utilisation (%). Default 85.
    pub max_gpu_utilization_percent: f32,
    /// Refuse to start a GPU task above this temperature (°C). Default 85.
    pub max_temperature_c: f32,
