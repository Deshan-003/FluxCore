//! FluxCore Phase 3 benchmark harness — real CPU-vs-GPU measurements only.
//!
//! Works without a Tauri app and without a database. Initializes every compute
//! provider, records the real environment, then benchmarks each supported
//! workload with warm-up + measured runs. When no GPU path runs, the report
//! says so and shows CPU-only numbers; a GPU time is never invented.
//!
//! Usage: cargo run --release --example gpu_benchmark [measured_runs]

use std::time::Duration;

use fluxcore_lib::gpu_engine::models::{
    default_params, AccelerationConfig, BenchmarkRequest, CancelToken,
};
use fluxcore_lib::gpu_engine::{benchmark, capability, providers};

fn main() {
    let measured: u32 = std::env::args()
        .nth(1)
        .and_then(|value| value.parse().ok())
        .unwrap_or(3)
        .clamp(1, 50);

    let providers = providers::initialize_all();
    let environment = capability::environment(&providers);

    println!("== FluxCore Phase 3 GPU benchmark ==");
    println!(
        "os={} cpu={} logical={} ram={} MiB",
        environment.os_name,
        environment.cpu_name,
        environment.logical_processors,
        environment.system_ram_bytes / (1024 * 1024)
    );
    println!(
        "gpu={} vram={:?} MiB driver={:?}",
        environment.gpu_name,
        environment.gpu_vram_bytes.map(|b| b / (1024 * 1024)),
        environment.driver_version
    );
    println!(
        "cuda_toolkit={} cuda_path={} wgpu={} backend={:?} vulkan={} dx12={}",
        environment.cuda_toolkit_present,
        environment.cuda_compute_path,
        environment.wgpu_version,
        environment.wgpu_backend,
        environment.vulkan_present,
        environment.dx12_present
    );
    for status in capability::detect_providers(&providers).providers {
        println!(
            "provider {} available={} compute_ready={} detail={}",
            status.id, status.available, status.compute_ready, status.detail
        );
    }

    let context = benchmark::BenchmarkContext {
        providers: providers.clone(),
        environment,
        config: AccelerationConfig::default(),
        timeout: Duration::from_millis(10_000),
        cancel: CancelToken::new(),
        preferred_backend: None,
    };

    let catalogue: [(&str, usize); 4] = [
        ("vector_axpy", 1_048_576),
        ("vector_scale", 1_048_576),
        ("matmul", 512),
        ("row_sum", 1024),
    ];
    for (workload_id, size) in catalogue {
        let Some(params) = default_params(workload_id, size) else {
            println!("{workload_id}: no default params for size {size}");
            continue;
        };
        let request = BenchmarkRequest {
            workload_id: workload_id.to_string(),
            params,
            warmup_runs: 1,
            measured_runs: measured,
            validate: true,
        };
        match benchmark::run_benchmark(&request, &context) {
            Ok(record) => {
                println!("-- {workload_id} (size {size}) --");
                println!(
                    "cpu median {:.3} ms (min {:.3} / max {:.3}, {} runs)",
                    record.cpu.median_ms, record.cpu.min_ms, record.cpu.max_ms, record.cpu.runs
                );
                match &record.gpu {
                    Some(gpu) => println!(
                        "gpu median {:.3} ms (min {:.3} / max {:.3}, {} runs), transfer {:?} ms, vram {:?} MiB",
                        gpu.median_ms,
                        gpu.min_ms,
                        gpu.max_ms,
                        gpu.runs,
                        record.transfer_ms,
                        record.vram_used_bytes.map(|b| b / (1024 * 1024))
                    ),
                    None => println!("gpu: did not run"),
                }
                println!(
                    "speedup {:?} validated={} max_abs_error={:?}",
                    record.speedup, record.validated, record.max_abs_error
                );
                println!("verdict: {}", record.verdict);
            }
            Err(error) => println!("-- {workload_id} -- benchmark failed: {error}"),
        }
    }

    for provider in &providers {
        provider.shutdown();
    }
}