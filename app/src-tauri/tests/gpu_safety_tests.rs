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
fn reservation_accounting_is_idempotent() {
    let (db, _path) = temp_db();
    let manager = ResourceManager::new(monitor_for(&db));
    assert!(manager.reserve(7, 1024));
    assert!(manager.reserve(7, 2048)); // double reserve must not double count
    assert_eq!(manager.running_gpu_tasks(), 1);
    manager.release(7);
    manager.release(7); // double release must not corrupt accounting
    assert_eq!(manager.running_gpu_tasks(), 0);
}

#[test]
fn admission_is_denied_when_no_vram_budget_exists() {
    let (db, _path) = temp_db();
    let manager = ResourceManager::new(monitor_for(&db));
    let config = AccelerationConfig::default();
    let requirement = WorkloadRequirement {
        workload_id: "vector_axpy".to_string(),
        input_size: 1024,
        device_bytes: 12 * 1024,
        transfer_bytes: 12 * 1024,
        timeout: std::time::Duration::from_secs(1),
    };
    // On a host without a discrete GPU the budget is zero: admission must deny.
    let decision = manager.admit(1, &requirement, &config);
    if manager.budget_bytes(&config) == 0 {
        assert!(!decision.allowed);
        assert!(decision.reason.is_some());
        assert_eq!(manager.running_gpu_tasks(), 0);
    }
}

#[test]
fn task_history_round_trips_through_sqlite() {
    let (db, _path) = temp_db();
    let task = sample_task(42);
    let id = persistence::record_task(&db, &task).unwrap();
    assert!(id > 0);
    let recent = persistence::recent_tasks(&db, 10, None).unwrap();
    assert_eq!(recent.len(), 1);
    assert_eq!(recent[0].id, 42);
    assert_eq!(recent[0].warnings, vec!["test note".to_string()]);
    let filtered = persistence::recent_tasks(&db, 10, Some("matmul")).unwrap();
    assert!(filtered.is_empty());
}

#[test]
fn benchmark_history_round_trips_and_clears() {
    let (db, _path) = temp_db();
    let record = sample_benchmark();
    persistence::record_benchmark(&db, &record).unwrap();
    let recent = persistence::recent_benchmarks(&db, 10, None).unwrap();
    assert_eq!(recent.len(), 1);
    assert_eq!(recent[0].verdict, "test verdict");
    assert_eq!(recent[0].gpu.map(|g| g.median_ms), Some(2.0));
    assert_eq!(persistence::clear_benchmarks(&db).unwrap(), 1);
    assert!(persistence::recent_benchmarks(&db, 10, None).unwrap().is_empty());
}

#[test]
fn config_and_workload_settings_round_trip() {
    let (db, _path) = temp_db();
    let mut config = AccelerationConfig::default();
    config.vram_budget_percent = 40.0;
    config.workload_settings = vec![WorkloadSetting {
        workload_id: "matmul".to_string(),
        mode: AccelerationMode::Manual,
        backend: BackendKind::Cpu,
        enabled: true,
        selected_size: 512,
    }];
    persistence::save_config(&db, &config).unwrap();
