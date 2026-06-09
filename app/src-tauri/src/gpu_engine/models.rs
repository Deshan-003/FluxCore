//! Phase 3 shared data contract — `gpu_engine/models.rs`.
//!
//! This is a **leaf module**: it depends only on `serde` and `std` (and is
//! permitted to use `chrono`, though it does not need it here). It must never
//! import any other `gpu_engine` module. Every JSON-facing type uses
//! `#[serde(rename_all = "camelCase")]`; `WorkloadParams` is additionally
//! internally tagged on `kind`.
//!
//! Exact signatures: `PHASE3_CONTRACT_FREEZE.md` §3. Behaviour: §5 / §7 of
//! `PHASE3_ARCHITECTURE.md`.

use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

// ---------------------------------------------------------------------------
// Small FX helpers shared by seed() and digest() (stable, no DefaultHasher).
// ---------------------------------------------------------------------------

/// One FNV-1a mixing step over a `u64` word.
fn fnv_mix(h: &mut u64, word: u64) {
    *h ^= word;
    *h = h.wrapping_mul(0x1000_0000_01b3);
}

const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;

/// Deterministic xorshift64 generator used to build `WorkloadInputs`.
/// Values are mapped to exactly-representable `f32` in roughly `[-1, 1)`, so
/// the CPU reference and every GPU provider read identical input bits.
struct XorShift64 {
    state: u64,
}

impl XorShift64 {
    fn new(seed: u64) -> Self {
        Self {
            state: if seed == 0 { 1 } else { seed },
        }
    }

    fn next_u64(&mut self) -> u64 {
        let mut x = self.state;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.state = x;
        x
    }

    fn next_f32(&mut self) -> f32 {
        // Top 24 bits -> a value that is a multiple of 2^-24 in [0, 1),
        // then scaled to [-1, 1). Both are exact in f32 (24-bit mantissa).
        let top = (self.next_u64() >> 40) as u32;
        let unit = (top as f64) / 16_777_216.0;
        (unit * 2.0 - 1.0) as f32
    }
}

// ---------------------------------------------------------------------------
// §5 — core enums
// ---------------------------------------------------------------------------

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

impl Default for AccelerationMode {
    fn default() -> Self {
        AccelerationMode::IntelligentAuto
    }
}

