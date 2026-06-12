//! Phase 3 — workload registry, validation and the CPU/GPU decision engine.
//!
//! Owned by Subagent 3. Exact public surface: `PHASE3_CONTRACT_FREEZE.md` §8.
//! Behaviour: `PHASE3_ARCHITECTURE.md` §7.1 (catalogue), §7.2 (decision order),
//! §7.3 (cost model), §7.4 (live inputs).
//!
//! `classify` is deterministic and side-effect-free: every live input is passed
//! in through `ClassifyInputs` (already assembled from ONE `MonitorEngine`
//! snapshot by `ResourceManager::status`). It never polls, never spawns a
//! thread and never reads GPU utilisation as a routing signal — utilisation,
//! temperature and system RAM are SAFETY GATES only (`blockers`).

use crate::gpu_engine::models::{
    AccelerationConfig, AccelerationMode, BackendKind, BenchmarkSummary, CostModel,
    EstimateSource, ProviderKind, ProviderStatus, ResourceBudgetStatus, Suitability,
    SuitabilityAssessment, WorkloadDef, WorkloadParams,
};

pub use crate::gpu_engine::models::default_params;

// ---------------------------------------------------------------------------
// §7.3 — cost-model constants.
// ---------------------------------------------------------------------------

/// PCIe/host-device bandwidth used for the transfer estimate: ~10 bytes/ns
/// (10 GB/s). `transfer_ms = transfer_bytes / BYTES_PER_NS / 1e6`.
const BYTES_PER_NS: f64 = 10.0;
const NS_PER_MS: f64 = 1_000_000.0;
/// Fixed kernel-launch / dispatch overhead added to every GPU estimate.
const LAUNCH_OVERHEAD_MS: f64 = 0.05;

/// Coefficients below are CONSERVATIVE, DOCUMENTED PLACEHOLDERS. They are not
/// measurements: they exist only to give the UI a first estimate until a real
/// benchmark at the same input size is persisted (§7.3). Every assessment
/// built from them is labelled `EstimateSource::Model`. A GPU recommendation is
/// NEVER issued from placeholders alone — see `classify`'s conservative guard.
fn model_ms(model: &CostModel, work_units: f64) -> f64 {
    (model.intercept_ns + model.scale_ns_per_unit * work_units) / NS_PER_MS
}

fn model_transfer_ms(bytes: u64) -> f64 {
    (bytes as f64 / BYTES_PER_NS) / NS_PER_MS
}

fn fmt_mib(bytes: u64) -> String {
    format!("{:.1} MiB", bytes as f64 / (1024.0 * 1024.0))
}

// ---------------------------------------------------------------------------
// §7.1 — fixed workload registry.
// ---------------------------------------------------------------------------

static VECTOR_SIZES: &[usize] = &[
    64 * 1024,
    256 * 1024,
    1024 * 1024,
    4 * 1024 * 1024,
    16 * 1024 * 1024,
];
static MATMUL_SIZES: &[usize] = &[256, 512, 1024];
static ROW_SUM_PRIMARY: &[usize] = &[1024, 4096];
static ROW_SUM_SECONDARY: &[usize] = &[1024, 4096];
static NO_SECONDARY: &[usize] = &[];

static WORKLOADS: [WorkloadDef; 4] = [
    WorkloadDef {
        id: "vector_axpy",
        title: "Vector AXPY",
        description: "out = a*x + y (BLAS axpy), three f32 buffers.",
        category: "vector",
        element_type: "f32",
        sizes: VECTOR_SIZES,
        default_size: 1024 * 1024,
        secondary_sizes: NO_SECONDARY,
        memory_formula: "n * 4 * 3 bytes (x, y, out)",
        // Conservative placeholders, NOT measured. Units: ns / work unit.
        cpu_model: CostModel { intercept_ns: 0.0, scale_ns_per_unit: 2.0 },
        gpu_model: CostModel { intercept_ns: 2_000.0, scale_ns_per_unit: 0.08 },
        beneficial_speedup: 1.15,
        requires_gpu_path: false,
    },
    WorkloadDef {
        id: "vector_scale",
        title: "Vector scale",
        description: "out = a*x, two f32 buffers.",
        category: "vector",
        element_type: "f32",
        sizes: VECTOR_SIZES,
        default_size: 1024 * 1024,
        secondary_sizes: NO_SECONDARY,
        memory_formula: "n * 4 * 2 bytes (x, out)",
        cpu_model: CostModel { intercept_ns: 0.0, scale_ns_per_unit: 1.5 },
        gpu_model: CostModel { intercept_ns: 2_000.0, scale_ns_per_unit: 0.06 },
        beneficial_speedup: 1.15,
        requires_gpu_path: false,
    },
    WorkloadDef {
        id: "matmul",
        title: "Matrix multiply (naive)",
        description: "out = A(m,k) * B(k,n), naive O(m*k*n).",
        category: "matrix",
        element_type: "f32",
        sizes: MATMUL_SIZES,
        default_size: 512,
        secondary_sizes: MATMUL_SIZES,
        memory_formula: "(m*k + k*n + m*n) * 4 bytes",
        cpu_model: CostModel { intercept_ns: 0.0, scale_ns_per_unit: 1.0 },
        gpu_model: CostModel { intercept_ns: 5_000.0, scale_ns_per_unit: 0.015 },
        beneficial_speedup: 1.15,
        requires_gpu_path: false,
    },
    WorkloadDef {
        id: "row_sum",
        title: "Row-wise reduction",
        description: "out[i] = sum of row i of A(rows, cols).",
        category: "reduction",
        element_type: "f32",
        sizes: ROW_SUM_PRIMARY,
        default_size: 1024,
        secondary_sizes: ROW_SUM_SECONDARY,
        memory_formula: "(rows*cols + rows) * 4 bytes",
        cpu_model: CostModel { intercept_ns: 0.0, scale_ns_per_unit: 1.2 },
        gpu_model: CostModel { intercept_ns: 2_000.0, scale_ns_per_unit: 0.05 },
        beneficial_speedup: 1.15,
        requires_gpu_path: false,
    },
];

pub fn registry() -> &'static [WorkloadDef] {
    &WORKLOADS[..]
}

pub fn find(id: &str) -> Option<&'static WorkloadDef> {
    WORKLOADS.iter().find(|def| def.id == id)
}

// ---------------------------------------------------------------------------
// §7.1 — parameter caps.
// ---------------------------------------------------------------------------

const VECTOR_MAX: usize = 64 * 1024 * 1024;
const MATMUL_MAX: usize = 2048;
const ROW_SUM_MAX_ELEMENTS: u64 = 32 * 1024 * 1024;
