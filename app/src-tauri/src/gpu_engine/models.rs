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
