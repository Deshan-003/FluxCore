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
        None => {
            missing.push(format!(
                "no measured benchmark exists at input size {}",
                params.input_size()
            ));
            (model_cpu, model_gpu, model_transfer, EstimateSource::Model)
        }
    };
    if degenerate && bench.is_none() {
        missing.push("no cost-model coefficients are available".to_string());
    }

    let gpu_total_ms = gpu_ms + transfer_ms + LAUNCH_OVERHEAD_MS;
    Estimates {
        cpu_ms,
        gpu_ms,
        transfer_ms,
        gpu_total_ms,
        source,
        missing,
    }
}

/// Safety gates read from the single passed-in snapshot. These NEVER route a
/// workload TO the GPU — they only block GPU execution and appear in blockers.
fn derived_safety_blockers(
    resources: &ResourceBudgetStatus,
    config: &AccelerationConfig,
) -> Vec<String> {
    let mut out = Vec::new();
    if let Some(util) = resources.gpu_utilization_percent {
        if util > config.max_gpu_utilization_percent {
            out.push(format!(
                "GPU utilisation {util:.0}% exceeds the limit {:.0}%",
                config.max_gpu_utilization_percent
            ));
        }
    }
    if let Some(temp) = resources.temperature_c {
        if temp > config.max_temperature_c {
            out.push(format!(
                "GPU temperature {temp:.0} C exceeds the limit {:.0} C",
                config.max_temperature_c
            ));
        }
    }
    // Power is recorded as a note only, never a blocker (§7.6).
    if let Some(ram) = resources.system_ram_available_percent {
        if ram < config.min_system_ram_available_percent {
            out.push(format!(
                "system RAM available {ram:.0}% is below the minimum {:.0}%",
                config.min_system_ram_available_percent
            ));
        }
    }
    out
}

fn merge_blockers(mut base: Vec<String>, extra: Vec<String>) -> Vec<String> {
    for item in extra {
        if !base.iter().any(|existing| existing == &item) {
            base.push(item);
        }
    }
    base
}

fn backend_of(provider: &ProviderStatus) -> BackendKind {
    match provider.id.as_str() {
        "cuda" => BackendKind::Cuda,
        "wgpu" => BackendKind::Wgpu,
        "cpu" => BackendKind::Cpu,
        _ => match provider.kind {
            ProviderKind::Cuda => BackendKind::Cuda,
            ProviderKind::Wgpu => BackendKind::Wgpu,
            _ => BackendKind::Cpu,
        },
    }
}

/// A provider with an empty support list is treated as supporting every
/// registered workload (§7.1 declares a GPU path for all four). An explicit
/// list is authoritative.
fn supports(provider: &ProviderStatus, workload_id: &str) -> bool {
    if provider.supported_workloads.is_empty() {
        return true;
    }
    provider
        .supported_workloads
        .iter()
        .any(|s| s.workload_id == workload_id)
}

fn ordered_by_priority<'a>(
    providers: &[&'a ProviderStatus],
    config: &AccelerationConfig,
) -> Vec<&'a ProviderStatus> {
    let mut out = providers.to_vec();
    out.sort_by_key(|p| {
        config
            .provider_priority
            .iter()
            .position(|id| id == &p.id)
            .unwrap_or(usize::MAX)
    });
    out
}

fn gpu_unavailable_reason(providers: &[ProviderStatus]) -> String {
    let reasons: Vec<String> = providers
        .iter()
        .filter(|p| p.kind != ProviderKind::Cpu)
        .map(|p| {
            let detail = p
                .last_error
                .as_deref()
                .filter(|s| !s.is_empty())
                .or_else(|| {
                    let d = p.detail.as_str();
                    if d.is_empty() { None } else { Some(d) }
                })
                .unwrap_or("not compute-ready");
            format!("{}: {}", p.id, detail)
        })
        .collect();
    if reasons.is_empty() {
        "no GPU compute provider was discovered".to_string()
    } else {
        reasons.join("; ")
    }
}

