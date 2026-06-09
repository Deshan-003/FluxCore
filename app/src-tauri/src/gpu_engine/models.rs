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
