//! Phase 3 safety + persistence tests: config validation ranges, idempotent
//! reservation accounting, admission denial without a VRAM budget, SQLite
//! round-trips and additive-migration guarantees (no Phase 1/2 data loss).

use std::sync::Arc;

use fluxcore_lib::database::Database;
use fluxcore_lib::gpu_engine::models::{
    AccelerationConfig, AccelerationEnvironment, BackendKind, BenchmarkRecord, GpuTask,
    ProviderCapabilities, ProviderKind, ProviderStatus, TaskPriority, TaskStatus, TimingStats,
    WorkloadParams, WorkloadRequirement, WorkloadSetting, AccelerationMode,
};
use fluxcore_lib::gpu_engine::persistence;
use fluxcore_lib::gpu_engine::resources::ResourceManager;
use fluxcore_lib::memory_engine::MemoryEngine;
use fluxcore_lib::monitoring::MonitorEngine;

fn temp_db() -> (Arc<Database>, std::path::PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("safety-test.sqlite3");
    std::mem::forget(dir);
    (Arc::new(Database::open_at_path(path.clone()).unwrap()), path)
}

fn monitor_for(db: &Arc<Database>) -> Arc<MonitorEngine> {
    let memory = Arc::new(MemoryEngine::new(Arc::clone(db)));
    Arc::new(MonitorEngine::new(Arc::clone(db), memory))
}

fn sample_env() -> AccelerationEnvironment {
    AccelerationEnvironment {
        os_name: "test".to_string(),
        os_version: "test".to_string(),
        cpu_name: "test cpu".to_string(),
        logical_processors: 4,
        system_ram_bytes: 8 * 1024 * 1024 * 1024,
        gpu_name: "none".to_string(),
        gpu_vram_bytes: None,
        driver_version: None,
        cuda_toolkit_present: false,
        cuda_runtime_api_present: false,
        cuda_compute_path: "none".to_string(),
        wgpu_version: "30".to_string(),
        wgpu_backend: None,
        vulkan_present: false,
        dx12_present: false,
        recorded_at: chrono::Utc::now().to_rfc3339(),
    }
}

fn sample_task(id: u64) -> GpuTask {
    GpuTask {
        id,
        client_ref: None,
        workload_id: "vector_axpy".to_string(),
        title: "Vector AXPY".to_string(),
        params: WorkloadParams::VectorAxpy { n: 1024, a: 1.0 },
        status: TaskStatus::Succeeded,
        priority: TaskPriority::Normal,
        requested_backend: Some(BackendKind::Cuda),
        selected_backend: Some(BackendKind::Cuda),
        assessment: None,
        queued_at: chrono::Utc::now().to_rfc3339(),
        started_at: Some(chrono::Utc::now().to_rfc3339()),
        finished_at: Some(chrono::Utc::now().to_rfc3339()),
        duration_ms: Some(5),
        queue_ms: Some(1),
        progress_percent: Some(100.0),
        cpu_time_ms: None,
        gpu_time_ms: Some(4.0),
        transfer_time_ms: Some(1.0),
        total_time_ms: Some(5.0),
        speedup: None,
        vram_used_bytes: Some(12 * 1024),
        max_abs_error: Some(0.0),
        validated: true,
        fell_back: false,
        error: None,
        warnings: vec!["test note".to_string()],
    }
}

fn sample_benchmark() -> BenchmarkRecord {
    let stats = TimingStats {
        runs: 3,
        min_ms: 1.0,
        median_ms: 2.0,
        mean_ms: 2.0,
        max_ms: 3.0,
        stddev_ms: 0.5,
    };
    BenchmarkRecord {
        id: 0,
        created_at: chrono::Utc::now().to_rfc3339(),
        workload_id: "vector_axpy".to_string(),
        params: WorkloadParams::VectorAxpy { n: 1024, a: 1.0 },
        backend: BackendKind::Cuda,
        provider_id: "cuda".to_string(),
        device_id: "0".to_string(),
        cpu: stats,
        gpu: Some(stats),
        transfer_ms: Some(0.5),
        speedup: Some(2.0),
        vram_used_bytes: Some(12 * 1024),
        validated: true,
        max_abs_error: Some(0.0),
        verdict: "test verdict".to_string(),
        environment: sample_env(),
    }
}

#[test]
fn config_validation_rejects_out_of_range_values() {
    let base = AccelerationConfig::default();
    assert!(ResourceManager::validate_config(&base).is_ok());
    let cases: Vec<fn(&mut AccelerationConfig)> = vec![
        |c| c.vram_budget_percent = 0.5,
        |c| c.vram_budget_percent = 95.0,
        |c| c.vram_budget_cap_mb = 8192,
        |c| c.max_gpu_utilization_percent = 5.0,
        |c| c.max_temperature_c = 200.0,
        |c| c.max_concurrent_gpu_tasks = 4,
        |c| c.max_concurrent_cpu_tasks = 8,
        |c| c.max_queue_length = 0,
        |c| c.task_timeout_ms = 10,
        |c| c.min_system_ram_available_percent = 80.0,
        |c| c.retention_days = 400,
    ];
    for mutate in cases {
        let mut config = base.clone();
        mutate(&mut config);
        assert!(
            ResourceManager::validate_config(&config).is_err(),
            "config should have been rejected"
        );
    }
}

#[test]
fn default_budget_uses_percent_semantics() {
    let config = AccelerationConfig::default();
    assert!((config.vram_budget_percent - 50.0).abs() < f32::EPSILON);
    assert!(ResourceManager::validate_config(&config).is_ok());
}

#[test]