#[allow(clippy::too_many_arguments)]
fn make_assessment(
    params: &WorkloadParams,
    classification: Suitability,
    reason: String,
    estimates: Option<&Estimates>,
    estimate_source: EstimateSource,
    inputs: &ClassifyInputs<'_>,
    selected_backend: BackendKind,
    available_vram_bytes: Option<u64>,
    blockers: Vec<String>,
) -> SuitabilityAssessment {
    SuitabilityAssessment {
        workload_id: params.workload_id().to_string(),
        input_size: params.input_size(),
        secondary_size: params.secondary_size(),
        classification,
        reason,
        estimated_cpu_ms: estimates.map(|e| e.cpu_ms),
        estimated_gpu_ms: estimates.map(|e| e.gpu_ms),
        estimated_gpu_total_ms: estimates.map(|e| e.gpu_total_ms),
        transfer_overhead_ms: estimates.map(|e| e.transfer_ms),
        transfer_bytes: params.transfer_bytes(),
        estimated_vram_bytes: params.device_bytes(),
        required_vram_bytes: params.device_bytes(),
        available_vram_bytes,
        gpu_budget_bytes: inputs.resources.budget_bytes,
        estimate_source,
        selected_backend,
        gpu_enabled: inputs.gpu_enabled,
        blockers,
    }
}

