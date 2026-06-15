//! Phase 3 provider contract tests: CPU reference correctness, deterministic
//! inputs, validation tolerance, cooperative cancel/timeout, honest support
//! reporting. These run on any host (no GPU required).

use std::time::Duration;

use fluxcore_lib::gpu_engine::error::GpuError;
use fluxcore_lib::gpu_engine::models::{
    CancelToken, ExecutionRequest, WorkloadParams, WorkloadRequirement,
};
use fluxcore_lib::gpu_engine::providers::{CpuProvider, GpuProvider};

fn cpu() -> CpuProvider {
    let provider = CpuProvider::new();
    provider.initialize().expect("CPU provider must initialize");
    provider
}

fn params_set() -> Vec<WorkloadParams> {
    vec![
        WorkloadParams::VectorAxpy { n: 4096, a: 1.5 },
        WorkloadParams::VectorScale { n: 4096, a: -2.0 },
        WorkloadParams::Matmul { m: 64, k: 32, n: 48 },
        WorkloadParams::RowSum { rows: 32, cols: 16 },
    ]
}

fn request(params: WorkloadParams, cancel: CancelToken, timeout: Duration) -> ExecutionRequest {
    ExecutionRequest {
        request_id: 1,
        workload_id: params.workload_id().to_string(),
        device_id: String::new(),
        params,
        timeout,
        cancel,
        validate: true,
    }
}

#[test]
fn cpu_provider_advertises_all_four_workloads() {
    let provider = cpu();
    assert!(provider.is_available());
    let ids: Vec<String> = provider
        .supported_workloads()
        .into_iter()
        .map(|support| support.workload_id)
        .collect();
    for expected in ["vector_axpy", "vector_scale", "matmul", "row_sum"] {
        assert!(ids.contains(&expected.to_string()), "missing {expected}");