impl AccelerationMode {
    pub fn as_str(&self) -> &'static str {
        match self {
            AccelerationMode::CpuOnly => "cpuOnly",
            AccelerationMode::GpuAcceleration => "gpuAcceleration",
            AccelerationMode::IntelligentAuto => "intelligentAuto",
            AccelerationMode::Manual => "manual",
        }
    }

    pub fn parse(value: &str) -> Option<AccelerationMode> {
        match value {
            "cpuOnly" => Some(AccelerationMode::CpuOnly),
            "gpuAcceleration" => Some(AccelerationMode::GpuAcceleration),
            "intelligentAuto" => Some(AccelerationMode::IntelligentAuto),
            "manual" => Some(AccelerationMode::Manual),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum BackendKind {
    Cpu,
    Cuda,
    Wgpu,
}

impl BackendKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            BackendKind::Cpu => "cpu",
            BackendKind::Cuda => "cuda",
            BackendKind::Wgpu => "wgpu",
        }
    }

    pub fn parse(value: &str) -> Option<BackendKind> {
        match value {
            "cpu" => Some(BackendKind::Cpu),
            "cuda" => Some(BackendKind::Cuda),
            "wgpu" => Some(BackendKind::Wgpu),
            _ => None,
        }
    }
}

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
pub enum TaskStatus {
    Queued,
    Running,
    Succeeded,
    Failed,
    Cancelled,
    TimedOut,
    Rejected,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TaskPriority {
    Low,
    Normal,
    High,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SafetyLevel {
    Ok,
    Caution,
    Blocked,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum EstimateSource {
    Model,
    Measured,
    Mixed,
}

// ---------------------------------------------------------------------------
// §5.1 — capability / provider state
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GpuAccelerationState {
    pub collected_at: String,
    pub mode: AccelerationMode,
    pub available: bool,
    pub unavailable_reason: Option<String>,
    pub providers: Vec<ProviderStatus>,
    pub workloads: Vec<WorkloadView>,
    pub resources: ResourceBudgetStatus,
    pub tasks: Vec<GpuTask>,
    pub safety_notes: Vec<String>,
    pub environment: AccelerationEnvironment,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderStatus {
    pub id: String,
    pub label: String,
    pub kind: ProviderKind,
    pub available: bool,
    pub compute_ready: bool,
    pub detail: String,
    pub devices: Vec<GpuDeviceInfo>,
    pub capabilities: ProviderCapabilities,
    pub supported_workloads: Vec<WorkloadSupport>,
    pub selected_device: String,
    pub last_error: Option<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
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
    pub cuda_compute_path: String,
    pub wgpu_version: String,
    pub wgpu_backend: Option<String>,
    pub vulkan_present: bool,
    pub dx12_present: bool,
    pub recorded_at: String,
}

// ---------------------------------------------------------------------------
// §5.2 — workload catalogue and per-workload view
// ---------------------------------------------------------------------------

/// Immutable description of a supported workload (declared by `profiler`).
/// Not JSON-facing: it holds `&'static` metadata.
#[derive(Clone, Debug)]
pub struct WorkloadDef {
    pub id: &'static str,
    pub title: &'static str,
    pub description: &'static str,
    pub category: &'static str,
    pub element_type: &'static str,
    pub sizes: &'static [usize],
    pub default_size: usize,
    pub secondary_sizes: &'static [usize],
    pub memory_formula: &'static str,
    pub cpu_model: CostModel,
    pub gpu_model: CostModel,
    pub beneficial_speedup: f64,
    pub requires_gpu_path: bool,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct CostModel {
    pub intercept_ns: f64,
    pub scale_ns_per_unit: f64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkloadView {
    pub id: String,
    pub title: String,
    pub description: String,
    pub category: String,
    pub sizes: Vec<usize>,
    pub selected_size: usize,
    pub mode: AccelerationMode,
    pub pinned: bool,
    pub assessment: SuitabilityAssessment,
    pub last_benchmark: Option<BenchmarkSummary>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SuitabilityAssessment {
    pub workload_id: String,
    pub input_size: usize,
    pub secondary_size: Option<usize>,
    pub classification: Suitability,
    pub reason: String,
    pub estimated_cpu_ms: Option<f64>,
    pub estimated_gpu_ms: Option<f64>,
    pub estimated_gpu_total_ms: Option<f64>,
    pub transfer_overhead_ms: Option<f64>,
    pub transfer_bytes: u64,
    pub estimated_vram_bytes: u64,
    pub required_vram_bytes: u64,
    pub available_vram_bytes: Option<u64>,
    pub gpu_budget_bytes: u64,
    pub estimate_source: EstimateSource,
    pub selected_backend: BackendKind,
    pub gpu_enabled: bool,
    pub blockers: Vec<String>,
}

// ---------------------------------------------------------------------------
// §5.3 — workload parameters, results, tasks
// ---------------------------------------------------------------------------

/// Discriminated union on `kind`. The freeze §3 describes this as a single
/// `#[serde(tag = "kind", rename_all = "camelCase")]` attribute; keeping it
/// consolidated avoids a duplicate `rename_all` attribute (the architecture
/// §5.3 showed two attributes carrying the same value).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
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

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TimingBreakdown {
    pub setup_ns: u64,
    pub transfer_in_ns: u64,
    pub compute_ns: u64,
    pub transfer_out_ns: u64,
    pub validation_ns: u64,
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
    pub max_abs_error: Option<f32>,
    pub validated: bool,
    pub notes: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GpuTask {
    pub id: u64,
    pub client_ref: Option<String>,
    pub workload_id: String,
    pub title: String,
    pub params: WorkloadParams,
    pub status: TaskStatus,
    pub priority: TaskPriority,
    pub requested_backend: Option<BackendKind>,
    pub selected_backend: Option<BackendKind>,
    pub assessment: Option<SuitabilityAssessment>,
    pub queued_at: String,
    pub started_at: Option<String>,
    pub finished_at: Option<String>,
    pub duration_ms: Option<u64>,
    pub queue_ms: Option<u64>,
    pub progress_percent: Option<f32>,
    pub cpu_time_ms: Option<f64>,
    pub gpu_time_ms: Option<f64>,
    pub transfer_time_ms: Option<f64>,
    pub total_time_ms: Option<f64>,
    pub speedup: Option<f64>,
    pub vram_used_bytes: Option<u64>,
    pub max_abs_error: Option<f32>,
    pub validated: bool,
    pub fell_back: bool,
    pub error: Option<String>,
    pub warnings: Vec<String>,
}

// ---------------------------------------------------------------------------
// §5.4 — configuration (persisted)
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AccelerationConfig {
    pub mode: AccelerationMode,
    pub workload_settings: Vec<WorkloadSetting>,
    pub provider_priority: Vec<String>,
    pub preferred_devices: Vec<DevicePreference>,
    pub vram_budget_percent: f32,
    pub vram_budget_cap_mb: u64,
    pub max_gpu_utilization_percent: f32,
    pub max_temperature_c: f32,
    pub max_concurrent_gpu_tasks: usize,
    pub max_concurrent_cpu_tasks: usize,
    pub max_queue_length: usize,
    pub task_timeout_ms: u64,
    pub min_system_ram_available_percent: f32,
    pub persist_history: bool,
    pub retention_days: u32,
    pub auto_benchmark: bool,
    pub validate_results: bool,
    pub emit_events: bool,
}

impl Default for AccelerationConfig {
    fn default() -> Self {
        AccelerationConfig {
            mode: AccelerationMode::IntelligentAuto,
            workload_settings: Vec::new(),
            provider_priority: vec![
                "cuda".to_string(),
                "wgpu".to_string(),
                "cpu".to_string(),
            ],
            preferred_devices: Vec::new(),
            // Main-agent arbitration (conflict C3 from Subagent 1): this field is
            // a PERCENT (0-100), not a 0-1 fraction. §5.4's validation range
            // (1..=90) and §7.6's "/100" budget formula are authoritative; the
            // UI also renders it as a percentage. Default = 50 % of dedicated VRAM.
            vram_budget_percent: 50.0,
            vram_budget_cap_mb: 1024,
            max_gpu_utilization_percent: 85.0,
            max_temperature_c: 85.0,
            max_concurrent_gpu_tasks: 1,
            max_concurrent_cpu_tasks: 2,
            max_queue_length: 32,
            task_timeout_ms: 10_000,
            min_system_ram_available_percent: 15.0,
            persist_history: true,
            retention_days: 30,
            auto_benchmark: true,
            validate_results: true,
            emit_events: true,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkloadSetting {
    pub workload_id: String,
    pub mode: AccelerationMode,
    pub backend: BackendKind,
    pub enabled: bool,
    pub selected_size: usize,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DevicePreference {
    pub provider_id: String,
    pub device_id: String,
}

// ---------------------------------------------------------------------------
// §5.5 — resource / safety status
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourceBudgetStatus {
    pub total_vram_bytes: Option<u64>,
    pub free_vram_bytes: Option<u64>,
    pub budget_bytes: u64,
    pub reserved_bytes: u64,
    pub allocated_bytes: u64,
    pub remaining_budget_bytes: u64,
    pub running_gpu_tasks: usize,
    pub queued_gpu_tasks: usize,
    pub max_concurrent_gpu_tasks: usize,
    pub gpu_utilization_percent: Option<f32>,
    pub temperature_c: Option<f32>,
    pub power_watts: Option<f32>,
    pub safety_level: SafetyLevel,
    pub gates: Vec<String>,
    pub system_ram_available_percent: Option<f32>,
}

// ---------------------------------------------------------------------------
// §5.6 — benchmarks
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BenchmarkRequest {
    pub workload_id: String,
    pub params: WorkloadParams,
    pub warmup_runs: u32,
    pub measured_runs: u32,
    pub validate: bool,
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
    pub gpu: Option<TimingStats>,
    pub transfer_ms: Option<f64>,
    pub speedup: Option<f64>,
    pub verdict: String,
    pub gpu_available: bool,
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

// ---------------------------------------------------------------------------
// §6 — submit request
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubmitTaskRequest {
    pub workload_id: String,
    pub params: WorkloadParams,
    pub priority: TaskPriority,
    pub backend: Option<BackendKind>,
    pub client_ref: Option<String>,
    pub require_gpu: bool,
    pub timeout_ms: Option<u64>,
}

// ---------------------------------------------------------------------------
// Freeze §3 additions — provider-side shared data (moved here to avoid a
// providers <-> scheduler circular dependency).
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ProviderKind {
    Cpu,
    Cuda,
    Wgpu,
    Other,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GpuDeviceInfo {
    pub device_id: String,
    pub name: String,
    pub vendor: String,
    pub backend: String,
    pub monitor_gpu_index: Option<usize>,
    pub dedicated_total_bytes: Option<u64>,
    pub dedicated_free_bytes: Option<u64>,
    pub compute_capability: Option<String>,
    pub driver_version: Option<String>,
    pub max_workgroup_size: Option<u32>,
    pub max_storage_buffer_bytes: Option<u64>,
    pub is_discrete: bool,
    pub notes: Vec<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderCapabilities {
    pub compute_ready: bool,
    pub unavailable_reason: Option<String>,
    pub max_threads_per_workgroup: Option<u32>,
    pub max_workgroup_size_x: Option<u32>,
    pub max_workgroup_size_y: Option<u32>,
    pub max_workgroup_size_z: Option<u32>,
    pub max_buffer_bytes: Option<u64>,
    pub allocation_alignment: u64,
    pub backend_notes: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkloadSupport {
    pub workload_id: String,
    pub max_elements: u64,
    pub notes: Vec<String>,
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GpuMemoryInfo {
    pub total_bytes: Option<u64>,
    pub free_bytes: Option<u64>,
    pub used_bytes: Option<u64>,
    pub allocated_bytes: u64,
}

/// Cooperative cancellation, usable by providers without depending on the
/// scheduler. Cloning shares the same flag.
#[derive(Clone, Debug, Default)]
pub struct CancelToken {
    cancelled: Arc<AtomicBool>,
}

impl CancelToken {
    pub fn new() -> Self {
        CancelToken {
            cancelled: Arc::new(AtomicBool::new(false)),
        }
    }

    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::SeqCst);
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::SeqCst)
    }
}

#[derive(Clone, Debug)]
pub struct WorkloadRequirement {
    pub workload_id: String,
    pub input_size: usize,
    pub device_bytes: u64,
    pub transfer_bytes: u64,
    pub timeout: std::time::Duration,
}

/// The ONLY request object passed into a provider (freeze §3 shape; adds the
/// `validate` flag over architecture §3.1).
#[derive(Clone, Debug)]
pub struct ExecutionRequest {
    pub request_id: u64,
    pub workload_id: String,
    pub device_id: String,
    pub params: WorkloadParams,
    pub timeout: std::time::Duration,
    pub cancel: CancelToken,
    pub validate: bool,
}

/// Result of one execution attempt. Freeze §3 shape: `timing: TimingBreakdown`
/// replaces architecture §3.1's three `Duration` fields.
#[derive(Clone, Debug)]
pub struct ExecutionOutcome {
    pub backend: String,
    pub provider_id: String,
    pub device_id: String,
    pub timing: TimingBreakdown,
    pub peak_device_bytes: u64,
    pub notes: Vec<String>,
    pub output_digest: String,
    pub max_abs_error: Option<f32>,
    pub validated: bool,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CapabilityReport {
    pub collected_at: String,
    pub providers: Vec<ProviderStatus>,
    pub environment: AccelerationEnvironment,
    pub notes: Vec<String>,
}

/// Deterministic input buffers shared by the CPU reference and every GPU
/// provider. Row-major f32, `y` empty for VectorScale and RowSum.
#[derive(Clone, Debug)]
pub struct WorkloadInputs {
    pub x: Vec<f32>,
    pub y: Vec<f32>,
}

// ---------------------------------------------------------------------------
// WorkloadParams — the shared computation contract.
// ---------------------------------------------------------------------------

impl WorkloadParams {
    pub fn workload_id(&self) -> &'static str {
        match *self {
            WorkloadParams::VectorAxpy { .. } => "vector_axpy",
            WorkloadParams::VectorScale { .. } => "vector_scale",
            WorkloadParams::Matmul { .. } => "matmul",
            WorkloadParams::RowSum { .. } => "row_sum",
        }
    }

    /// Primary element count / dimension.
    pub fn input_size(&self) -> usize {
        match *self {
            WorkloadParams::VectorAxpy { n, .. }
            | WorkloadParams::VectorScale { n, .. } => n,
            WorkloadParams::Matmul { m, .. } => m,
            WorkloadParams::RowSum { rows, .. } => rows,
        }
    }

    pub fn secondary_size(&self) -> Option<usize> {
        match *self {
            WorkloadParams::VectorAxpy { .. }
            | WorkloadParams::VectorScale { .. } => None,
            WorkloadParams::Matmul { n, .. } => Some(n),
            WorkloadParams::RowSum { cols, .. } => Some(cols),
        }
    }

    /// (x length, y length, out length) in elements. Saturating products keep
    /// this panic-free even for hostile, directly-constructed params.
    fn layout(&self) -> (usize, usize, usize) {
        match *self {
            WorkloadParams::VectorAxpy { n, .. } => (n, n, n),
            WorkloadParams::VectorScale { n, .. } => (n, 0, n),
            WorkloadParams::Matmul { m, k, n } => (
                m.saturating_mul(k),
                k.saturating_mul(n),
                m.saturating_mul(n),
            ),
            WorkloadParams::RowSum { rows, cols } => {
                (rows.saturating_mul(cols), 0, rows)
            }
        }
    }

    /// Total f32 values across all buffers (inputs + output).
    pub fn total_elements(&self) -> u64 {
        let (x, y, out) = self.layout();
        (x as u64)
            .saturating_add(y as u64)
            .saturating_add(out as u64)
    }

    /// Host<->device bytes, both directions (inputs uploaded + output
    /// downloaded), 4 bytes per f32.
    pub fn transfer_bytes(&self) -> u64 {
        self.total_elements().saturating_mul(4)
    }

    /// Bytes of all device buffers (inputs + output resident at once).
    pub fn device_bytes(&self) -> u64 {
        self.total_elements().saturating_mul(4)
    }

    pub fn work_units(&self) -> f64 {
        match *self {
            WorkloadParams::VectorAxpy { n, .. }
            | WorkloadParams::VectorScale { n, .. } => n as f64,
            WorkloadParams::Matmul { m, k, n } => {
                (m as f64) * (k as f64) * (n as f64)
            }
            WorkloadParams::RowSum { rows, cols } => {
                (rows as f64) * (cols as f64)
            }
        }
    }

    /// Deterministic, non-zero seed derived from the parameters.
    pub fn seed(&self) -> u64 {
        let mut h = FNV_OFFSET;
        match *self {
            WorkloadParams::VectorAxpy { n, a } => {
                fnv_mix(&mut h, 1);
                fnv_mix(&mut h, n as u64);
                fnv_mix(&mut h, a.to_bits() as u64);
            }
            WorkloadParams::VectorScale { n, a } => {
                fnv_mix(&mut h, 2);
                fnv_mix(&mut h, n as u64);
                fnv_mix(&mut h, a.to_bits() as u64);
            }
            WorkloadParams::Matmul { m, k, n } => {
                fnv_mix(&mut h, 3);
                fnv_mix(&mut h, m as u64);
                fnv_mix(&mut h, k as u64);
                fnv_mix(&mut h, n as u64);
            }
            WorkloadParams::RowSum { rows, cols } => {
                fnv_mix(&mut h, 4);
                fnv_mix(&mut h, rows as u64);
                fnv_mix(&mut h, cols as u64);
            }
        }
        h | 1
    }

    pub fn output_len(&self) -> usize {
        self.layout().2
    }

    /// Deterministic, bounded input buffers. `x` first, then `y`, on one PRNG
    /// stream so CPU and GPU see identical bits.
    pub fn make_inputs(&self) -> WorkloadInputs {
        let (x_len, y_len, _) = self.layout();
        let mut rng = XorShift64::new(self.seed());
        let x = (0..x_len).map(|_| rng.next_f32()).collect::<Vec<f32>>();
        let y = (0..y_len).map(|_| rng.next_f32()).collect::<Vec<f32>>();
        WorkloadInputs { x, y }
    }

    /// CPU reference. `out.len()` must equal `output_len()`; input buffers
    /// must be at least the expected lengths.
    pub fn run_cpu(
        &self,
        inputs: &WorkloadInputs,
        out: &mut [f32],
    ) -> Result<(), String> {
        let (x_len, y_len, out_len) = self.layout();
        if out.len() != out_len {
            return Err(format!(
                "output length {} does not match required {}",
                out.len(),
                out_len
            ));
        }
        if inputs.x.len() < x_len || inputs.y.len() < y_len {
            return Err(format!(
                "input lengths x={} y={} are below required x={} y={}",
                inputs.x.len(),
                inputs.y.len(),
                x_len,
                y_len
            ));
        }
        let x = &inputs.x[..x_len];
        let y = &inputs.y[..y_len];

        match *self {
            WorkloadParams::VectorAxpy { n, a } => {
                for i in 0..n {
                    out[i] = a * x[i] + y[i];
                }
            }
            WorkloadParams::VectorScale { n, a } => {
                for i in 0..n {
                    out[i] = a * x[i];
                }
            }
            WorkloadParams::Matmul { m, k, n } => {
                // A = x (m*k), B = y (k*n), out = m*n. Ascending t.
                for i in 0..m {
                    for j in 0..n {
                        let mut sum = 0.0f32;
                        for t in 0..k {
                            sum += x[i * k + t] * y[t * n + j];
                        }
                        out[i * n + j] = sum;
                    }
                }
            }
            WorkloadParams::RowSum { rows, cols } => {
                // A = x (rows*cols). Ascending column accumulation.
                for i in 0..rows {
                    let mut sum = 0.0f32;
                    for c in 0..cols {
                        sum += x[i * cols + c];
                    }
                    out[i] = sum;
                }
            }
        }
        Ok(())
    }

    /// Returns max absolute error; Err on length mismatch, non-finite values,
    /// or error > max(1e-3 * max(1, |reference|), tolerance) per element.
    pub fn validate(
        &self,
        reference: &[f32],
        candidate: &[f32],
        tolerance: f32,
    ) -> Result<f32, String> {
        if reference.len() != candidate.len() {
            return Err(format!(
                "output length mismatch: reference {} values, candidate {} values",
                reference.len(),
                candidate.len()
            ));
        }
        let tol = if tolerance.is_finite() && tolerance >= 0.0 {
            tolerance
        } else {
            0.0
        };
        let mut max_err = 0.0f32;
        for (i, (&r, &c)) in reference.iter().zip(candidate.iter()).enumerate() {
            if !r.is_finite() || !c.is_finite() {
                return Err(format!(
                    "non-finite value at element {i}: reference {r}, candidate {c}"
                ));
            }
            let err = (r - c).abs();
            if err > max_err {
                max_err = err;
            }
            let allowed = (1.0e-3f32 * r.abs().max(1.0f32)).max(tol);
            if err > allowed {
                return Err(format!(
                    "correctness check failed at element {i}: |{r} - {c}| = {err:.6} exceeds tolerance {allowed:.6}"
                ));
            }
        }
        Ok(max_err)
    }

    /// Stable FNV-1a over `f32::to_bits()`, 16 lowercase hex chars.
    pub fn digest(&self, values: &[f32]) -> String {
        let mut h = FNV_OFFSET;
        for &v in values {
            for byte in v.to_bits().to_le_bytes() {
                fnv_mix(&mut h, byte as u64);
            }
        }
        format!("{:016x}", h)
    }
}

// ---------------------------------------------------------------------------
// Free function — build valid params from a workload id + size.
// ---------------------------------------------------------------------------

/// Build `WorkloadParams` from §7.1's catalogue. Also accepts arbitrary
/// smaller positive sizes (matmul -> (size,size,size), row_sum -> (size,size)).
/// Returns `None` for unknown ids, zero size, or sizes above the hard caps.
pub fn default_params(workload_id: &str, size: usize) -> Option<WorkloadParams> {
    if size == 0 {
        return None;
    }
    const VECTOR_MAX: usize = 64 * 1024 * 1024;
    const MATMUL_MAX: usize = 2048;
    const ROW_SUM_MAX_ELEMENTS: u64 = 32 * 1024 * 1024;

    match workload_id {
        "vector_axpy" => {
            if size > VECTOR_MAX {
                return None;
            }
            Some(WorkloadParams::VectorAxpy { n: size, a: 1.5f32 })
        }
        "vector_scale" => {
            if size > VECTOR_MAX {
                return None;
            }
            Some(WorkloadParams::VectorScale { n: size, a: 2.0f32 })
        }
        "matmul" => {
            if size > MATMUL_MAX {
                return None;
            }
            Some(WorkloadParams::Matmul {
                m: size,
                k: size,
                n: size,
            })
        }
        "row_sum" => {
            // Square (size,size). §7.1's square entries (1024,1024) and
            // (4096,4096) fall out naturally; the wide (4096,1024) entry is
            // reachable via an explicit `WorkloadParams::RowSum` (see report).
            let (rows, cols) = (size, size);
            if (rows as u64).saturating_mul(cols as u64) > ROW_SUM_MAX_ELEMENTS {
                return None;
            }
            Some(WorkloadParams::RowSum { rows, cols })
        }
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cpu_vector_paths() {
        let axpy = WorkloadParams::VectorAxpy { n: 4, a: 1.5 };
        let inputs = WorkloadInputs {
            x: vec![1.0, 2.0, 3.0, 4.0],
            y: vec![0.5, 0.5, 0.5, 0.5],
        };
        let mut out = vec![0.0f32; 4];
        axpy.run_cpu(&inputs, &mut out).unwrap();
        assert_eq!(out, vec![2.0, 3.5, 5.0, 6.5]);

        let scale = WorkloadParams::VectorScale { n: 4, a: 2.0 };
        let inputs2 = WorkloadInputs {
            x: vec![1.0, -2.0, 3.0, 0.0],
            y: vec![],
        };
        let mut out2 = vec![0.0f32; 4];
        scale.run_cpu(&inputs2, &mut out2).unwrap();
        assert_eq!(out2, vec![2.0, -4.0, 6.0, 0.0]);

        // Wrong output length is an error, not a panic.
        let mut too_short = vec![0.0f32; 2];
        assert!(scale.run_cpu(&inputs2, &mut too_short).is_err());
    }

    #[test]
    fn cpu_matrix_paths() {
        // 2x2 * 2x2, ascending t accumulation.
        let mm = WorkloadParams::Matmul { m: 2, k: 2, n: 2 };
        let inputs = WorkloadInputs {
            x: vec![1.0, 2.0, 3.0, 4.0], // A = [[1,2],[3,4]]
            y: vec![5.0, 6.0, 7.0, 8.0], // B = [[5,6],[7,8]]
        };
        let mut out = vec![0.0f32; 4];
        mm.run_cpu(&inputs, &mut out).unwrap();
        assert_eq!(out, vec![19.0, 22.0, 43.0, 50.0]);

        // 2x3 row-sum, ascending column accumulation.
        let rs = WorkloadParams::RowSum { rows: 2, cols: 3 };
        let inputs2 = WorkloadInputs {
            x: vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0],
            y: vec![],
        };
        let mut out2 = vec![0.0f32; 2];
        rs.run_cpu(&inputs2, &mut out2).unwrap();
        assert_eq!(out2, vec![6.0, 15.0]);
        assert_eq!(rs.output_len(), 2);
    }

    #[test]
    fn make_inputs_is_deterministic() {
        let p = WorkloadParams::VectorAxpy { n: 64, a: 1.5 };
        let a = p.make_inputs();
        let b = p.make_inputs();
        assert_eq!(a.x, b.x);
        assert_eq!(a.y, b.y);
        assert_eq!(a.x.len(), 64);
        assert_eq!(a.y.len(), 64);
        for &v in a.x.iter().chain(a.y.iter()) {
            assert!(v.is_finite());
            assert!(v >= -1.0 && v < 1.0, "value out of range: {v}");
        }
        // Scale and row-sum have an empty y buffer.
        let s = WorkloadParams::VectorScale { n: 8, a: 2.0 };
        assert!(s.make_inputs().y.is_empty());
        let r = WorkloadParams::RowSum { rows: 4, cols: 4 };
        assert!(r.make_inputs().y.is_empty());
        // Different params produce different data (seed depends on `a`).
        let q = WorkloadParams::VectorAxpy { n: 64, a: 2.5 };
        assert_ne!(a.x, q.make_inputs().x);
    }

    #[test]
    fn validate_accepts_and_rejects() {
        let p = WorkloadParams::VectorScale { n: 4, a: 2.0 };
        let reference = vec![1.0f32, 2.0, 3.0, 4.0];

        let good = vec![1.0f32 + 1.0e-4, 2.0, 3.0, 4.0];
        let err = p.validate(&reference, &good, 0.0).unwrap();
        assert!(err <= 1.0e-3);

        let bad = vec![1.0f32, 2.0, 3.0, 100.0];
        assert!(p.validate(&reference, &bad, 0.0).is_err());

        assert!(p.validate(&reference, &[1.0f32, 2.0], 0.0).is_err());

        let nan = vec![1.0f32, f32::NAN, 3.0, 4.0];
        assert!(p.validate(&reference, &nan, 0.0).is_err());

        // A larger explicit tolerance admits a larger error.
        let loose = vec![1.0f32, 2.0, 3.0, 4.5];
        assert!(p.validate(&reference, &loose, 1.0).is_ok());
        assert!(p.validate(&reference, &loose, 0.0).is_err());
    }

    #[test]
    fn digest_is_stable() {
        let p = WorkloadParams::VectorScale { n: 3, a: 2.0 };
        let values = vec![1.0f32, -2.5, 0.0];
        let d1 = p.digest(&values);
        let d2 = p.digest(&values);
        assert_eq!(d1, d2);
        assert_eq!(d1.len(), 16);
        assert!(d1
            .chars()
            .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()));
        assert_ne!(d1, p.digest(&[1.0f32, -2.5, 0.001]));
        // Bit-level: -0.0 and 0.0 hash differently.
        assert_ne!(p.digest(&[0.0f32]), p.digest(&[-0.0f32]));
    }

    #[test]
    fn workload_params_serde_tagged_roundtrip() {
        let cases = vec![
            WorkloadParams::VectorAxpy { n: 8, a: 1.5 },
            WorkloadParams::VectorScale { n: 16, a: 2.0 },
            WorkloadParams::Matmul { m: 4, k: 4, n: 4 },
            WorkloadParams::RowSum { rows: 8, cols: 8 },
        ];
        for p in cases {
            let json = serde_json::to_string(&p).unwrap();
            assert!(json.contains("\"kind\":"), "missing tag: {json}");
            let back: WorkloadParams = serde_json::from_str(&json).unwrap();
            assert_eq!(p, back);
        }
        let axpy =
            serde_json::to_string(&WorkloadParams::VectorAxpy { n: 1, a: 1.5 }).unwrap();
        assert!(axpy.contains("\"kind\":\"vectorAxpy\""), "unexpected: {axpy}");
        let rs =
            serde_json::to_string(&WorkloadParams::RowSum { rows: 1, cols: 1 }).unwrap();
        assert!(rs.contains("\"kind\":\"rowSum\""), "unexpected: {rs}");
    }

    #[test]
    fn default_params_catalogue_and_arbitrary() {
        const K: usize = 1024;
        const M: usize = 1024 * 1024;
        for &size in &[64 * K, 256 * K, M, 4 * M, 16 * M] {
            let axpy = default_params("vector_axpy", size).unwrap();
            assert_eq!(axpy.workload_id(), "vector_axpy");
            assert_eq!(axpy.input_size(), size);
            assert!(matches!(
                axpy,
                WorkloadParams::VectorAxpy { a, .. } if a == 1.5
            ));
            let scale = default_params("vector_scale", size).unwrap();
            assert!(matches!(
                scale,
                WorkloadParams::VectorScale { a, .. } if a == 2.0
            ));
        }
        for &size in &[256usize, 512, 1024] {
            let p = default_params("matmul", size).unwrap();
            assert_eq!(p.workload_id(), "matmul");
            assert_eq!(p.input_size(), size);
            assert_eq!(p.secondary_size(), Some(size));
            assert!(matches!(
                p,
                WorkloadParams::Matmul { m, k, n }
                    if m == size && k == size && n == size
            ));
        }
        let rs = default_params("row_sum", 1024).unwrap();
        assert!(matches!(
            rs,
            WorkloadParams::RowSum { rows: 1024, cols: 1024 }
        ));
        assert_eq!(rs.input_size(), 1024);
        assert_eq!(rs.secondary_size(), Some(1024));

        // Arbitrary smaller positive sizes.
        assert!(matches!(
            default_params("vector_axpy", 1024),
            Some(WorkloadParams::VectorAxpy { n: 1024, .. })
        ));
        assert!(matches!(
            default_params("matmul", 32),
            Some(WorkloadParams::Matmul { m: 32, k: 32, n: 32 })
        ));
        assert!(matches!(
            default_params("row_sum", 128),
            Some(WorkloadParams::RowSum { rows: 128, cols: 128 })
        ));

        // Rejections.
        assert!(default_params("unknown_id", 100).is_none());
        assert!(default_params("vector_axpy", 0).is_none());
        assert!(default_params("matmul", 0).is_none());
        assert!(default_params("vector_axpy", 64 * M + 1).is_none());
        assert!(default_params("matmul", 2049).is_none());
        assert!(default_params("row_sum", 6000).is_none()); // 36M > 32M cap
    }

    #[test]
    fn byte_accounting_is_consistent() {
        let cases = vec![
            WorkloadParams::VectorAxpy { n: 1024, a: 1.5 },
            WorkloadParams::VectorScale { n: 1024, a: 2.0 },
            WorkloadParams::Matmul { m: 32, k: 32, n: 32 },
            WorkloadParams::RowSum { rows: 64, cols: 32 },
        ];
        for p in cases {
            assert_eq!(p.transfer_bytes(), p.device_bytes());
            assert_eq!(p.transfer_bytes(), p.total_elements() * 4);
            assert!(p.transfer_bytes() > 0);
            assert!((p.output_len() as u64) * 4 <= p.device_bytes());
        }
        let axpy = WorkloadParams::VectorAxpy { n: 10, a: 1.0 };
        assert_eq!(axpy.total_elements(), 30); // x + y + out
        assert_eq!(axpy.device_bytes(), 120);
        let scale = WorkloadParams::VectorScale { n: 10, a: 1.0 };
        assert_eq!(scale.total_elements(), 20); // x + out (y empty)
        assert_eq!(scale.device_bytes(), 80);
        assert!(scale.make_inputs().y.is_empty());
    }

    #[test]
    fn cancel_token_is_shared_and_cooperative() {
        let token = CancelToken::new();
        assert!(!token.is_cancelled());
        let clone = token.clone();
        clone.cancel();
        assert!(token.is_cancelled());
        assert!(!CancelToken::default().is_cancelled());
    }

    #[test]
    fn mode_and_backend_as_str_parse_roundtrip() {
        for mode in [
            AccelerationMode::CpuOnly,
            AccelerationMode::GpuAcceleration,
            AccelerationMode::IntelligentAuto,
            AccelerationMode::Manual,
        ] {
            assert_eq!(AccelerationMode::parse(mode.as_str()), Some(mode));
        }
        assert_eq!(AccelerationMode::default(), AccelerationMode::IntelligentAuto);
        assert_eq!(AccelerationMode::parse("bogus"), None);
        for backend in [BackendKind::Cpu, BackendKind::Cuda, BackendKind::Wgpu] {
            assert_eq!(BackendKind::parse(backend.as_str()), Some(backend));
        }
        assert_eq!(BackendKind::parse("cuda"), Some(BackendKind::Cuda));
        assert_eq!(BackendKind::parse("bogus"), None);
    }
}