/// Deterministic, side-effect-free. Decision order is EXACTLY §7.2, with two
/// explicit conservative guards required by the Subagent-3 brief:
///  * VRAM checks (steps 5/6) are skipped when VRAM is unknown, and an unknown
///    VRAM is reported in the conservative reason.
///  * A `GpuRecommended` verdict requires a MEASURED benchmark at this exact
///    input size. Placeholder model coefficients (EstimateSource::Model/Mixed)
///    are conservative estimates and are NEVER used alone to claim a speedup.
pub fn classify(params: &WorkloadParams, inputs: &ClassifyInputs<'_>) -> SuitabilityAssessment {
    let config = inputs.config;
    let resources = inputs.resources;
    let wid = params.workload_id();

    // Always-truthful base fields.
    let free_vram = resources.free_vram_bytes;
    let device_free_vram: Option<u64> = inputs
        .providers
        .iter()
        .filter(|p| p.kind != ProviderKind::Cpu)
        .flat_map(|p| p.devices.iter())
        .filter_map(|d| d.dedicated_free_bytes)
        .max();
    let vram_known = resources.total_vram_bytes.is_some()
        || free_vram.is_some()
        || device_free_vram.is_some();
    let available_vram_bytes = Some(free_vram.unwrap_or(resources.remaining_budget_bytes));

    let safety_blockers = merge_blockers(
        resources.gates.clone(),
        derived_safety_blockers(resources, config),
    );

    let ready_gpu: Vec<&ProviderStatus> = inputs
        .providers
        .iter()
        .filter(|p| p.available && p.compute_ready && p.kind != ProviderKind::Cpu)
        .collect();

    let bench = most_recent_benchmark(inputs.benchmarks, wid, params.input_size());

    // ---- Step 1: workload id not in registry --------------------------------
    let def = match find(wid) {
        Some(def) => def,
        None => {
            let mut blockers = safety_blockers.clone();
            blockers.push(format!("no implementation is registered for '{wid}'"));
            return make_assessment(
                params,
                Suitability::UnsupportedWorkload,
                format!("No implementation is registered for workload '{wid}'"),
                None,
                EstimateSource::Model,
                inputs,
                BackendKind::Cpu,
                available_vram_bytes,
                blockers,
            );
        }
    };

    let estimates = build_estimates(params, def, bench);
    let estimate_source = estimates.source;
    let make = |classification, reason, backend, blockers: Vec<String>| {
        make_assessment(
            params,
            classification,
            reason,
            Some(&estimates),
            estimate_source,
            inputs,
            backend,
            available_vram_bytes,
            blockers,
        )
    };

    // ---- Step 2: CpuOnly mode -----------------------------------------------
    if config.mode == AccelerationMode::CpuOnly {
        return make(
            Suitability::CpuRecommended,
            "GPU acceleration is disabled in CPU Only mode; this workload runs on the CPU."
                .to_string(),
            BackendKind::Cpu,
            safety_blockers,
        );
    }

    // ---- Step 3: no compute-ready GPU provider ------------------------------
    if ready_gpu.is_empty() {
        let reason = gpu_unavailable_reason(inputs.providers);
        let mut blockers = safety_blockers.clone();
        blockers.insert(
            0,
            "no compute-ready GPU provider is available".to_string(),
        );
        return make(
            Suitability::AccelerationUnavailable,
            format!("No GPU compute backend is usable: {reason}"),
            BackendKind::Cpu,
            blockers,
        );
    }

    // ---- Step 4: no provider implements this workload -----------------------
    let implementing: Vec<&ProviderStatus> = ready_gpu
        .iter()
        .copied()
        .filter(|p| supports(p, wid))
        .collect();
    if implementing.is_empty() {
        let names = ordered_by_priority(&ready_gpu, config)
            .iter()
            .map(|p| p.id.as_str())
            .collect::<Vec<_>>()
            .join(", ");
        return make(
            Suitability::UnsupportedWorkload,
            format!("No compute-ready GPU provider implements '{wid}' (checked: {names})"),
            BackendKind::Cpu,
            safety_blockers,
        );
    }
    let ordered = ordered_by_priority(&implementing, config);
    let selected_gpu_backend = backend_of(ordered[0]);

    // ---- Step 5: required VRAM exceeds remaining budget ---------------------
    let required = params.device_bytes();
    if vram_known && required > resources.remaining_budget_bytes {
        let mut blockers = safety_blockers.clone();
        blockers.push(format!(
            "workload needs {} but the budget leaves {}",
            fmt_mib(required),
            fmt_mib(resources.remaining_budget_bytes)
        ));
        return make(
            Suitability::InsufficientGpuResources,
            format!(
                "{wid} needs {} of VRAM but the configured GPU budget leaves only {}",
                fmt_mib(required),
                fmt_mib(resources.remaining_budget_bytes)
            ),
            BackendKind::Cpu,
            blockers,
        );
    }

    // ---- Step 6: provider-reported free VRAM below required -----------------
    if let Some(reported_free) = device_free_vram.or(free_vram) {
        if reported_free < required {
            let mut blockers = safety_blockers.clone();
            blockers.push(format!(
                "GPU reports only {} free VRAM",
                fmt_mib(reported_free)
            ));
            return make(
                Suitability::InsufficientGpuResources,
                format!(
                    "GPU reports only {} free VRAM; {wid} needs {}",
                    fmt_mib(reported_free),
                    fmt_mib(required)
                ),
                BackendKind::Cpu,
                blockers,
            );
        }
    }

    // ---- Step 7: safety gates block (NOT InsufficientGpuResources) ----------
    // Utilisation / temperature / RAM are safety gates ONLY. They never route
    // a workload to the GPU; they go into blockers and force a CPU decision.
    if !safety_blockers.is_empty() {
        return make(
            Suitability::GpuAvailableNotBeneficial,
            format!(
                "The GPU is usable but a safety gate blocks GPU execution: {}",
                safety_blockers.join("; ")
            ),
            BackendKind::Cpu,
            safety_blockers.clone(),
        );
    }

    let user_pinned_cpu = config.mode == AccelerationMode::Manual
        && config
            .workload_settings
            .iter()
            .any(|s| s.workload_id == wid && s.backend == BackendKind::Cpu);

    // ---- Step 8: model says the GPU would not pay off -----------------------
    let beneficial = estimates.gpu_total_ms * def.beneficial_speedup < estimates.cpu_ms;
    if !beneficial {
        let classification = if user_pinned_cpu {
            Suitability::CpuRecommended
        } else {
            Suitability::GpuAvailableNotBeneficial
        };
        let pin_note = if user_pinned_cpu {
            " (CPU is pinned by the user)"
        } else {
            ""
        };
        return make(
            classification,
            format!(
                "GPU is available but would not pay off at this size: estimated CPU {:.3} ms vs \
                 GPU total {:.3} ms (compute {:.3} ms + transfer {:.3} ms + {:.2} ms launch); \
                 a >{:.2}x end-to-end speedup is required{pin_note}. Transfer + launch overhead \
                 dominates at small sizes.",
                estimates.cpu_ms,
                estimates.gpu_total_ms,
                estimates.gpu_ms,
                estimates.transfer_ms,
                LAUNCH_OVERHEAD_MS,
                def.beneficial_speedup,
            ),
            BackendKind::Cpu,
            safety_blockers,
        );
    }

    // ---- Conservative guard (brief §classify) -------------------------------
    // A GpuRecommended verdict is only issued when a MEASURED benchmark at
    // this exact input size backs it and VRAM could actually be verified.
    // Otherwise we stay on the CPU and say exactly what was missing.
    let measured_basis = estimate_source == EstimateSource::Measured;
    if !measured_basis || !vram_known || !inputs.gpu_enabled || !estimates.missing.is_empty() {
        let mut missing = estimates.missing.clone();
        if !measured_basis {
            missing.push("no measured CPU+GPU benchmark backs a speedup claim".to_string());
        }
        if !vram_known {
            missing.push("GPU VRAM is unknown, so the memory budget cannot be verified".to_string());
        }
        if !inputs.gpu_enabled {
            missing.push("GPU acceleration is not enabled".to_string());
        }
        let what_missing = if missing.is_empty() {
            "the decision lacks sufficient measured evidence".to_string()
        } else {
            missing.join("; ")
        };
        return make(
            Suitability::CpuRecommended,
            format!(
                "The GPU may be faster (estimated CPU {:.3} ms vs GPU total {:.3} ms) but the \
                 estimate is not sufficient to recommend it: {what_missing}. Running on the CPU \
                 until a measured benchmark at this size is available.",
                estimates.cpu_ms, estimates.gpu_total_ms,
            ),
            BackendKind::Cpu,
            safety_blockers,
        );
    }

    // ---- Step 9: otherwise GPU is recommended -------------------------------
    let ratio = estimates.cpu_ms / estimates.gpu_total_ms.max(1.0e-9);
    make(
        Suitability::GpuRecommended,
        format!(
            "A measured benchmark at this exact input size shows the GPU is worthwhile: \
             CPU {:.3} ms vs GPU total {:.3} ms ({:.2}x, threshold {:.2}x). \
             {} selected by provider priority.",
            estimates.cpu_ms,
            estimates.gpu_total_ms,
            ratio,
            def.beneficial_speedup,
            selected_gpu_backend.as_str(),
        ),
        selected_gpu_backend,
        safety_blockers,
    )
}

