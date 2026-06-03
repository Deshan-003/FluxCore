use serde::{Deserialize, Serialize};

/// Memory pressure classification. Deliberately derived from multiple signals
/// (available memory, commit charge, pagefile usage, hard-fault rate and
/// trends) instead of RAM utilization alone.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PressureLevel {
    Normal,
    Elevated,
    High,
    Critical,
}

impl PressureLevel {
    pub fn as_str(&self) -> &'static str {
        match self {
            PressureLevel::Normal => "normal",
            PressureLevel::Elevated => "elevated",
            PressureLevel::High => "high",
            PressureLevel::Critical => "critical",
        }
    }
    pub fn rank(&self) -> u8 {
        match self {
            PressureLevel::Normal => 0,
            PressureLevel::Elevated => 1,
            PressureLevel::High => 2,
            PressureLevel::Critical => 3,
        }
    }
    pub fn from_rank(rank: u8) -> Self {
        match rank {
            0 => PressureLevel::Normal,
            1 => PressureLevel::Elevated,
            2 => PressureLevel::High,
            _ => PressureLevel::Critical,
        }
    }
}

/// Detailed physical + virtual memory accounting for one observation.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoryAnalysis {
    pub collected_at: String,
    pub total_bytes: u64,
    pub used_bytes: u64,
    pub available_bytes: u64,
    pub free_bytes: Option<u64>,
    pub usage_percent: f32,
    pub available_percent: f32,
    /// Windows system cache (file cache). Reclaimable; not "used" memory.
    pub cached_bytes: Option<u64>,
    /// Standby list (reserve + normal + core). Reclaimable cached pages.
    pub standby_bytes: Option<u64>,
    /// Modified page list: pages awaiting write-back. Not available, not in use by apps.
    pub modified_bytes: Option<u64>,
    pub compressed_bytes: Option<u64>,
    pub committed_bytes: Option<u64>,
    pub commit_limit_bytes: Option<u64>,
    pub commit_percent: Option<f32>,
    pub pagefile_used_bytes: Option<u64>,
    pub pagefile_total_bytes: Option<u64>,
    pub pagefile_percent: Option<f32>,
    /// Hard-fault proxy: physical page reads per second.
    pub hard_faults_per_sec: Option<f32>,
    pub page_faults_per_sec: Option<f32>,
    /// Windows' own memory load estimate from MEMORYSTATUSEX.
    pub memory_load_percent: Option<u32>,
    /// Which Windows APIs produced each group of values.
    pub sources: Vec<String>,
    /// Metrics that could not be collected on this system.
    pub limitations: Vec<String>,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PressureSignals {
    pub available_percent: f32,
    pub commit_percent: Option<f32>,
    pub pagefile_percent: Option<f32>,
    pub hard_faults_per_sec: Option<f32>,
    pub available_trend_bytes_per_min: f64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PressureReport {
    pub level: PressureLevel,
    pub level_since: String,
    pub reasons: Vec<String>,
    pub signals: PressureSignals,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PressureEvent {
    pub id: i64,
    pub started_at: String,
    pub ended_at: Option<String>,
    pub level: PressureLevel,
    pub reason: String,
    pub min_available_bytes: u64,
    pub peak_commit_percent: Option<f32>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TrendDirection {
    Rising,
    Falling,
    Stable,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum LeakConfidence {
    Low,
    Medium,
    High,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LeakIndicator {
    pub confidence: LeakConfidence,
    pub growth_bytes_per_min: f64,
    pub reason: String,
}

/// Per-process memory detail collected with documented Windows APIs.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProcessMemoryInfo {
    pub pid: u32,
    pub name: String,
    /// Commit charge attributable to the process (private bytes).
    pub private_bytes: u64,
    pub working_set_bytes: u64,
    /// Private, non-shareable portion of the working set.
    pub private_working_set_bytes: Option<u64>,
    pub peak_working_set_bytes: u64,
    /// Not reported: per-process virtual size requires undocumented query paths.
    pub virtual_bytes: Option<u64>,
    pub cpu_percent: Option<f32>,
    pub share_percent: f32,
    pub trend_bytes_per_min: f64,
    pub trend_direction: TrendDirection,
    pub samples: u32,
    pub observation_seconds: u64,
    pub leak: Option<LeakIndicator>,
    pub started_at: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CompressionInfo {
    pub available: bool,
    pub compressed_bytes: Option<u64>,
    /// Working set of the Windows "Memory Compression" process, when visible.
    pub compression_process_bytes: Option<u64>,
    pub pressure_relationship: String,
    pub cpu_impact_note: String,
    pub limitation: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PagefileConfigEntry {
    pub path: String,
    pub system_managed: bool,
    pub initial_size_mb: Option<u64>,
    pub maximum_size_mb: Option<u64>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VirtualMemoryInfo {
    pub commit_charge_bytes: Option<u64>,
    pub commit_limit_bytes: Option<u64>,
    pub commit_percent: Option<f32>,
    pub available_commit_bytes: Option<u64>,
    pub pagefile_percent_usage: Option<f32>,
    pub pagefile_config: Vec<PagefileConfigEntry>,
    pub notes: Vec<String>,
    pub limitations: Vec<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RiskLevel {
    None,
    Low,
    Medium,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Recommendation {
    pub id: String,
    pub title: String,
    pub reason: String,
    /// Qualitative benefit only; FluxCore never invents byte savings.
    pub expected_benefit: String,
    pub risk: RiskLevel,
    pub category: String,
    pub action_id: Option<String>,
    pub requires_approval: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum OptimizationMode {
    AnalysisOnly,
    SmartAssist,
    Manual,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActionDescriptor {
    pub id: String,
    pub title: String,
    pub description: String,
    pub side_effects: String,
    pub risk: RiskLevel,
    pub requires_confirmation: bool,
    pub changes_system: bool,
    pub available_in_modes: Vec<OptimizationMode>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ActionStatus {
    Success,
    Failed,
    Rejected,
    Cancelled,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActionResult {
    pub action_id: String,
    pub status: ActionStatus,
    pub detail: String,
    pub executed_at: String,
    pub duration_ms: u64,
}
