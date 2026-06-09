# Phase 3 — Frozen Interface Contract (main agent sign-off)

Status: **FROZEN**. Supersedes conflicting details in `PHASE3_ARCHITECTURE.md` §3/§8/§9 only where
explicitly noted below. `PHASE3_ARCHITECTURE.md` remains authoritative for *behaviour*, decision
order, workload catalogue, safety rules, UI plan and the database schema. This document is
authoritative for **exact Rust/TS signatures and file ownership**.

All subagents MUST read `PHASE3_ARCHITECTURE.md` (behaviour) and this file (signatures).
No subagent may change the global architecture or edit a file it does not own. Deviations must be
reported to the main agent, not silently applied.

## 0. Verified environment (measured 2026-11-13, this machine)

| Item | Measured value |
| --- | --- |
| OS | Windows (win32), shell Git Bash |
| CPU / GPU | NVIDIA GeForce RTX 2050, 4096 MiB dedicated VRAM, WDDM |
| Driver | 591.59 (`nvidia-smi`), CUDA driver API 13.1 |
| `nvcuda.dll`, `nvml.dll` | both present in `C:\Windows\System32` |
| `nvcc`, `CUDA_PATH` | **absent** — no CUDA toolkit |
| Rust | cargo/rustc 1.99.1 at `C:\Users\hdgan\.cargo\bin\cargo.exe` (NOT on PATH) |
| Node | v22.22.2, npm 10.9.7 |
| wgpu | 30.0.2 already in the local cargo registry cache |
| Git branch | `feature/phase3-gpu-acceleration` |

