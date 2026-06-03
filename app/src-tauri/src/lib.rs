pub mod database;
pub mod memory_engine;
pub mod monitoring;

use memory_engine::models::{
    ActionDescriptor, ActionResult, MemoryHistorySeries, MemoryIntelligenceSnapshot,
    OptimizationMode, PolicyConfig,
};
use memory_engine::MemoryEngine;
use monitoring::{HardwareCheckReport, HistoryPoint, MonitorEngine, MonitoringSettings, SystemSnapshot};
use std::sync::Arc;
use tauri::{Manager, State};

struct AppState {
    engine: Arc<MonitorEngine>,
    memory: Arc<MemoryEngine>,
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
fn get_memory_intelligence(state: State<'_, AppState>) -> Result<MemoryIntelligenceSnapshot, String> {
    state.memory.intelligence()
}

#[tauri::command]
fn get_memory_history_series(limit: Option<usize>, state: State<'_, AppState>) -> Result<MemoryHistorySeries, String> {
    state.memory.history_series(limit.unwrap_or(720).clamp(1, 5000))
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
fn update_memory_policy_config(config: PolicyConfig, state: State<'_, AppState>) -> Result<(), String> {
    state.memory.set_config(config)
}

#[tauri::command]
fn set_dashboard_active(active: bool, state: State<'_, AppState>) -> Result<(), String> {
    state.memory.set_dashboard_active(active);
    Ok(())
}

pub fn run() {
    let database =
        Arc::new(database::Database::open().expect("failed to initialize FluxCore local database"));
    let memory = Arc::new(MemoryEngine::new(Arc::clone(&database)));
    let engine = Arc::new(MonitorEngine::new(Arc::clone(&database), Arc::clone(&memory)));
    tauri::Builder::default()
        .manage(AppState { engine, memory })
        .setup(|app| {
            let state = app.state::<AppState>();
            state.engine.set_app_handle(app.handle().clone());
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
            set_dashboard_active
        ])
        .run(tauri::generate_context!())
        .expect("error while running FluxCore");
}
