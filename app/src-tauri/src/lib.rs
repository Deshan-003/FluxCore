pub mod database;
pub mod gpu_engine;
pub mod intelligence;
pub mod memory_engine;
pub mod monitoring;

use memory_engine::models::{
    ActionDescriptor, ActionResult, MemoryHistorySeries, MemoryIntelligenceSnapshot,
    OptimizationMode, PolicyConfig,
};
use memory_engine::MemoryEngine;
use monitoring::{
    HardwareCheckReport, HistoryPoint, MonitorEngine, MonitoringSettings, SystemSnapshot,
};
use std::sync::Arc;
use tauri::{Manager, State};

use gpu_engine::models::{
    AccelerationConfig, AccelerationEnvironment, AccelerationMode, BenchmarkRecord,
    BenchmarkRequest, GpuAccelerationState, GpuTask, ProviderStatus, SubmitTaskRequest,
    SuitabilityAssessment, TaskStatus, WorkloadView,
};

struct AppState {
    engine: Arc<MonitorEngine>,
    memory: Arc<MemoryEngine>,
    gpu: Arc<gpu_engine::GpuEngine>,
    intelligence: Arc<intelligence::IntelligenceEngine>,
}

#[tauri::command]
fn get_snapshot(state: State<'_, AppState>) -> Result<SystemSnapshot, String> {
    state.engine.snapshot()
}

#[tauri::command]
fn get_settings(state: State<'_, AppState>) -> Result<MonitoringSettings, String> {
    state.engine.settings()
}

#[tauri::command]
fn save_settings(settings: MonitoringSettings, state: State<'_, AppState>) -> Result<(), String> {
    state.engine.save_settings(settings)
}

#[tauri::command]
fn get_history(state: State<'_, AppState>) -> Result<Vec<HistoryPoint>, String> {
    state.engine.history()
}

#[tauri::command]
fn check_hardware(state: State<'_, AppState>) -> Result<HardwareCheckReport, String> {
    state.engine.check_hardware()
}

#[tauri::command]
fn get_memory_intelligence(
    state: State<'_, AppState>,
) -> Result<MemoryIntelligenceSnapshot, String> {
    state.memory.intelligence()
}

#[tauri::command]
fn get_memory_history_series(
    limit: Option<usize>,
    state: State<'_, AppState>,
) -> Result<MemoryHistorySeries, String> {
    state
        .memory
        .history_series(limit.unwrap_or(720).clamp(1, 5000))
}

#[tauri::command]
fn get_optimization_mode(state: State<'_, AppState>) -> Result<OptimizationMode, String> {
    Ok(state.memory.mode())
}

#[tauri::command]
fn set_optimization_mode(mode: OptimizationMode, state: State<'_, AppState>) -> Result<(), String> {
    state.memory.set_mode(mode);
    Ok(())
}

#[tauri::command]
fn get_optimization_actions(state: State<'_, AppState>) -> Result<Vec<ActionDescriptor>, String> {
    Ok(state.memory.actions())
}

#[tauri::command]
fn execute_optimization_action(
    action_id: String,
    confirmed: bool,
    state: State<'_, AppState>,
) -> Result<ActionResult, String> {
    state.memory.execute_action(&action_id, confirmed)
}

#[tauri::command]
fn get_memory_policy_config(state: State<'_, AppState>) -> Result<PolicyConfig, String> {
    Ok(state.memory.config())
}

#[tauri::command]
fn update_memory_policy_config(
    config: PolicyConfig,
    state: State<'_, AppState>,
) -> Result<(), String> {
    state.memory.set_config(config)
}

#[tauri::command]
fn set_dashboard_active(active: bool, state: State<'_, AppState>) -> Result<(), String> {
    state.memory.set_dashboard_active(active);
    Ok(())
}

// ---------------------------------------------------------------------------
// Phase 3: GPU acceleration commands. All state comes from GpuEngine; none of
// these commands block on GPU execution (submit returns a queued task).
// ---------------------------------------------------------------------------

#[tauri::command]
fn get_gpu_acceleration_state(state: State<'_, AppState>) -> Result<GpuAccelerationState, String> {
    Ok(state.gpu.state())
}

#[tauri::command]
fn set_acceleration_mode(
    mode: AccelerationMode,
    state: State<'_, AppState>,
) -> Result<GpuAccelerationState, String> {
    state.gpu.set_mode(mode)
}

#[tauri::command]
fn get_gpu_resource_config(state: State<'_, AppState>) -> Result<AccelerationConfig, String> {
    Ok(state.gpu.config())
}

#[tauri::command]
fn update_gpu_resource_config(
    config: AccelerationConfig,
    state: State<'_, AppState>,
) -> Result<AccelerationConfig, String> {
    state.gpu.update_config(config)
}

#[tauri::command]
fn list_gpu_providers(state: State<'_, AppState>) -> Result<Vec<ProviderStatus>, String> {
    Ok(state.gpu.providers())
}

#[tauri::command]
fn probe_gpu_providers(
    force: bool,
    state: State<'_, AppState>,
) -> Result<Vec<ProviderStatus>, String> {
    Ok(state.gpu.probe(force))
}

#[tauri::command]
fn list_supported_workloads(state: State<'_, AppState>) -> Result<Vec<WorkloadView>, String> {
    Ok(state.gpu.workloads())
}

#[tauri::command]
fn get_workload_recommendations(
    size: Option<usize>,
    workload_id: Option<String>,
    state: State<'_, AppState>,
) -> Result<Vec<SuitabilityAssessment>, String> {
    state.gpu.recommendations(size, workload_id)
}

