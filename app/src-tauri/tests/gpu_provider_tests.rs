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
    }
    assert!(provider.capabilities().compute_ready);
}

#[test]
fn cpu_output_matches_the_reference_digest_for_every_workload() {
    let provider = cpu();
    for params in params_set() {
        let inputs = params.make_inputs();
        let mut reference = vec![0.0f32; params.output_len()];
        params
            .run_cpu(&inputs, &mut reference)
            .expect("reference must run");
        let outcome = provider
            .execute(&request(params, CancelToken::new(), Duration::from_secs(10)))
            .expect("CPU execution must succeed");
        assert_eq!(outcome.backend, "cpu");
        assert_eq!(outcome.output_digest, params.digest(&reference));
        assert!(outcome.timing.total_ns > 0);
        // The CPU path is the reference: it must not claim GPU validation.
        assert!(!outcome.validated);
        assert_eq!(outcome.max_abs_error, None);
    }
}

#[test]
fn inputs_are_deterministic() {
    for params in params_set() {
        assert_eq!(params.make_inputs().x, params.make_inputs().x);
        assert_eq!(params.make_inputs().y, params.make_inputs().y);
    }
}

#[test]
fn validation_accepts_identical_and_rejects_perturbed_output() {
    for params in params_set() {
        let inputs = params.make_inputs();
        let mut reference = vec![0.0f32; params.output_len()];
        params.run_cpu(&inputs, &mut reference).unwrap();
        assert!(params.validate(&reference, &reference, 1e-3).is_ok());
        let mut wrong = reference.clone();
        wrong[0] += 1.0;
        assert!(params.validate(&reference, &wrong, 1e-3).is_err());
    }
}

#[test]
fn cancelled_token_stops_execution() {
    let provider = cpu();
    let cancel = CancelToken::new();
    cancel.cancel();
    let error = provider
        .execute(&request(
            WorkloadParams::VectorAxpy { n: 1_048_576, a: 1.0 },
            cancel,
            Duration::from_secs(10),
        ))
        .expect_err("cancelled execution must fail");
    assert!(matches!(error, GpuError::Cancelled { .. }));
}

#[test]
fn expired_timeout_stops_execution() {
    let provider = cpu();
    let error = provider
        .execute(&request(
            WorkloadParams::VectorAxpy { n: 4_194_304, a: 1.0 },
            CancelToken::new(),
            Duration::ZERO,
        ))
        .expect_err("expired timeout must fail");
    assert!(matches!(error, GpuError::Timeout { .. }));
}

#[test]
fn unknown_workload_is_not_admitted() {
    let provider = cpu();
    let requirement = WorkloadRequirement {
        workload_id: "not_a_workload".to_string(),
        input_size: 16,
        device_bytes: 64,
        transfer_bytes: 64,
        timeout: Duration::from_secs(1),
    };
    assert!(provider.can_execute(&requirement).is_err());
}

#[test]
fn oversized_workload_is_rejected_by_caps() {
    let provider = cpu();
    let requirement = WorkloadRequirement {
        workload_id: "matmul".to_string(),
        input_size: 4096,
        device_bytes: 1 << 30,
        transfer_bytes: 1 << 30,
        timeout: Duration::from_secs(1),
    };
    assert!(provider.can_execute(&requirement).is_err());
}