// ---------------------------------------------------------------------------
// Unit tests — pure `classify` with hand-built inputs (no MonitorEngine).
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gpu_engine::models::{
        GpuDeviceInfo, ProviderCapabilities, SafetyLevel, WorkloadSupport,
    };

    fn base_resources() -> ResourceBudgetStatus {
        ResourceBudgetStatus {
            total_vram_bytes: Some(4 * 1024 * 1024 * 1024),
            free_vram_bytes: Some(3 * 1024 * 1024 * 1024),
            budget_bytes: 1024 * 1024 * 1024,
            reserved_bytes: 0,
            allocated_bytes: 0,
            remaining_budget_bytes: 1024 * 1024 * 1024,
            running_gpu_tasks: 0,
            queued_gpu_tasks: 0,
            max_concurrent_gpu_tasks: 1,
            gpu_utilization_percent: Some(20.0),
            temperature_c: Some(50.0),
            power_watts: Some(30.0),
            safety_level: SafetyLevel::Ok,
            gates: Vec::new(),
            system_ram_available_percent: Some(60.0),
        }
    }

    fn gpu_provider(id: &str, workloads: &[&str]) -> ProviderStatus {
        ProviderStatus {
            id: id.to_string(),
            label: id.to_string(),
            kind: match id {
                "cuda" => ProviderKind::Cuda,
                _ => ProviderKind::Wgpu,
            },
            available: true,
            compute_ready: true,
            detail: "ready".to_string(),
            devices: vec![GpuDeviceInfo {
                device_id: format!("{id}-0"),
                dedicated_total_bytes: Some(4 * 1024 * 1024 * 1024),
                dedicated_free_bytes: Some(3 * 1024 * 1024 * 1024),
                ..Default::default()
            }],
            capabilities: ProviderCapabilities {
                compute_ready: true,
                ..Default::default()
            },
            supported_workloads: workloads
                .iter()
                .map(|w| WorkloadSupport {
                    workload_id: (*w).to_string(),
                    max_elements: u64::MAX,
                    notes: Vec::new(),
                })
                .collect(),
            selected_device: format!("{id}-0"),
            last_error: None,
        }
    }

    fn summary(size: usize, cpu: f64, gpu: Option<f64>, transfer: Option<f64>) -> BenchmarkSummary {
        BenchmarkSummary {
            id: 7,
            created_at: "2026-01-01T00:00:00Z".to_string(),
            workload_id: "vector_axpy".to_string(),
            input_size: size,
            backend: BackendKind::Cuda,
            cpu_median_ms: cpu,
            gpu_median_ms: gpu,
            transfer_ms: transfer,
            speedup: None,
            verdict: "measured".to_string(),
        }
    }

    fn inputs<'a>(
        config: &'a AccelerationConfig,
        providers: &'a [ProviderStatus],
        resources: &'a ResourceBudgetStatus,
        benchmarks: &'a [BenchmarkSummary],
        gpu_enabled: bool,
    ) -> ClassifyInputs<'a> {
        ClassifyInputs {
            config,
            providers,
            resources,
            benchmarks,
            gpu_enabled,
        }
    }

    fn healthy_setup() -> (AccelerationConfig, Vec<ProviderStatus>, ResourceBudgetStatus) {
        (
            AccelerationConfig::default(),
            vec![gpu_provider("cuda", &["vector_axpy", "vector_scale", "matmul", "row_sum"])],
            base_resources(),
        )
    }

    #[test]
    fn registry_has_exactly_four_workloads() {
        let ids: Vec<&str> = registry().iter().map(|d| d.id).collect();
        assert_eq!(ids, vec!["vector_axpy", "vector_scale", "matmul", "row_sum"]);
        for def in registry() {
            assert_eq!(def.beneficial_speedup, 1.15);
            assert!(!def.sizes.is_empty());
            assert!(def.sizes.contains(&def.default_size));
            assert!(def.cpu_model.scale_ns_per_unit > 0.0);
            assert!(def.gpu_model.scale_ns_per_unit > 0.0);
        }
        assert!(find("vector_axpy").is_some());
        assert!(find("not_a_workload").is_none());
        assert!(default_params("not_a_workload", 1024).is_none());
    }

    #[test]
    fn validate_params_enforces_caps() {
        assert!(validate_params(&WorkloadParams::VectorAxpy { n: 1024, a: 1.5 }).is_ok());
        assert!(validate_params(&WorkloadParams::Matmul { m: 32, k: 32, n: 32 }).is_ok());
        assert!(validate_params(&WorkloadParams::RowSum { rows: 1024, cols: 1024 }).is_ok());

        assert!(validate_params(&WorkloadParams::VectorAxpy { n: 0, a: 1.0 }).is_err());
        assert!(validate_params(&WorkloadParams::VectorScale {
            n: VECTOR_MAX + 1,
            a: 1.0
        })
        .is_err());
        assert!(validate_params(&WorkloadParams::VectorAxpy {
            n: 1024,
            a: f32::NAN
