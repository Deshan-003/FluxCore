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

pub fn validate_params(params: &WorkloadParams) -> Result<(), String> {
    let id = params.workload_id();
    match *params {
        WorkloadParams::VectorAxpy { n, a } | WorkloadParams::VectorScale { n, a } => {
            if n == 0 {
                return Err(format!("{id}: n must be at least 1 (got 0)"));
            }
            if n > VECTOR_MAX {
                return Err(format!(
                    "{id}: n={n} exceeds the cap of {VECTOR_MAX} elements (256 MiB per f32 buffer)"
                ));
            }
            if !a.is_finite() {
                return Err(format!("{id}: scalar a must be finite (got {a})"));
            }
        }
        WorkloadParams::Matmul { m, k, n } => {
            for (name, value) in [("m", m), ("k", k), ("n", n)] {
                if value == 0 {
                    return Err(format!("{id}: {name} must be at least 1 (got 0)"));
                }
                if value > MATMUL_MAX {
                    return Err(format!(
                        "{id}: {name}={value} exceeds the cap of {MATMUL_MAX}"
                    ));
                }
            }
            if m.checked_mul(k).is_none()
                || k.checked_mul(n).is_none()
                || m.checked_mul(n).is_none()
            {
                return Err(format!("{id}: matrix dimensions overflow usize"));
            }
        }
        WorkloadParams::RowSum { rows, cols } => {
            if rows == 0 || cols == 0 {
                return Err(format!(
                    "{id}: rows and cols must be at least 1 (got rows={rows}, cols={cols})"
                ));
            }
            let elements = (rows as u64).saturating_mul(cols as u64);
            if elements > ROW_SUM_MAX_ELEMENTS {
                return Err(format!(
                    "{id}: rows*cols = {elements} exceeds the cap of {ROW_SUM_MAX_ELEMENTS} elements"
                ));
            }
            rows.checked_mul(cols)
                .ok_or_else(|| format!("{id}: rows*cols overflows usize"))?;
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// §7.2 inputs + estimates.
// ---------------------------------------------------------------------------

pub struct ClassifyInputs<'a> {
    pub config: &'a AccelerationConfig,
    pub providers: &'a [ProviderStatus],
    pub resources: &'a ResourceBudgetStatus,
    pub benchmarks: &'a [BenchmarkSummary],
    pub gpu_enabled: bool,
}

struct Estimates {
    cpu_ms: f64,
    gpu_ms: f64,
    transfer_ms: f64,
    gpu_total_ms: f64,
    source: EstimateSource,
    /// Human-readable list of information that was missing (for conservative
    /// "CpuRecommended" reasons).
    missing: Vec<String>,
}

fn most_recent_benchmark<'a>(
    benchmarks: &'a [BenchmarkSummary],
    workload_id: &str,
    input_size: usize,
) -> Option<&'a BenchmarkSummary> {
    benchmarks
        .iter()
        .filter(|b| b.workload_id == workload_id && b.input_size == input_size)
        .max_by_key(|b| b.id)
}

fn build_estimates(
    params: &WorkloadParams,
    def: &WorkloadDef,
    bench: Option<&BenchmarkSummary>,
) -> Estimates {
    let work = params.work_units();
    let model_cpu = model_ms(&def.cpu_model, work);
    let model_gpu = model_ms(&def.gpu_model, work);
    let model_transfer = model_transfer_ms(params.transfer_bytes());
    let degenerate = def.cpu_model.intercept_ns <= 0.0
        && def.cpu_model.scale_ns_per_unit <= 0.0
        && def.gpu_model.intercept_ns <= 0.0
        && def.gpu_model.scale_ns_per_unit <= 0.0;

    let mut missing: Vec<String> = Vec::new();
    let (cpu_ms, gpu_ms, transfer_ms, source) = match bench {
        Some(b) => {
            let cpu_measured = b.cpu_median_ms.is_finite() && b.cpu_median_ms > 0.0;
            let cpu = if cpu_measured {
                b.cpu_median_ms
            } else {
                missing.push("the benchmark has no valid CPU median".to_string());
                model_cpu
            };
            let transfer = match b.transfer_ms {
                Some(t) if t.is_finite() && t >= 0.0 => t,
                _ => {
                    missing.push("the benchmark has no measured transfer time".to_string());
                    model_transfer
                }
            };
            match b.gpu_median_ms {
                Some(g) if g.is_finite() && g > 0.0 => {
                    // `gpu_median_ms` is interpreted as the GPU compute median;
                    // `transfer_ms` and the launch overhead are added separately
                    // (§7.3, §11's speedup = cpu / (gpu + transfer)). If a
                    // producer instead reports an end-to-end GPU total, this
                    // over-estimates gpu_total_ms — the CONSERVATIVE direction.
                    (
                        cpu,
                        g,
                        transfer,
                        if cpu_measured {
                            EstimateSource::Measured
                        } else {
                            EstimateSource::Mixed
                        },
                    )
                }
                _ => {
                    missing.push("the benchmark has no measured GPU median".to_string());
                    (cpu, model_gpu, transfer, EstimateSource::Mixed)
                }
            }
        }