Consequences: CUDA compute uses the **driver API** (`libloading` + `nvcuda.dll`) with **embedded PTX**
(no `nvcc`, no build script, no `cust`/`cuda-sys`). A vendor-neutral **wgpu 30 WGSL** path covers
DX12/Vulkan adapters (AMD/Intel/NVIDIA) without a toolkit. `pollster = "1"` is used for `block_on`
rather than a hand-rolled no-op waker (reliability overrides §8.3's "custom first" note).

No vendor support claim may be made for hardware that was not actually exercised. AMD/Intel coverage
is via wgpu adapter discovery only; if no such adapter exists here, report it as **untested**.

## 0bis. Main-agent arbitration of reported contract conflicts (2026-11-13)

Subagent 1 reported five conflicts. Rulings, binding on every other subagent:

- **C3 (binding): `AccelerationConfig::vram_budget_percent` is a PERCENT in `1.0..=90.0`,
  default `50.0` — NOT a 0..1 fraction.** `budget_bytes = min(dedicated_total *
  vram_budget_percent / 100.0, cap_mb*MiB when cap>0)`. `models.rs` has been corrected by the main
  agent. Subagent 4 (`resources.rs`) and Subagent 5 (slider) must use percent semantics. §5.4's
  prose "fraction … Default 0.5" is overridden.
- C1: freeze §3 wins over architecture §3.1/§5.3 for `run_cpu(&inputs, ..)`, `make_inputs`,
  `ExecutionOutcome.timing`, `ExecutionRequest.validate`. Confirmed.
- C2: `WorkloadParams` uses one `#[serde(tag="kind", rename_all="camelCase")]`; `PartialEq` +
  `Copy` (not `Eq`, because of `f32`). Confirmed.
- C4: `(4096,1024)` row_sum is reachable by submitting an explicit `WorkloadParams::RowSum`, and by
  the UI's secondary-size control; `default_params` maps a single `size` to `(size,size)`. Confirmed.
- C5: unstated §5.4 defaults chosen by Subagent 1 (`min_system_ram_available_percent = 15`,
  `retention_days = 30`, etc.) are approved.

## 1. Frozen answers to §15 open questions

1. `wgpu = "30"` approved; **`pollster = "1"` approved and will be added by the main agent**.
2. Workload size catalogue in §7.1 approved. Benchmark example defaults to cheaper sizes so runs finish
   quickly; the full catalogue stays available in the UI.
3. `execution.rs` is owned **entirely by Subagent 3** (no split with Subagent 2). Subagent 2 owns only
   `providers/*` and `capability.rs`; its only obligation to execution is the `GpuProvider::execute`
   contract below. This removes the shared-file race.
4. Nav group `'GPU INTELLIGENCE'`, page label `'GPU Acceleration'` approved.
5. `gpu_task_history` written **on task completion only**, never per monitor tick, approved.

## 2. File ownership (STRICT — one writer per file)

| Owner | Files it may create/modify |
| --- | --- |
| **Main agent** | `src-tauri/Cargo.toml`, `src-tauri/src/lib.rs`, `src-tauri/src/database.rs`, `src-tauri/src/gpu_engine/mod.rs`, `src/App.tsx`, `PHASE3_CONTRACT_FREEZE.md`, `PHASE3_COMPLETION_REPORT.md`, `README.md`, `PHASE3_BENCHMARK_REPORT.md` (final edit) |
| **Subagent 1** | `src-tauri/src/gpu_engine/models.rs`, `PHASE3_ARCHITECTURE.md` (append-only: diagrams / dependency analysis / integration requirements) |
| **Subagent 2** | `src-tauri/src/gpu_engine/capability.rs`, `src-tauri/src/gpu_engine/providers/{mod,cpu,cuda,wgpu}.rs`, `src-tauri/src/gpu_engine/providers/wgsl/*.wgsl`, `src-tauri/ptx/*.ptx` |
| **Subagent 3** | `src-tauri/src/gpu_engine/{profiler,scheduler,execution}.rs` |
| **Subagent 4** | `src-tauri/src/gpu_engine/{resources,persistence}.rs` |
| **Subagent 5** | `src/types/gpu.ts`, `src/services/gpu.ts`, `src/stores/gpu.ts`, `src/pages/GpuAcceleration.tsx`, `src/components/gpu/*.tsx`, `src/components/gpu/*.test.tsx`, `src/App.css` (append `gpu-*` section only) |
| **Subagent 6** | `src-tauri/src/gpu_engine/benchmark.rs`, `src-tauri/tests/gpu_*.rs`, `src-tauri/examples/gpu_benchmark.rs`, `PHASE3_BENCHMARK_REPORT.md` (draft), `PHASE3_TEST_RESULTS.md` |
| **Subagent 7** | `PHASE3_IMPLEMENTATION.md`, `GPU_COMPATIBILITY.md`, `GPU_ACCELERATION_GUIDE.md`, `GPU_SAFETY.md`, `PHASE3_TEST_REPORT.md`, `PHASE3_REVIEW.md` |

**Nobody edits `gpu_engine/mod.rs`, `lib.rs`, `Cargo.toml`, `database.rs` or `App.tsx` except the main
agent.** The main agent has already written `mod.rs`/`lib.rs` against the signatures below, so those
files double as an executable contract.

## 3. `models.rs` (Subagent 1) — exact contents

Implement **every** type from `PHASE3_ARCHITECTURE.md` §5 (all enums/structs, exact field names and
`#[serde(rename_all = "camelCase")]`), plus §6 `SubmitTaskRequest`, plus the following additions which
resolve contract gaps. `models.rs` is a **leaf module**: it may depend only on `serde`, `chrono`,
`std`. No dependency on any other `gpu_engine` module.

```rust
// --- provider-side shared data (from architecture §3.1, MOVED here so providers
//     and scheduler can both use them without a circular dependency) ---
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ProviderKind { Cpu, Cuda, Wgpu, Other }

#[derive(Clone, Debug, Serialize, Deserialize)] #[serde(rename_all="camelCase")]
pub struct GpuDeviceInfo { /* exact §3.1 fields, all public */ }
#[derive(Clone, Debug, Default, Serialize, Deserialize)] #[serde(rename_all="camelCase")]
pub struct ProviderCapabilities { /* exact §3.1 fields */ }
#[derive(Clone, Debug, Serialize, Deserialize)] #[serde(rename_all="camelCase")]
pub struct WorkloadSupport { pub workload_id: String, pub max_elements: u64, pub notes: Vec<String> }
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize)] #[serde(rename_all="camelCase")]
pub struct GpuMemoryInfo { /* exact §3.1 fields */ }

/// Cooperative cancellation, usable by providers without depending on the scheduler.
#[derive(Clone, Debug, Default)]
pub struct CancelToken { /* Arc<AtomicBool> */ }
impl CancelToken {
    pub fn new() -> Self;
    pub fn cancel(&self);
    pub fn is_cancelled(&self) -> bool;
}

#[derive(Clone, Debug)]
pub struct WorkloadRequirement {
    pub workload_id: String,
    pub input_size: usize,
    pub device_bytes: u64,
    pub transfer_bytes: u64,
    pub timeout: std::time::Duration,
}

/// The ONLY request object passed into a provider.
#[derive(Clone, Debug)]
pub struct ExecutionRequest {
    pub request_id: u64,
    pub workload_id: String,
    pub device_id: String,
    pub params: WorkloadParams,
    pub timeout: std::time::Duration,
    pub cancel: CancelToken,
    /// When true, GPU providers MUST validate their output against the CPU reference
    /// and return GpuError::CorrectnessFailed on mismatch.
    pub validate: bool,
}

/// REPLACES §3.1's ExecutionOutcome (the three Durations were insufficient for
/// an honest transfer-in / compute / transfer-out breakdown).
#[derive(Clone, Debug)]
pub struct ExecutionOutcome {
    pub backend: String,          // "cpu" | "cuda" | "wgpu"
    pub provider_id: String,
    pub device_id: String,
    pub timing: TimingBreakdown,  // §5.3
    pub peak_device_bytes: u64,
    pub notes: Vec<String>,
    pub output_digest: String,
    pub max_abs_error: Option<f32>,
    pub validated: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)] #[serde(rename_all="camelCase")]
pub struct CapabilityReport {
    pub collected_at: String,
    pub providers: Vec<ProviderStatus>,
    pub environment: AccelerationEnvironment,
    pub notes: Vec<String>,
}

/// Deterministic input buffers shared by the CPU reference and every GPU provider.
/// Layout: for axpy x=x, y=y; scale x=x, y=[]; matmul x=A(m*k), y=B(k*n);
/// row_sum x=A(rows*cols), y=[]. All f32, all row-major.
#[derive(Clone, Debug)]
pub struct WorkloadInputs { pub x: Vec<f32>, pub y: Vec<f32> }
```

`WorkloadParams` MUST implement **all** of the following (this is real logic, not a type stub):

```rust
impl WorkloadParams {
    pub fn workload_id(&self) -> &'static str;
    pub fn input_size(&self) -> usize;
    pub fn secondary_size(&self) -> Option<usize>;
    pub fn total_elements(&self) -> u64;
    pub fn transfer_bytes(&self) -> u64;
    pub fn device_bytes(&self) -> u64;
    pub fn work_units(&self) -> f64;
    pub fn seed(&self) -> u64;
    pub fn output_len(&self) -> usize;
    pub fn make_inputs(&self) -> WorkloadInputs;              // deterministic, bounded values
    /// CPU reference. `out.len()` must equal `output_len()`.
    pub fn run_cpu(&self, inputs: &WorkloadInputs, out: &mut [f32]) -> Result<(), String>;
    /// Returns max absolute error; Err when lengths differ or error > tolerance.
    /// Tolerance rule: 1e-3 * max(1.0, |reference|) per element (R16).
    pub fn validate(&self, reference: &[f32], candidate: &[f32], tolerance: f32) -> Result<f32, String>;
    pub fn digest(&self, values: &[f32]) -> String;           // stable hex of f32 bits
}
impl AccelerationConfig { /* Default per §5.4 */ }
impl AccelerationMode { pub fn as_str(&self) -> &'static str; pub fn parse(&str) -> Option<Self>; }
impl BackendKind { pub fn as_str(&self) -> &'static str; pub fn parse(&str) -> Option<Self>; }
```

`WorkloadParams` uses `#[serde(tag="kind", rename_all="camelCase")]` exactly as in §5.3, so the TS
mirror is a discriminated union on `kind`.

Also provide `pub fn default_params(workload_id: &str, size: usize) -> Option<WorkloadParams>` so the
frontend and benchmark example can build valid params without duplicating the size catalogue.

## 4. `providers/mod.rs` (Subagent 2) — exact public surface

```rust
pub use crate::gpu_engine::models::{ /* provider-side types */ };
pub use crate::gpu_engine::error::{GpuError, GpuResult};

pub trait GpuProvider: Send + Sync {
    fn id(&self) -> &'static str;
    fn label(&self) -> &'static str;
    fn kind(&self) -> ProviderKind;
    fn initialize(&self) -> GpuResult<Vec<GpuDeviceInfo>>;   // idempotent, <= ~2 s
    fn is_available(&self) -> bool;
    fn devices(&self) -> Vec<GpuDeviceInfo>;
    fn capabilities(&self) -> ProviderCapabilities;
    fn supported_workloads(&self) -> Vec<WorkloadSupport>;
    fn memory_info(&self) -> Option<GpuMemoryInfo>;
    fn can_execute(&self, requirement: &WorkloadRequirement) -> GpuResult<()>;
    fn execute(&self, request: &ExecutionRequest) -> GpuResult<ExecutionOutcome>;
    fn shutdown(&self);
    fn last_error(&self) -> Option<String>;
}

/// Constructs CPU + CUDA + wgpu providers (not yet initialized).
pub fn all_providers() -> Vec<std::sync::Arc<dyn GpuProvider>>;
/// Calls initialize() on each and returns ALL of them, including unavailable ones
/// (unavailable providers must still be reported in the UI).
pub fn initialize_all() -> Vec<std::sync::Arc<dyn GpuProvider>>;
pub fn backend_for_provider(id: &str) -> BackendKind;
pub fn provider_for_backend(backend: BackendKind) -> &'static str;
```

Provider rules: never `unwrap()` a driver call; map every nonzero CUDA/wgpu error to a `GpuError`;
**never report success for a backend that did not run**; never silently substitute another backend;
each GPU provider must call `request.params.make_inputs()`, `request.params.run_cpu(...)` (when
`validate == true`) and `request.params.validate(...)` and return `CorrectnessFailed` with the real
`max_abs_error` on mismatch. CPU provider sets `validated = false`, `max_abs_error = None`.
All providers honour `cancel.is_cancelled()` between chunks and honour `request.timeout`.

## 5. `capability.rs` (Subagent 2)

```rust
pub fn detect_providers(providers: &[std::sync::Arc<dyn GpuProvider>]) -> CapabilityReport;
pub fn environment(providers: &[std::sync::Arc<dyn GpuProvider>]) -> AccelerationEnvironment;
pub fn status_of(provider: &dyn GpuProvider) -> ProviderStatus;
```

`environment()` must record real detected facts only: CUDA toolkit presence via `CUDA_PATH`/`nvcc`
lookup, `nvcuda.dll`/`nvml.dll` presence, driver version from the CUDA driver API (`cuDriverGetVersion`)
or NVML when available, OS via `std::env::consts::OS`, CPU name/logical processors/RAM from
`monitoring::SystemSnapshot` where possible, GPU name/VRAM from the snapshot, Vulkan/DX12 presence as
`Option`-backed booleans only when actually probed (never hardcoded `true`).

## 6. `resources.rs` (Subagent 4) — exact public surface

```rust
#[derive(Clone, Debug)]
pub struct AdmissionDecision { pub allowed: bool, pub reason: Option<String>, pub reserved_bytes: u64 }

pub struct ResourceManager { /* Arc<MonitorEngine>, Mutex<HashMap<u64,u64>> */ }
impl ResourceManager {
    pub fn new(monitor: std::sync::Arc<crate::monitoring::MonitorEngine>) -> Self;
    /// Reads exactly ONE MonitorEngine snapshot. Never spawns a polling thread.
    pub fn status(&self, config: &AccelerationConfig) -> ResourceBudgetStatus;
    pub fn budget_bytes(&self, config: &AccelerationConfig) -> u64;
    pub fn remaining_bytes(&self, config: &AccelerationConfig) -> u64;
    pub fn gates(&self, config: &AccelerationConfig) -> Vec<String>;
    /// Checks budget, remaining, safety gates and concurrency. Reserves on success.
    pub fn admit(&self, task_id: u64, req: &WorkloadRequirement, config: &AccelerationConfig) -> AdmissionDecision;
    /// Idempotent per task id; a double release must not corrupt accounting.
    pub fn reserve(&self, task_id: u64, bytes: u64) -> bool;
    pub fn release(&self, task_id: u64);
    pub fn running_gpu_tasks(&self) -> usize;
    pub fn validate_config(config: &AccelerationConfig) -> Result<(), String>;
}
```

Budget = `min(dedicated_total * pct/100, cap_mb*MiB when cap>0)`, floor at 0. Gates per §7.6 read
from the single snapshot: GPU utilization, temperature, system RAM available %. Power is recorded as a
note, not a blocker. `validate_config` enforces §5.4 ranges and returns the exact validation message.

## 7. `persistence.rs` (Subagent 4) — exact public surface (free functions)

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
```

All SQL runs through the main-agent-provided helper (already added to `database.rs`):

```rust
pub fn with_connection<T>(&self, f: impl FnOnce(&rusqlite::Connection) -> rusqlite::Result<T>) -> Result<T, String>;
```

Tables are exactly those created by migration 0004 (see `migrations/0004_gpu_acceleration.sql`), which
uses `payload` JSON columns for task/benchmark records. `recent_tasks`/`recent_benchmarks` decode the
`payload` column; the narrow SQL columns exist for indexing/filtering only. `GpuTask.id` is the
in-memory id and is stored in `task_ref`; SQLite uses its own `id`.

## 8. `profiler.rs` (Subagent 3) — exact public surface

```rust
pub fn registry() -> &'static [WorkloadDef];
pub fn find(id: &str) -> Option<&'static WorkloadDef>;
pub fn validate_params(params: &WorkloadParams) -> Result<(), String>;  // §7.1 caps
pub fn default_params(workload_id: &str, size: usize) -> Option<WorkloadParams>; // re-export models::
pub struct ClassifyInputs<'a> {
    pub config: &'a AccelerationConfig,
    pub providers: &'a [ProviderStatus],
    pub resources: &'a ResourceBudgetStatus,
    pub benchmarks: &'a [BenchmarkSummary],
    pub gpu_enabled: bool,
}
/// Deterministic, side-effect-free. Decision order is EXACTLY §7.2.
pub fn classify(params: &WorkloadParams, inputs: &ClassifyInputs<'_>) -> SuitabilityAssessment;
```

`classify` must never use GPU utilization percentage as a reason to route to the GPU; utilization is a
*safety gate* only (`blockers`). Conservative when data is missing: `EstimateSource::Model`,
`CpuRecommended`, with a reason stating what was missing. Measured estimates come only from a
`BenchmarkSummary` whose `input_size` matches.

## 9. `scheduler.rs` (Subagent 3) — exact public surface

```rust
pub type TaskObserver = std::sync::Arc<dyn Fn(&GpuTask) + Send + Sync>;

pub struct Scheduler { /* priority queue, task map, GPU + CPU worker threads */ }
impl Scheduler {
    pub fn new(
        providers: Vec<std::sync::Arc<dyn GpuProvider>>,
        resources: std::sync::Arc<ResourceManager>,
        observer: TaskObserver,
        config: AccelerationConfig,
    ) -> Self;
    pub fn submit(&self, request: SubmitTaskRequest, assessment: SuitabilityAssessment) -> Result<GpuTask, String>;
    pub fn cancel(&self, task_id: u64) -> Result<GpuTask, String>;
    pub fn list(&self, limit: usize, status: Option<TaskStatus>) -> Vec<GpuTask>;
    pub fn clear_finished(&self) -> usize;
    pub fn set_config(&self, config: AccelerationConfig);
    pub fn set_providers(&self, providers: Vec<std::sync::Arc<dyn GpuProvider>>);
}
impl Drop for Scheduler { /* cancel all, join workers */ }
```

Semantics: §7.5. Two worker threads (1 GPU serialised, CPU up to `max_concurrent_cpu_tasks`),
priority queue, backpressure at `max_queue_length`, cooperative `CancelToken`, wall-clock timeout per
attempt, `resources.admit` on submit / `resources.release` on **every** exit path (guard struct),
CPU fallback unless `require_gpu`, `observer(&task)` on every state change. Worker calls
`execution::execute_task(...)` (below). Never blocks a Tauri command.

