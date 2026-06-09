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
    /// Maximum GPU tasks executing at once (1-2). Default 1.
    pub max_concurrent_gpu_tasks: usize,
    /// Maximum CPU tasks executing at once (1-4). Default 2.
    pub max_concurrent_cpu_tasks: usize,
    /// Maximum queue length before submissions are rejected (backpressure). Default 32.
    pub max_queue_length: usize,
    /// Per-task hard timeout. Default 10 s (TDR-safe).
    pub task_timeout_ms: u64,
    /// Allow GPU execution when system RAM available is below this % (safety).
    pub min_system_ram_available_percent: f32,
    /// Persist task history / benchmarks.
    pub persist_history: bool,
    /// History retention.
    pub retention_days: u32,
    /// Run a quick benchmark when a workload is first used (default true).
    pub auto_benchmark: bool,
    /// Validate every GPU result against the CPU reference (default true).
    pub validate_results: bool,
    /// Emit `fluxcore-gpu-task` events.
    pub emit_events: bool,
}
impl Default for AccelerationConfig { /* values above */ }

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkloadSetting {
    pub workload_id: String,
    pub mode: AccelerationMode,     // only Manual is honoured per workload
    pub backend: BackendKind,       // used when mode == Manual
    pub enabled: bool,
    pub selected_size: usize,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DevicePreference { pub provider_id: String, pub device_id: String }
```

Validation for `update_gpu_resource_config` (mirrors `MemoryEngine::set_config`): percentages in
`1.0..=90.0`; `vram_budget_cap_mb <= 4096`; `max_gpu_utilization_percent` in `10.0..=100.0`;
`max_temperature_c` in `40.0..=110.0`; concurrency `1..=2` GPU / `1..=4` CPU; `max_queue_length` `1..=256`;
`task_timeout_ms` `500..=60000`; `min_system_ram_available_percent` `1.0..=50.0`; `retention_days` `1..=90`.
Rejected configs return `Err(String)` and leave the stored config untouched.

### 5.5 Resource / safety status

```rust
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourceBudgetStatus {
    pub total_vram_bytes: Option<u64>,
    pub free_vram_bytes: Option<u64>,
    /// FluxCore's configured ceiling.
    pub budget_bytes: u64,
    /// Bytes reserved by running/queued GPU tasks.
    pub reserved_bytes: u64,
    /// Bytes currently allocated by our providers.
    pub allocated_bytes: u64,
    pub remaining_budget_bytes: u64,
    pub running_gpu_tasks: usize,
    pub queued_gpu_tasks: usize,
    pub max_concurrent_gpu_tasks: usize,
    pub gpu_utilization_percent: Option<f32>,
    pub temperature_c: Option<f32>,
    pub power_watts: Option<f32>,
    pub safety_level: SafetyLevel,
    /// Active gates, e.g. "GPU utilisation 91% above limit 85%".
    pub gates: Vec<String>,
    pub system_ram_available_percent: Option<f32>,
}
```

### 5.6 Benchmarks

```rust
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BenchmarkRequest {
    pub workload_id: String,
    pub params: WorkloadParams,
    pub warmup_runs: u32,       // clamped 0..=10, default 2
    pub measured_runs: u32,     // clamped 1..=50, default 5
    pub validate: bool,         // default true
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TimingStats {
    pub runs: u32,
    pub min_ms: f64,
    pub median_ms: f64,
    pub mean_ms: f64,
    pub max_ms: f64,
    pub stddev_ms: f64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BenchmarkComparison {
    pub cpu: TimingStats,
    /// Null when the GPU path was unavailable or failed.
    pub gpu: Option<TimingStats>,
    pub transfer_ms: Option<f64>,
    /// gpu_median / cpu_median; null when no GPU run succeeded.
    pub speedup: Option<f64>,
    /// Honest verdict string, e.g. "GPU was 1.8x slower at this size".
    pub verdict: String,
    pub gpu_available: bool,
    /// Reason the GPU run was skipped/failed, if so.
    pub gpu_unavailable_reason: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BenchmarkRecord {
    pub id: i64,
    pub created_at: String,
    pub workload_id: String,
    pub params: WorkloadParams,
    pub backend: BackendKind,
    pub provider_id: String,
    pub device_id: String,
    pub cpu: TimingStats,
    pub gpu: Option<TimingStats>,
    pub transfer_ms: Option<f64>,
    pub speedup: Option<f64>,
    pub vram_used_bytes: Option<u64>,
    pub validated: bool,
    pub max_abs_error: Option<f32>,
    pub verdict: String,
    pub environment: AccelerationEnvironment,
}

/// One row of the comparison table (UI-friendly subset).
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BenchmarkSummary {
    pub id: i64,
    pub created_at: String,
    pub workload_id: String,
    pub input_size: usize,
    pub backend: BackendKind,
    pub cpu_median_ms: f64,
    pub gpu_median_ms: Option<f64>,
    pub transfer_ms: Option<f64>,
    pub speedup: Option<f64>,
    pub verdict: String,
}
```

Benchmark honesty rules (Subagent 6):

- Always run warm-ups; discard them.
- Report `median` as the headline number; keep `min/mean/max/stddev`.
- `speedup = cpu_median / gpu_total_median` where `gpu_total` includes transfer + launch. Report `< 1.0`
  as a slowdown with the same prominence.
- If the GPU path fails or is unavailable, `gpu: None` + a reason — never fabricate a GPU time.
- `BenchmarkRecord.environment` records CPU, GPU, driver, wgpu version/backend, CUDA path, OS.
- Benchmarks run one at a time, are cancellable, and are bounded by `task_timeout_ms` * (warmup + runs).

---

## 6. Command contract (Tauri)

All commands return `Result<T, String>`; `Err` is a user-presentable message (no stack traces).
All JSON is camelCase via `#[serde(rename_all = "camelCase")]`.
Command names are exactly the snake_case below; frontend wrappers map 1:1.

| # | Command | Params (Rust) | Returns | Errors |
| --- | --- | --- | --- | --- |
| 1 | `get_gpu_acceleration_state` | `state: State<'_, AppState>` | `GpuAccelerationState` | never — degrades to `available:false` + reason when no provider works |
| 2 | `set_acceleration_mode` | `mode: AccelerationMode` | `GpuAccelerationState` (fresh) | `"unknown mode"` |
| 3 | `update_gpu_resource_config` | `config: AccelerationConfig` | `AccelerationConfig` (stored) | validation message (see §5.4) |
| 4 | `get_gpu_resource_config` | — | `AccelerationConfig` | never (defaults on first run) |
| 5 | `list_gpu_providers` | — | `Vec<ProviderStatus>` | never (unavailable providers are reported, not errors) |
| 6 | `probe_gpu_providers` | `force: bool` | `CapabilityReport`-lite → `Vec<ProviderStatus>` + `environment` as `GpuAccelerationState`? **Use `Vec<ProviderStatus>`** | never; each provider's failure is in its own `detail` |
| 7 | `list_supported_workloads` | — | `Vec<WorkloadView>` | never |
| 8 | `get_workload_recommendations` | `size: Option<usize>`, `workload_id: Option<String>` | `Vec<SuitabilityAssessment>` | `"unknown workload id: …"` |
| 9 | `submit_workload_task` | `request: SubmitTaskRequest` | `GpuTask` | queue full / unknown workload / unsupported params / GPU disabled when `require_gpu` |
| 10 | `cancel_task` | `task_id: u64` | `GpuTask` | `"unknown task id: …"` / `"task already finished"` |
| 11 | `list_tasks` | `limit: Option<usize>`, `status: Option<TaskStatus>` | `Vec<GpuTask>` | never |
| 12 | `clear_finished_tasks` | — | `usize` (removed count) | never |
| 13 | `get_task_history` | `limit: Option<usize>`, `workload_id: Option<String>` | `Vec<GpuTask>` (from SQLite) | never |
| 14 | `run_benchmark` | `request: BenchmarkRequest` | `BenchmarkRecord` | provider/timeout/validation errors |
| 15 | `get_benchmark_history` | `limit: Option<usize>`, `workload_id: Option<String>` | `Vec<BenchmarkRecord>` | never |
| 16 | `clear_benchmark_history` | — | `usize` | never |
| 17 | `get_gpu_environment` | — | `AccelerationEnvironment` | never |

`SubmitTaskRequest`:

```rust
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubmitTaskRequest {
    pub workload_id: String,
    pub params: WorkloadParams,
    pub priority: TaskPriority,
    /// Force a backend (Manual). `None` = engine decides.
    pub backend: Option<BackendKind>,
    /// For benchmark tasks; ordinary tasks ignore it.
    pub client_ref: Option<String>,
    /// Reject instead of falling back to CPU when the GPU is requested.
    pub require_gpu: bool,
    /// Override the default timeout.
    pub timeout_ms: Option<u64>,
}
```

Behaviour notes:

- `submit_workload_task` never blocks on GPU execution. It validates params against the `WorkloadDef`
  (size bounds, element counts), runs `profiler::classify` for the assessment, applies `resources`
  admission, and returns a `Queued`/`Rejected` task.
- If `mode == CpuOnly` and `require_gpu == true` → `Err("GPU acceleration is disabled")`.
- If the assessment says `InsufficientGpuResources` and `require_gpu == true` → task `Rejected` with the
  reason; otherwise it is queued for the CPU path.
- `cancel_task` on a running task sets the `CancelToken`; the worker returns `Cancelled` at the next
  cooperative checkpoint. The command returns immediately with `status: Cancelled` once the worker
  acknowledges (or `Running` with a note when the checkpoint is inside a driver call).
- `run_benchmark` bypasses the queue but still takes the GPU worker slot; it is subject to the same
  safety gates and timeout.

Events (Tauri `emit`, only when `emit_events` is true):

- `fluxcore-gpu-task` — payload `GpuTask` (on enqueue, start, progress, finish).
- `fluxcore-gpu-capabilities` — payload `Vec<ProviderStatus>` (after `probe_gpu_providers` or a change).

`lib.rs` registration (main agent):

```rust
pub mod gpu_engine;
struct AppState { engine: Arc<MonitorEngine>, memory: Arc<MemoryEngine>, gpu: Arc<gpu_engine::GpuEngine> }
// in run(): let gpu = Arc::new(gpu_engine::GpuEngine::new(Arc::clone(&database), Arc::clone(&engine)));
// .manage(AppState { engine, memory, gpu })
// .setup(|app| { let s = app.state::<AppState>(); s.engine.set_app_handle(app.handle().clone());
//                s.gpu.set_app_handle(app.handle().clone()); Ok(()) })
```

---

## 7. Decision engine, profiler and scheduler

### 7.1 Workload registry (fixed set for Phase 3)

| id | Title | Params | Sizes (elements) | GPU path |
| --- | --- | --- | --- | --- |
| `vector_axpy` | Vector AXPY | `n`, `a` | 64 K, 256 K, 1 M, 4 M, 16 M | CUDA PTX + WGSL |
| `vector_scale` | Vector scale | `n`, `a` | 64 K, 256 K, 1 M, 4 M, 16 M | CUDA PTX + WGSL |
| `matmul` | Matrix multiply (naive) | `m,k,n` | (256,256,256), (512,512,512), (1024,1024,1024) | CUDA PTX + WGSL |
| `row_sum` | Row-wise reduction | `rows`, `cols` | (1024,1024), (4096,1024), (4096,4096) | CUDA PTX + WGSL |

Caps (enforced by providers and profiler):

- `vector_*`: `n <= 64 * 1024 * 1024` elements (256 MB per buffer at f32; 3 buffers = 768 MB > 4 GB budget
  minus reserve, so `resources` will usually reject the top size on a 4 GB card).
- `matmul`: `m,k,n <= 2048`; naive `O(n^3)` so 1024³ is the practical default max; providers reject
  above 2048 to keep kernel time under the TDR window.
- `row_sum`: `rows*cols <= 32 * 1024 * 1024`.

### 7.2 Classification (decision order, first match wins)

```
1. workload id not in registry                 -> UnsupportedWorkload   ("no implementation registered")
2. mode == CpuOnly                             -> CpuRecommended      ("GPU acceleration is disabled")
3. no provider with compute_ready              -> AccelerationUnavailable ("no GPU compute backend is usable: <reason>")
4. no provider implements this workload        -> UnsupportedWorkload  ("CUDA does not implement matmul")
5. required_vram > remaining budget            -> InsufficientGpuResources ("needs 384 MB, budget leaves 210 MB")
6. provider memory_info free < required        -> InsufficientGpuResources ("GPU reports 120 MB free")
7. safety gate blocks (temp/power/util/ram)    -> InsufficientGpuResources? NO -> GpuAvailableNotBeneficial
                                                  with blockers[] and reason "thermal/power gate: ..."
8. estimated_gpu_total_ms * beneficial_speedup >= estimated_cpu_ms
                                                -> CpuRecommended / GpuAvailableNotBeneficial
                                                  ("transfer + launch overhead dominates at 64 K elements")
9. otherwise                                   -> GpuRecommended
```

`GpuAvailableNotBeneficial` vs `CpuRecommended`: use `CpuRecommended` when the mode is `CpuOnly` or the
user pinned the CPU; use `GpuAvailableNotBeneficial` when the GPU is usable but the model/measurements say
it would not pay off. Both carry a `reason` string.

`AccelerationUnavailable` is reserved for "no compute backend at all" — the GPU is still monitored.

### 7.3 Cost model

Per workload, `work_units` is chosen so the model is linear:

- `vector_*`: `work_units = n` (bytes moved = `n*4*3` for axpy, `n*4*2` for scale).
- `matmul`: `work_units = m*k*n` (FLOPs ≈ `2*m*k*n`).
- `row_sum`: `work_units = rows*cols`.

Estimates:

```
cpu_ms     = cpu_model.intercept_ms + cpu_model.scale_ms_per_unit * work_units
gpu_ms     = gpu_model.intercept_ms + gpu_model.scale_ms_per_unit * work_units
transfer_ms = transfer_bytes / PCIe_BW_BYTES_PER_MS   (default 10_000 MB/s ≈ 10 bytes/ns)
gpu_total_ms = gpu_ms + transfer_ms + launch_overhead_ms (0.05 ms)
```

Where possible, the model coefficients are **replaced by the most recent benchmark at that exact size**
(`EstimateSource::Measured` / `Mixed`). Initial coefficients are seeded from the first benchmark run and
persisted in `gpu_benchmark_results`; until then they are conservative placeholders documented in
`profiler.rs` (`EstimateSource::Model`) and the UI labels them as estimates.

There is no GPU-utilization-percentage-based routing anywhere in the decision path (requirement §4).

### 7.4 Live inputs

`classify()` reads exactly one `MonitorEngine::snapshot()` for: the matched `GpuMetrics` (by
`monitor_gpu_index`), CPU usage, and `MemoryEngine::intelligence().analysis.available_percent`. It is called
from the command thread (state assembly, recommendations) and from the worker before starting a task.
No background polling loop is added.

### 7.5 Scheduler semantics

- **Queues**: one priority queue (`BinaryHeap` on `(priority desc, queued_at asc, id)`), plus separate
  running sets for GPU and CPU workers.
- **Concurrency**: `max_concurrent_gpu_tasks` (1-2, default 1) and `max_concurrent_cpu_tasks` (default 2).
  A task's worker is chosen by its `selected_backend`.
- **Admission**: on submit, `resources.try_reserve(required_vram, safety)`; on finish/cancel/error,
  `resources.release(task_id)` (id-keyed so a double release cannot corrupt accounting).
- **Backpressure**: when `queued >= max_queue_length`, submit returns `Err("task queue is full ...")`.
  Benchmarks do not enter the queue but take the GPU slot, so a running benchmark blocks GPU tasks
  (reported as `GpuAvailableNotBeneficial` with reason "GPU busy with benchmark").
- **Cancellation**: `CancelToken { cancelled: Arc<AtomicBool> }` passed into `ExecutionRequest`. Providers
  check it before each chunk (per row-block for matmul, per 1/8 of the vector for axpy) and return
  `GpuError::Cancelled`. The scheduler also enforces a wall-clock timeout on the worker thread; on timeout
  it marks the task `TimedOut`, releases resources, and (for wgpu) drops the device to recover.
- **Fallback**: only when `require_gpu == false`. A GPU `ExecutionFailed`/`ResourceExhausted`/
  `CorrectnessFailed` re-runs the workload on the CPU path and records `fell_back = true`,
  `selected_backend` stays the GPU (what was attempted) and `fell_back` tells the UI the result came from
  the CPU. `CorrectnessFailed` marks the provider degraded: 3 consecutive correctness failures disable that
  provider for the session and emit a capabilities update.
- **Timeouts** are per attempt, default 10 s; matmul 1024³ is expected to fit in < 1 s on CC 7.5 with a
  naive kernel, so 10 s is generous while still far below typical TDR (2 s) per *launch*.

### 7.6 Resource manager (`resources.rs`, Subagent 4)

- `budget_bytes = min(dedicated_total * vram_budget_percent/100, vram_budget_cap_mb*1MiB)` (cap applied only
  when > 0).
- `remaining = budget_bytes - reserved_bytes - allocated_bytes`.
- Safety gates (all from the monitor snapshot, no new sensors):
  - `utilization_percent > max_gpu_utilization_percent` → block new GPU tasks.
  - `temperature_c > max_temperature_c` → block.
  - `power_watts` above the device's board power (when known) is recorded but not blocking (we cannot
    read the limit reliably through NVML without extra FFI; documented as a limitation).
  - `system available RAM% < min_system_ram_available_percent` → block (prevents RAM pressure from pinned
    host buffers on an 8 GB machine).
- `AdmissionDecision { allowed: bool, reason: Option<String>, reserved_bytes: u64 }`.
- Cleanup: `Drop for GpuEngine` → cancel all, join workers, `provider.shutdown()` for each, release all
  reservations. Every error path in `execution.rs` releases in a guard struct (`ResourceGuard`) so a panic
  or early return cannot leak a reservation.
- No VRAM is ever "freed" from other processes; FluxCore only releases what it allocated. The UI must not
  imply otherwise.

---

## 8. Backend implementation notes

### 8.1 CPU provider (reference + fallback)

`providers/cpu.rs` implements all four workloads with plain `f32` loops (chunked so `CancelToken` is
honoured). This is the reference every GPU result is validated against. It also reports
`ProviderKind::Cpu`, `compute_ready = true`, and `GpuMemoryInfo` with `allocated_bytes` tracked.

### 8.2 CUDA provider — driver API + embedded PTX (no toolkit)

- Load `"nvcuda.dll"` via `libloading` (same pattern as `NvmlSession`).
- Resolve at minimum: `cuInit`, `cuDriverGetVersion`, `cuDeviceGetCount`, `cuDeviceGet`,
  `cuDeviceGetName`, `cuDeviceGetAttribute` (CC major/minor, max threads per block, max shared mem),
  `cuDevicePrimaryCtxRetain`, `cuCtxPushCurrent_v2`/`cuCtxSetCurrent`, `cuMemAlloc_v2`, `cuMemFree_v2`,
  `cuMemcpyHtoD_v2`, `cuMemcpyDtoH_v2`, `cuMemGetInfo_v2`, `cuModuleLoadData`, `cuModuleGetFunction`,
  `cuLaunchKernel`, `cuCtxSynchronize`, `cuGetErrorString`, `cuCtxDestroy_v2`/`cuDevicePrimaryCtxRelease`.
  All are documented public driver API entry points.
- **Kernels are embedded PTX text** (`include_str!("../ptx/axpy.ptx")` etc.) compiled once by hand for
  `sm_75` and validated on the target device. `cuModuleLoadData` accepts PTX text and the driver JITs it
  for the actual device. No `nvcc`, no `CUDA_PATH`, no build script.
- PTX files live in `app/src-tauri/ptx/*.ptx` and are owned by Subagent 2. If a load fails (bad PTX,
  unsupported arch), return `GpuError::Unsupported` with the driver error string and the state shows
  "CUDA compute unavailable (PTX load failed: …)". **Never** fall back to pretending success.
- The provider does **not** call NVML. It reads device memory totals from `cuMemGetInfo_v2` and prefers the
  `GpuMetrics` values handed in by `resources.rs` for utilisation/temperature. `monitor_gpu_index` is
  matched by device name.
- Kernel launch configuration: `block = min(256, max_threads_per_block)`, `grid = ceil(n / block)`, chunked
  in slices of ≤ 1 M elements with a `CancelToken` check between chunks, so a launch is always short.
- If `cuInit` fails (e.g. no NVIDIA driver, session 0), `available = false`, `detail` = driver error
  string, and `compute_ready = false`. Phase 1/2 features are unaffected.

### 8.3 wgpu provider — WGSL compute (vendor-neutral)

- Dependencies: `wgpu = "30"` with `default-features = false, features = ["wgsl", "dx12", "vulkan"]`
  (no `metal`/`gles`/`webgpu` — Windows only, faster builds). `pollster` is **not** required: implement a
  tiny `block_on` in `providers/wgpu.rs`:
  ```rust
  fn block_on<F: std::future::Future>(future: F) -> F::Output {
      use std::task::{Context, Poll, RawWaker, RawWakerVTable, Waker};
      // noop waker; wgpu's futures are driven by the wgpu device/queue, not by IO
      ...
  }
  ```
  If the noop-waker approach proves unreliable for `request_adapter`, use `pollster = "1"` (main agent
  adds it). Decision: **custom block_on first, pollster as fallback.**
- `Instance::new(wgpu::InstanceDescriptor::from_env_or_default())` (wgpu 30 takes the descriptor **by value**;
  `Instance::new(&desc)` is the 0.19-era signature and must not be used). Backends from the descriptor's
  `backends` field (DX12 + Vulkan).
- `instance.enumerate_adapters(Backends::all())` for discovery; `adapter.request_device(&desc, None)`;
  `device.poll(PollType::Wait { submission_index: Some(index), timeout: Some(task_timeout) })` — note
  wgpu 30's `PollType` has exactly `Wait { submission_index, timeout }` and `Poll` (no `WaitAndMaintain`).
  Every submit is followed by a bounded `Wait` so a hung GPU cannot block the worker forever.
- Buffers: `wgpu::Buffer` with `MAP_READ | COPY_DST | COPY_SRC` plus `wgpu::util::DeviceExt::create_buffer_init`
  for uploads; download via `encoder.copy_buffer_to_buffer` + `buffer.slice(..).map_async(MapMode::Read)` +
  `device.poll(Wait)` inside `block_on`.
- WGSL kernels in `providers/wgsl/*.wgsl` (`include_str!`): `axpy.wgsl`, `scale.wgsl`, `matmul.wgsl`,
  `rowsum.wgsl`. Same layout/semantics as §5.3. Matmul is the naive triple loop with a `vec4`-free scalar
  kernel (simplest correct path; tiled matmul is out of scope).
- `max_storage_buffer_bytes` / `max_buffer_size` recorded into `ProviderCapabilities` and enforced against
  `WorkloadRequirement` (a 1024³ matmul needs ~12 MB per buffer, well within limits).
- Device loss / validation errors → `GpuError::ExecutionFailed` with the wgpu error text; the scheduler
  marks the provider degraded after 3 failures and drops the device to reset state.

### 8.4 `execution.rs` shared seam (Subagents 2 + 3)

```rust
impl GpuEngine {
    /// Decision + dispatch + timing + validation + fallback. Called by the
    /// scheduler worker and by run_benchmark (with `force_backend`).
    pub(crate) fn run_task(
        &self,
        task: &mut GpuTask,
        force_backend: Option<BackendKind>,
        timeout: Duration,
        cancel: CancelToken,
    ) -> Result<ExecutionRecord, GpuError>;
}
```

- Subagent 2 owns: provider selection inside the function, `ExecutionRequest` construction, calling
  `provider.execute`, converting `ExecutionOutcome` → `TimingBreakdown`, and `validate_results`.
- Subagent 3 owns: the `force_backend` / fallback / re-run control flow and the `GpuTask` mutation.
- The main agent freezes this signature before both start.

---

## 9. Database schema (migration 0004)

`database.rs` does **not** load `.sql` files. Migrations are `execute_batch` blocks inside `open_at`, guarded
by `schema_migrations`. The main agent will append this block after the migration-3 block, exactly matching
the existing style:

```rust
// Migration 4: Phase 3 GPU acceleration tables. No existing table is touched.
let v4_applied: i64 = connection
    .query_row("SELECT COUNT(*) FROM schema_migrations WHERE version = 4", [], |row| row.get(0))
    .unwrap_or(0);
if v4_applied == 0 {
    connection.execute_batch(
        "BEGIN;
         CREATE TABLE IF NOT EXISTS gpu_providers (
           provider_id TEXT PRIMARY KEY,
           label TEXT NOT NULL,
           kind TEXT NOT NULL,
           available INTEGER NOT NULL,
           compute_ready INTEGER NOT NULL,
           detail TEXT NOT NULL,
           device_count INTEGER NOT NULL,
           capabilities TEXT NOT NULL,
           last_seen TEXT NOT NULL
         );
         CREATE TABLE IF NOT EXISTS gpu_provider_cache (
           id INTEGER PRIMARY KEY CHECK (id=1),
           payload TEXT NOT NULL,
           updated_at TEXT NOT NULL
         );
         CREATE TABLE IF NOT EXISTS gpu_acceleration_settings (
           id INTEGER PRIMARY KEY CHECK (id=1),
           mode TEXT NOT NULL,
           payload TEXT NOT NULL,
           updated_at TEXT NOT NULL
         );
         CREATE TABLE IF NOT EXISTS gpu_workload_settings (
           workload_id TEXT PRIMARY KEY,
           mode TEXT NOT NULL,
           backend TEXT NOT NULL,
           enabled INTEGER NOT NULL,
           selected_size INTEGER NOT NULL,
           updated_at TEXT NOT NULL
         );
         CREATE TABLE IF NOT EXISTS gpu_task_history (
           id INTEGER PRIMARY KEY,
           task_ref TEXT,
           workload_id TEXT NOT NULL,
           backend TEXT NOT NULL,
           provider_id TEXT NOT NULL,
           device_id TEXT NOT NULL,
           status TEXT NOT NULL,
           priority TEXT NOT NULL,
           queued_at TEXT NOT NULL,
           started_at TEXT,
           finished_at TEXT,
           duration_ms INTEGER,
           queue_ms INTEGER,
           cpu_time_ms REAL,
           gpu_time_ms REAL,
           transfer_time_ms REAL,
           total_time_ms REAL,
           speedup REAL,
           vram_used_bytes INTEGER,
           max_abs_error REAL,
           validated INTEGER NOT NULL,
           fell_back INTEGER NOT NULL,
           error TEXT
         );
         CREATE INDEX IF NOT EXISTS idx_gpu_tasks_finished_at ON gpu_task_history(finished_at);
         CREATE INDEX IF NOT EXISTS idx_gpu_tasks_workload ON gpu_task_history(workload_id, finished_at);
         CREATE TABLE IF NOT EXISTS gpu_benchmark_results (
           id INTEGER PRIMARY KEY,
           created_at TEXT NOT NULL,
           workload_id TEXT NOT NULL,
           workload_params TEXT NOT NULL,
           backend TEXT NOT NULL,
           provider_id TEXT NOT NULL,
           device_id TEXT NOT NULL,
           warmup_runs INTEGER NOT NULL,
           measured_runs INTEGER NOT NULL,
           cpu_min_ms REAL NOT NULL, cpu_median_ms REAL NOT NULL,
           cpu_mean_ms REAL NOT NULL, cpu_max_ms REAL NOT NULL, cpu_stddev_ms REAL NOT NULL,
           gpu_min_ms REAL, gpu_median_ms REAL, gpu_mean_ms REAL, gpu_max_ms REAL, gpu_stddev_ms REAL,
           transfer_ms REAL,
           speedup REAL,
           vram_used_bytes INTEGER,
           validated INTEGER NOT NULL,
           max_abs_error REAL,
           verdict TEXT NOT NULL,
           environment TEXT NOT NULL
         );
         CREATE INDEX IF NOT EXISTS idx_gpu_bench_workload ON gpu_benchmark_results(workload_id, created_at);
         INSERT OR IGNORE INTO schema_migrations(version) VALUES (4);
         COMMIT;"
    ).map_err(|error| format!("apply database migration 4: {error}"))?;
}
```

Retention (in `prune_memory_data`, appended by the main agent, one extra statement):

```rust
pruned += connection.execute(
    "DELETE FROM gpu_task_history WHERE finished_at < datetime('now', ?1)
       AND rowid NOT IN (SELECT rowid FROM gpu_task_history ORDER BY rowid DESC LIMIT 5000)",
    params![cutoff],
)?;
pruned += connection.execute(
    "DELETE FROM gpu_benchmark_results WHERE created_at < datetime('now', ?1)
       AND rowid NOT IN (SELECT rowid FROM gpu_benchmark_results ORDER BY rowid DESC LIMIT 2000)",
    params![cutoff],
)?;
```

Rules: `gpu_task_history.id` is SQLite's own id (the in-memory `GpuTask.id` is separate and stored in
`task_ref`); `workload_params` and `environment` are `serde_json` text; every table is additive; existing
tables and rows are untouched; migration is idempotent (`INSERT OR IGNORE`, `IF NOT EXISTS`).

`persistence.rs` (Subagent 4) exposes exactly:

```rust
pub fn record_task(db: &Database, task: &GpuTask) -> Result<i64, String>;
pub fn recent_tasks(db: &Database, limit: usize, workload: Option<&str>) -> Result<Vec<GpuTask>, String>;
pub fn record_benchmark(db: &Database, record: &BenchmarkRecord) -> Result<i64, String>;
pub fn recent_benchmarks(db: &Database, limit: usize, workload: Option<&str>) -> Result<Vec<BenchmarkRecord>, String>;
pub fn clear_benchmarks(db: &Database) -> Result<usize, String>;
pub fn save_config(db: &Database, config: &AccelerationConfig) -> Result<(), String>;
pub fn load_config(db: &Database) -> Result<Option<AccelerationConfig>, String>;
pub fn save_workload_settings(db: &Database, settings: &[WorkloadSetting]) -> Result<(), String>;
pub fn load_workload_settings(db: &Database) -> Result<Vec<WorkloadSetting>, String>;
pub fn upsert_provider(db: &Database, status: &ProviderStatus) -> Result<(), String>;
pub fn recent_providers(db: &Database) -> Result<Vec<ProviderStatus>, String>;
```

Config persistence uses the `gpu_acceleration_settings` row (single-row JSON) — the same pattern as
`optimization_policies`. Frontend preferences that are *not* safety-relevant (selected size, last tab) live
in localStorage under `fluxcore.gpu.preferences.v1`.

---

## 10. Frontend integration plan

Files owned by Subagent 5. The main agent wires `App.tsx` and the nav only.

```
app/src/types/gpu.ts        — TS mirrors of every type in §5/§6 (SensorValue-style nullable numbers where
                              the backend can return null; no `any`).
app/src/services/gpu.ts     — invoke wrappers:
                                getGpuAccelerationState, setAccelerationMode, updateGpuResourceConfig,
                                getGpuResourceConfig, listGpuProviders, probeGpuProviders,
                                listSupportedWorkloads, getWorkloadRecommendations, submitWorkloadTask,
                                cancelTask, listTasks, clearFinishedTasks, getTaskHistory, runBenchmark,
                                getBenchmarkHistory, clearBenchmarkHistory, getGpuEnvironment
                              + onGpuTaskEvent(): UnlistenFn  (wraps listen('fluxcore-gpu-task'))
app/src/stores/gpu.ts       — zustand: state/mode/providers/workloads/resources/tasks/benchmarks/
                              environment/error; refresh(), setMode(), saveConfig(), submit(), cancel(),
                              clearFinished(), runBenchmark(), probe(); localStorage
                              'fluxcore.gpu.preferences.v1' for { lastTab, selectedSizes, autoRefresh }.
                              Mirrors stores/memory.ts (no polling faster than the monitor interval).
app/src/pages/GpuAcceleration.tsx — tab shell (shadcn Tabs) + 5 views:
                                Overview | Workload Intelligence | Acceleration Center |
                                Task Monitor | Performance Comparison
app/src/components/gpu/GpuStatusCard.tsx        — device info, utilization, VRAM used/available, budget bar
app/src/components/gpu/BackendBadge.tsx         — cuda/wgpu/cpu/unsupported badge with reason tooltip
app/src/components/gpu/WorkloadRecommendationRow.tsx — classification pill, reason, estimates, size picker,
                                                "Run" button
app/src/components/gpu/TaskMonitorTable.tsx     — task rows (status, backend, progress, timings, error),
                                                cancel action
app/src/components/gpu/BenchmarkComparisonChart.tsx — recharts grouped bar: CPU median vs GPU median +
                                                transfer, speedup label, honest verdict
app/src/components/gpu/ResourceBudgetPanel.tsx  — budget/reserved/allocated/remaining + safety gates
app/src/components/gpu/UnsupportedState.tsx     — "No GPU compute backend" empty/unsupported state
app/src/components/gpu/gpu.test.tsx             — vitest + @testing-library/react (loading/error/empty/
                                                unsupported/decision rendering)
```

Design rules: reuse `Panel`, `EmptyState`, `Detail`, `GpuMetric`, `ResourceBar`, `SensorNotice`, `Toggle`,
`SettingRow` from `components/monitoring` and `components/ui/*`; extend `App.css` with a small
`gpu-*` section only if existing classes cannot express a view (no new design language). Every number must
come from the backend; no demo data. Show `—`/`Unavailable` when the backend returns null, never a guessed 0.
Class names for the new page: `gpu-page`, `gpu-grid`, `gpu-metric`, `gpu-task-table`, `gpu-bench-chart`.

Nav (main agent): `{ label: 'GPU Acceleration', icon: MonitorCog, group: 'GPU INTELLIGENCE' }` and a route
in `pageContent` → `<GpuAccelerationPage />`. `grouped` array gains `'GPU INTELLIGENCE'`.

Store refresh cadence: refresh on tab focus and every `max(2000, refreshIntervalMs)` while the page is
visible (same pattern as `MemoryIntelligence`), and immediately on a `fluxcore-gpu-task` event.

---

## 11. Dependencies to add (main agent, `app/src-tauri/Cargo.toml`)

```toml
[dependencies]
wgpu = { version = "30", default-features = false, features = ["wgsl", "dx12", "vulkan"] }
# pollster = "1"   # only if the custom block_on in providers/wgpu.rs proves unreliable
# bytemuck = { version = "1", features = ["derive"] }  # optional, for buffer casts

[dev-dependencies]
tempfile = "3"      # already present
```

Notes:

- `libloading` and `windows` are already present; CUDA/NVML FFI needs nothing new.
- wgpu 30 requires rustc ≥ 1.87 (we have 1.99.1). Expect a long first build on an 8 GB machine
  (single-digit minutes); keep `default-features = false` to cut build time.
- Do **not** add `cust`, `cuda-sys`, `rustacuda`, or any crate that needs `nvcc` at build time.
- No new npm dependencies: recharts, zustand, lucide-react, shadcn/ui are already installed.

---

## 12. Milestones / execution order

| # | Milestone | Owner | Done when |
| --- | --- | --- | --- |
| M0 | Contract frozen (this document reviewed by main agent) | Subagent 1 + main | main agent signs off; no open type questions |
| M1 | `models.rs` + `error.rs` + migration 0004 applied; `gpu_engine` compiles as an empty module registered in `AppState` | Main + Subagent 1 | `cargo check` passes; Phase 1/2 tests green |
| M2 | Provider trait + CPU provider + registry; all four CPU paths pass correctness tests | Subagent 2 | `cargo test gpu_engine::` green (CPU) |
| M3 | wgpu provider: device discovery, 4 WGSL kernels, correctness vs CPU | Subagent 2 | `cargo test gpu_engine::providers::wgpu` green on real HW |
| M4 | CUDA provider: driver API probe, PTX load, axpy + scale kernels, correctness vs CPU; graceful "compute unavailable" when PTX fails | Subagent 2 | state shows `compute_ready` on RTX 2050; or documented unavailable reason |
| M5 | Profiler + classifier + scheduler + cancellation/timeout/fallback | Subagent 3 | scheduler tests green (auto, cpu-only, insufficient VRAM, cancel, concurrency) |
| M6 | Resource manager: budget, gates, cleanup, config persistence | Subagent 4 | safety tests green (budget, thermal block, cleanup after error) |
| M7 | Commands wired into `lib.rs`; state/history/benchmark commands return real data | Main + Subagent 2 | manual `invoke` smoke test via the app |
| M8 | Frontend page + components + store + vitest | Subagent 5 | `npm run test:frontend` green; page renders real backend data |
| M9 | Benchmark engine + example + benchmark report | Subagent 6 | `cargo run --release --example gpu_benchmark` produces a real report |
| M10 | Full test pass, regression (Phase 1/2), docs, production build | Subagent 6 + 7 + main | `npm run build`, `cargo build --release`, all tests green |

Parallelism: M2 → M3/M4 in parallel; M5 and M6 in parallel (different files); M8 can start as soon as M0 is
signed off, against the frozen types with a mocked service layer (mock only in `*.test.tsx`, never in
production code). M9 depends on M3/M4.

---

## 13. Risk register

| # | Risk | Likelihood | Impact | Mitigation |
| --- | --- | --- | --- | --- |
| R1 | **WDDM TDR** (driver resets after ~2 s GPU hang) during long kernels or benchmark runs | Medium | High | Hard caps on `m,k,n ≤ 2048` and `n ≤ 64 M`; chunked launches (≤ 1 M elements/chunk); per-launch timeout well under TDR; default timeout 10 s enforced by the worker; never run unbounded loops on the GPU |
| R2 | **4 GB VRAM** exhausted by other apps (browser, games) → allocation failure | High | Medium | Budget = 50 % of dedicated VRAM (2 GB) with a 1 GB cap; check `cuMemGetInfo`/NVML free before launch; `InsufficientGpuResources` with a clear reason; release on every exit path |
| R3 | **No CUDA toolkit** → PTX authoring errors block the CUDA path | Medium | High | PTX is hand-written for `sm_75` and validated by a unit test that loads every module and asserts launch success; if PTX load fails, the provider reports `Unsupported` with the driver error string and the app still works via wgpu/CPU |
| R4 | **PTX `sm_75`-only** → future NVIDIA GPUs with different arch still work via driver JIT (PTX is forward-compatible); pre-`sm_75` devices would fail | Low | Medium | Report CC; mark unsupported on CC < 7.5 with a documented reason; wgpu remains the fallback |
| R5 | **wgpu compile time** on 8 GB RAM / 4-core CPU (single-digit minutes, possible OOM) | Medium | Medium | `default-features = false` (dx12 + vulkan + wgsl only); no `metal`/`gles`/`webgpu`; build once in CI-like step; dev builds incremental afterwards |
| R6 | **wgpu 30 API drift** (`Instance::new` by value, `PollType::Wait { .. }`) | Medium | Low | Documented exact signatures in §8.3; Subagent 2 verifies with `cargo check` before writing kernels |
| R7 | **Async/blocking mismatch** — calling `block_on` on the Tauri command thread deadlocks | Medium | High | All GPU work runs on the dedicated worker threads; commands only enqueue/read state; `block_on` is used inside the worker only |
| R8 | **CUDA + NVML both loaded** causing conflicts (WDDM + primary context) | Low | Medium | CUDA provider creates only a primary context per device, pushes/pops per task, destroys on shutdown; NVML stays read-only telemetry (owned by Phase 1) |
| R9 | **Double accounting of VRAM** (NVML total includes FluxCore's own allocations) | High | Low | `resources.rs` reports `reserved/allocated` separately from NVML `used`; UI shows "FluxCore budget" and "total GPU usage" as distinct numbers with an explanatory note |
| R10 | **Incorrect benchmark claims** (reporting speedup without measurement) | Medium | High | `EstimateSource` marks model vs measured; verdict strings are generated from measured medians only; `gpu: None` when the GPU path did not run; docs state limitations |
| R11 | **Scheduler starvation** of CPU tasks by a flood of GPU tasks | Low | Medium | Separate worker threads and separate concurrency caps; priority queue; benchmark blocks the GPU slot only |
| R12 | **Frontend/backend type drift** | Medium | Medium | This document is the contract; Subagent 5 writes `types/gpu.ts` from §5 verbatim; main agent runs `tsc -b` in the build step |
| R13 | **Regression in Phase 1/2** (monitor loop timing, DB lock contention) | Medium | High | GPU engine reads snapshots only; DB access is short and off the monitor thread; Phase 1/2 tests must stay green; GPU history writes are rate-limited (on task finish, not per tick) |
| R14 | **DB migration failure** leaves the app unable to start | Low | High | Additive DDL only; `INSERT OR IGNORE` / `IF NOT EXISTS`; wrapped in `BEGIN/COMMIT`; test in M1 with a legacy 0001-0003 database (reuse the pattern of `migration_three_preserves_legacy_rows`) |
| R15 | **Cancellation cannot interrupt a driver call** | Medium | Low | Cooperative checkpoints between chunks; a task stuck in a driver call is reported `Running` until the call returns; the timeout marks it `TimedOut` and releases accounting; provider is marked degraded after repeated stalls |
| R16 | **`f32` validation tolerance** rejects valid GPU output (different summation order) | Medium | Low | Tolerance = `1e-3 * max(1, |ref|)`; reductions use the same accumulation order per row; matmul compares with a relative tolerance; failures are reported as `CorrectnessFailed` with the actual error, never silently ignored |

---

## 14. Acceptance mapping (requirement → design)

| Requirement (§ of the Phase 3 brief) | Where it is satisfied |
| --- | --- |
| Provider abstraction, device discovery, capabilities, cleanup, errors (§3) | `providers/mod.rs` trait, `capability.rs`, `ProviderStatus` |
| CUDA prioritised, toolkit absence detected (§3, §8) | `providers/cuda.rs`, `AccelerationEnvironment.cuda_*`, `GpuError::DriverUnavailable` |
| AMD/Intel adapter readiness without false claims (§3) | `ProviderKind::Other` reserved; wgpu covers DX12/Vulkan adapters; no vendor claims beyond tested status |
| Profiling & classification with reasons (§4) | `profiler.rs`, `SuitabilityAssessment`, six-variant `Suitability` |
| Real workloads with CPU path, GPU path, validation, timing, transfer (§5) | four workloads, `ExecutionRecord`, `TimingBreakdown`, `validate_results` |
| Warm-up + N runs, medians, honest speedups (§5, §13) | `benchmark.rs`, `BenchmarkRecord.verdict` |
| Decision engine + four execution modes (§6) | `AccelerationMode`, `classify()`, `WorkloadSetting` |
| VRAM management, no RAM claims (§7) | `resources.rs`, `ResourceBudgetStatus`, UI note in `GpuStatusCard` |
| Configurable limits, concurrency, cancellation, backpressure, cleanup (§9) | `AccelerationConfig`, `scheduler.rs`, `resources.rs` |
| Reuse Phase 1/2 monitoring (§10) | reads `MonitorEngine::snapshot()`, `MemoryEngine::intelligence()`, NVML only in Phase 1 |
| SQLite persistence + retention (§11) | migration 0004, `persistence.rs`, `prune_memory_data` |