#[tauri::command]
fn submit_workload_task(
    request: SubmitTaskRequest,
    state: State<'_, AppState>,
) -> Result<GpuTask, String> {
    state.gpu.submit(request)
}

#[tauri::command]
fn cancel_task(task_id: u64, state: State<'_, AppState>) -> Result<GpuTask, String> {
    state.gpu.cancel(task_id)
}

#[tauri::command]
fn list_tasks(
    limit: Option<usize>,
    status: Option<TaskStatus>,
    state: State<'_, AppState>,
) -> Result<Vec<GpuTask>, String> {
    Ok(state.gpu.list_tasks(limit, status))
}

#[tauri::command]
fn clear_finished_tasks(state: State<'_, AppState>) -> Result<usize, String> {
    Ok(state.gpu.clear_finished())
}

#[tauri::command]
fn get_task_history(
    limit: Option<usize>,
    workload_id: Option<String>,
    state: State<'_, AppState>,
) -> Result<Vec<GpuTask>, String> {
    Ok(state.gpu.task_history(limit, workload_id))
}

#[tauri::command]
fn run_benchmark(
    request: BenchmarkRequest,
    state: State<'_, AppState>,
) -> Result<BenchmarkRecord, String> {
    state.gpu.run_benchmark(request)
}

#[tauri::command]
fn get_benchmark_history(
    limit: Option<usize>,
    workload_id: Option<String>,
    state: State<'_, AppState>,
) -> Result<Vec<BenchmarkRecord>, String> {
    Ok(state.gpu.benchmark_history(limit, workload_id))
}

#[tauri::command]
fn clear_benchmark_history(state: State<'_, AppState>) -> Result<usize, String> {
    Ok(state.gpu.clear_benchmarks())
}

#[tauri::command]
fn get_gpu_environment(state: State<'_, AppState>) -> Result<AccelerationEnvironment, String> {
    Ok(state.gpu.environment())
}

#[tauri::command]
fn get_resource_intelligence(
    state: State<'_, AppState>,
) -> Result<intelligence::models::ResourceState, String> {
    state.intelligence.state()
}

#[tauri::command]
fn get_resource_history(
    metric: String,
    horizon: i64,
    state: State<'_, AppState>,
) -> Result<intelligence::models::ResourceHistory, String> {
    state.intelligence.history(&metric, horizon)
}

#[tauri::command]
fn configure_resource_intelligence(
    policy: intelligence::models::Policy,
    confirmed: bool,
    state: State<'_, AppState>,
) -> Result<(), String> {
    state.intelligence.configure(policy, confirmed)
}

#[tauri::command]
fn respond_to_resource_recommendation(
    id: String,
    decision: intelligence::Decision,
    confirmed: bool,
    state: State<'_, AppState>,
) -> Result<(), String> {
    state.intelligence.respond(&id, decision, confirmed)
}

#[tauri::command]
fn apply_resource_policy(confirmed: bool, state: State<'_, AppState>) -> Result<(), String> {
    state.intelligence.manual(confirmed)
}

#[tauri::command]
fn revert_resource_policy(id: String, state: State<'_, AppState>) -> Result<(), String> {
    state.intelligence.revert(&id)
}

pub fn run() {
    let database =
        Arc::new(database::Database::open().expect("failed to initialize FluxCore local database"));
    let memory = Arc::new(MemoryEngine::new(Arc::clone(&database)));
    let intelligence = Arc::new(intelligence::IntelligenceEngine::new(
        Arc::clone(&database),
        Arc::clone(&memory),
    ));
    let engine = Arc::new(MonitorEngine::with_intelligence(
        Arc::clone(&database),
        Arc::clone(&memory),
        Some(Arc::clone(&intelligence)),
    ));
    let gpu = Arc::new(gpu_engine::GpuEngine::new(
        Arc::clone(&database),
        Arc::clone(&engine),
    ));
    intelligence.attach_gpu(&gpu);
    #[allow(unused_mut)]
    let mut builder = tauri::Builder::default();
    // Optional WebDriver server for the native UI E2E build only; the shipped
    // build does not enable the `e2e` feature and never registers these plugins.
    #[cfg(feature = "e2e")]
    {
        builder = builder
            .plugin(tauri_plugin_wdio::init())
            .plugin(tauri_plugin_wdio_webdriver::init());
    }
    builder
        .manage(AppState {
            engine,
            memory,
            gpu,
            intelligence,
        })
        .setup(|app| {
            let state = app.state::<AppState>();
            state.engine.set_app_handle(app.handle().clone());
            state.gpu.set_app_handle(app.handle().clone());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_snapshot,
            get_settings,
            save_settings,
            get_history,
            check_hardware,
            get_memory_intelligence,
            get_memory_history_series,
            get_optimization_mode,
            set_optimization_mode,
            get_optimization_actions,
            execute_optimization_action,
            get_memory_policy_config,
            update_memory_policy_config,
            set_dashboard_active,
            get_gpu_acceleration_state,
            set_acceleration_mode,
            get_gpu_resource_config,
            update_gpu_resource_config,
            list_gpu_providers,
            probe_gpu_providers,
            list_supported_workloads,
            get_workload_recommendations,
            submit_workload_task,
            cancel_task,
            list_tasks,
            clear_finished_tasks,
            get_task_history,
            run_benchmark,
            get_benchmark_history,
            clear_benchmark_history,
            get_gpu_environment,
            get_resource_intelligence,
            get_resource_history,
            configure_resource_intelligence,
            respond_to_resource_recommendation,
            apply_resource_policy,
            revert_resource_policy
        ])
        .run(tauri::generate_context!())
        .expect("error while running FluxCore");
}
