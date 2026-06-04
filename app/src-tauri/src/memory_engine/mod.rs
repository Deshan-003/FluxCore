//! Intelligent Memory Engine orchestration.
//!
//! `MemoryEngine` is driven by the Phase 1 monitor loop: every collected
//! snapshot is fed to `tick`, which runs analysis, pressure detection,
//! process tracking, prediction, policies and recommendations, persists at
//! the configured intervals and keeps the latest intelligence snapshot for
//! the dashboard.

pub mod analyzer;
pub mod compression;
pub mod history;
pub mod models;
pub mod optimizer;
pub mod policies;
pub mod prediction;
pub mod pressure_detector;
pub mod process_analyzer;
pub mod recommendations;
pub mod safety;
pub mod stats;
pub mod virtual_memory;

use crate::database::Database;
use crate::monitoring::SystemSnapshot;
use chrono::Utc;
use models::*;
use std::collections::{HashSet, VecDeque};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant};

struct EngineState {
    mode: OptimizationMode,
    tracker: process_analyzer::ProcessTracker,
    detector: pressure_detector::PressureDetector,
    window: prediction::SampleWindow,
    history_points: VecDeque<MemoryHistoryPoint>,
    open_event: Option<PressureEvent>,
    audit: history::AuditRing,
    persisted_recommendations: HashSet<String>,
    stored_recommendations: Vec<StoredRecommendation>,
    recommendations: Vec<Recommendation>,
    prediction: Option<Prediction>,
    snapshot: Option<MemoryIntelligenceSnapshot>,
    current: Vec<(u32, String, process_analyzer::ProcessReading)>,
    last_level: PressureLevel,
    last_process_scan: Instant,
    last_persist: Instant,
    last_process_persist: Instant,
    last_prediction: Instant,
    background_paused_until: Option<Instant>,
    force_scan: bool,
    last_activity: Instant,
}

pub struct MemoryEngine {
    db: Arc<Database>,
    state: Mutex<EngineState>,
    counters: Mutex<analyzer::WindowsMemoryCounters>,
    config: RwLock<PolicyConfig>,
    pagefile_config: RwLock<Option<Vec<PagefileConfigEntry>>>,
    dashboard_active: AtomicBool,
}

impl MemoryEngine {
    pub fn new(db: Arc<Database>) -> Self {
        let config = db.load_memory_policy_config().ok().flatten().unwrap_or_default();
        let now = Instant::now();
        Self {
            db,
            state: Mutex::new(EngineState {
                mode: OptimizationMode::AnalysisOnly,
                tracker: process_analyzer::ProcessTracker::new(),
                detector: pressure_detector::PressureDetector::new(),
                window: prediction::SampleWindow::default(),
                history_points: VecDeque::new(),
                open_event: None,
                audit: history::AuditRing::default(),
                persisted_recommendations: HashSet::new(),
                stored_recommendations: Vec::new(),
                recommendations: Vec::new(),
                prediction: None,
                snapshot: None,
                current: Vec::new(),
                last_level: PressureLevel::Normal,
                // Scan on the very first tick so the ranking is never empty.
                last_process_scan: now - Duration::from_secs(3600),
                last_persist: now,
                last_process_persist: now,
                last_prediction: now - Duration::from_secs(3600),
                background_paused_until: None,
                force_scan: false,
                last_activity: now,
            }),
            counters: Mutex::new(analyzer::WindowsMemoryCounters::new()),
            config: RwLock::new(config),
            pagefile_config: RwLock::new(None),
            dashboard_active: AtomicBool::new(true),
        }
    }

    pub fn mode(&self) -> OptimizationMode {
        self.state.lock().map(|state| state.mode).unwrap_or(OptimizationMode::AnalysisOnly)
    }

    pub fn set_mode(&self, mode: OptimizationMode) {
        if let Ok(mut state) = self.state.lock() {
            state.mode = mode;
        }
    }

    pub fn config(&self) -> PolicyConfig {
        self.config.read().map(|config| config.clone()).unwrap_or_default()
    }

    pub fn set_config(&self, config: PolicyConfig) -> Result<(), String> {
        let thresholds = &config.thresholds;
        if !(1.0..=90.0).contains(&thresholds.critical_available_percent)
            || thresholds.critical_available_percent >= thresholds.high_available_percent
            || thresholds.high_available_percent >= thresholds.elevated_available_percent
            || thresholds.elevated_commit_percent >= thresholds.high_commit_percent
            || thresholds.high_commit_percent >= thresholds.critical_commit_percent
            || !(50.0..=100.0).contains(&thresholds.critical_commit_percent)
        {
            return Err("thresholds must be ordered: critical < high < elevated (availability) and elevated < high < critical (commit)".into());
        }
        if !(1..=600).contains(&config.upgrade_min_seconds) || !(1..=3600).contains(&config.downgrade_min_seconds) {
            return Err("hysteresis durations must be between 1 and 3600 seconds".into());
        }
        if !(1000..=60000).contains(&config.process_scan_interval_ms) {
            return Err("process scan interval must be between 1000 and 60000 milliseconds".into());
        }
        if !(1000..=300000).contains(&config.persist_interval_ms) {
            return Err("persistence interval must be between 1000 and 300000 milliseconds".into());
        }
        if !(1..=90).contains(&config.retention_days) {
            return Err("retention must be between 1 and 90 days".into());
        }
        self.db.save_memory_policy_config(&config)?;
        *self.config.write().map_err(|_| "policy lock poisoned".to_string())? = config;
        Ok(())
    }

    pub fn set_dashboard_active(&self, active: bool) {
        self.dashboard_active.store(active, Ordering::Release);
        if active {
            if let Ok(mut state) = self.state.lock() {
                state.last_activity = Instant::now();
            }
        }
    }

    pub fn intelligence(&self) -> Result<MemoryIntelligenceSnapshot, String> {
        self.state
            .lock()
            .map_err(|_| "memory engine lock poisoned".to_string())?
            .snapshot
            .clone()
            .ok_or_else(|| "memory engine is initializing; retry shortly".to_string())
    }

    pub fn actions(&self) -> Vec<ActionDescriptor> {
        optimizer::descriptors()
    }

    pub fn history_series(&self, limit: usize) -> Result<MemoryHistorySeries, String> {
        Ok(MemoryHistorySeries {
            samples: self.db.memory_samples(limit)?,
            pressure_events: self.db.pressure_events(50)?,
            process_history: self.db.process_history(600)?,
            recommendations: self.db.stored_recommendations(50)?,
            actions: self.db.recent_actions(50)?,
        })
    }

    /// One monitoring tick: analysis → pressure → processes → prediction →
    /// policies → recommendations → persistence → dashboard snapshot.
    pub fn tick(&self, snapshot: &SystemSnapshot) {
        let now = Instant::now();
        let ts = Utc::now().to_rfc3339();
        let epoch = Utc::now().timestamp() as f64;
        let config = self.config();
        let Ok(mut state) = self.state.lock() else { return };
        let Ok(mut counters) = self.counters.lock() else { return };

        let reading = counters.read();
        let status = analyzer::memory_status();
        let analysis = analyzer::analyze(snapshot, &reading, &status, &ts);

        state.window.record(SystemSample {
            at_secs: epoch,
            available_bytes: analysis.available_bytes,
            committed_bytes: analysis.committed_bytes,
            used_bytes: analysis.used_bytes,
        });
        let trend = state.window.available_forecast().slope_bytes_per_min;

        let pressure = state.detector.update(
            &analysis,
            trend,
            &config.thresholds,
            config.upgrade_min_seconds,
            config.downgrade_min_seconds,
            now,
            &ts,
        );
        self.track_pressure_event(&mut state, &pressure, &analysis, &ts);

        let idle = !self.dashboard_active.load(Ordering::Acquire)
            || now.duration_since(state.last_activity) > Duration::from_secs(config.idle_after_seconds);
        let paused = state.background_paused_until.map(|until| now < until).unwrap_or(false);

        let (active_policies, detail_boost) =
            policies::evaluate(&pressure, &analysis, &state.current_processes_view(), idle);

        let scan_interval = if paused {
            Duration::from_secs(u64::MAX / 2)
        } else if idle {
            Duration::from_millis(config.idle_refresh_interval_ms.max(config.process_scan_interval_ms * 2))
        } else if detail_boost {
            Duration::from_millis(config.process_scan_interval_ms / 2)
        } else {
            Duration::from_millis(config.process_scan_interval_ms)
        };
        let scan_due = state.force_scan || now.duration_since(state.last_process_scan) >= scan_interval;
        if scan_due {
            self.scan_processes(&mut state, snapshot, epoch);
            state.last_process_scan = now;
            state.force_scan = false;
        }

        let cpu: std::collections::HashMap<u32, f32> = snapshot
            .processes
            .iter()
            .filter_map(|process| process.cpu_percent.map(|cpu| (process.pid, cpu)))
            .collect();
        let started: std::collections::HashMap<u32, String> = snapshot
            .processes
            .iter()
            .filter_map(|process| process.started_at.clone().map(|started| (process.pid, started)))
            .collect();
        let processes = process_analyzer::analyze(
            &state.current,
            &state.tracker,
            analysis.total_bytes,
            &cpu,
            &started,
        );

        let compression_info = compression::analyze(&analysis, &processes);
        let pagefile_config = {
            let cached = self.pagefile_config.read().map(|cached| cached.clone()).unwrap_or(None);
            match cached {
                Some(config) => config,
                None => {
                    let config = virtual_memory::pagefile_config();
                    if let Ok(mut slot) = self.pagefile_config.write() {
                        *slot = Some(config.clone());
                    }
                    config
                }
            }
        };
        let virtual_memory_info = virtual_memory::analyze(&analysis, pagefile_config);

        if now.duration_since(state.last_prediction) >= Duration::from_secs(15) || state.prediction.is_none() {
            let prediction = prediction::predict(
                &state.window,
                &processes,
                analysis.total_bytes,
                config.thresholds.high_available_percent,
                config.prediction_window_minutes,
                &ts,
            );
            state.prediction = Some(prediction);
            state.last_prediction = now;
        }

        let recommendations = recommendations::generate(
            &pressure,
            &analysis,
            &virtual_memory_info,
            &processes,
            &compression_info,
        );
        self.persist_new_recommendations(&mut state, &recommendations, &ts);
        state.recommendations = recommendations.clone();

        let point = history::history_point(&analysis, pressure.level);
        state.history_points.push_back(point.clone());
        if state.history_points.len() > 720 {
            state.history_points.pop_front();
        }

        if now.duration_since(state.last_persist) >= Duration::from_millis(config.persist_interval_ms) {
            if let Err(error) = self.db.record_memory_snapshot(&point) {
                eprintln!("FluxCore memory history persistence error: {error}");
            }
            if let Err(error) = self.db.prune_memory_data(config.retention_days) {
                eprintln!("FluxCore memory retention prune error: {error}");
            }
            state.last_persist = now;
        }
        if now.duration_since(state.last_process_persist)
            >= Duration::from_millis(config.process_persist_interval_ms)
        {
            let points = history::process_points(&processes, &ts, 20);
            if let Err(error) = self.db.record_process_history(&points) {
                eprintln!("FluxCore process history persistence error: {error}");
            }
            if let Some(prediction) = &state.prediction {
                if let Err(error) = self.db.record_prediction(prediction) {
                    eprintln!("FluxCore prediction persistence error: {error}");
                }
            }
            state.last_process_persist = now;
        }

        let mut limitations = analysis.limitations.clone();
        limitations.extend(virtual_memory_info.limitations.clone());
        limitations.extend(compression_info.limitation.clone());

        let prediction = state.prediction.clone().unwrap_or_else(|| Prediction {
            generated_at: ts.clone(),
            observation_minutes: 0.0,
            confidence: 0.0,
            horizon_minutes: config.prediction_window_minutes,
            available_forecast: TrendForecast {
                slope_bytes_per_min: 0.0,
                current_bytes: Some(analysis.available_bytes),
                projected_bytes: None,
                r2: 0.0,
                samples: 0,
            },
            commit_forecast: TrendForecast {
                slope_bytes_per_min: 0.0,
                current_bytes: analysis.committed_bytes,
                projected_bytes: None,
                r2: 0.0,
                samples: 0,
            },
            time_to_high_pressure_minutes: None,
            growing_processes: Vec::new(),
            summary: "Not enough observations yet for a forecast.".to_string(),
            disclaimer: "Forecast based on recent trends only; actual behavior depends on workload changes.".to_string(),
        });

        state.snapshot = Some(MemoryIntelligenceSnapshot {
            collected_at: ts,
            analysis,
            pressure,
            compression: compression_info,
            virtual_memory: virtual_memory_info,
            processes,
            recommendations,
            prediction,
            policies: PolicyStatus {
                mode: state.mode,
                active: active_policies,
                config,
                dashboard_active: !idle,
            },
            recent_actions: state.audit.recent(25),
            limitations,
        });
    }

    fn track_pressure_event(
        &self,
        state: &mut EngineState,
        pressure: &PressureReport,
        analysis: &MemoryAnalysis,
        ts: &str,
    ) {
        if pressure.level == state.last_level {
            if let Some(open) = state.open_event.as_mut() {
                open.min_available_bytes = open.min_available_bytes.min(analysis.available_bytes);
                open.peak_commit_percent = match (open.peak_commit_percent, analysis.commit_percent) {
                    (Some(previous), Some(current)) => Some(previous.max(current)),
                    (Some(previous), None) => Some(previous),
                    (None, current) => current,
                };
            }
            return;
        }
        if let Some(mut open) = state.open_event.take() {
            open.ended_at = Some(ts.to_string());
            if let Err(error) = self.db.close_pressure_event(&open) {
                eprintln!("FluxCore pressure event close error: {error}");
            }
        }
        if pressure.level != PressureLevel::Normal {
            let event = PressureEvent {
                id: 0,
                started_at: ts.to_string(),
                ended_at: None,
                level: pressure.level,
                reason: pressure.reasons.join(" "),
                min_available_bytes: analysis.available_bytes,
                peak_commit_percent: analysis.commit_percent,
            };
            match self.db.record_pressure_event(&event) {
                Ok(id) => {
                    let mut stored = event;
                    stored.id = id;
                    state.open_event = Some(stored);
                }
                Err(error) => eprintln!("FluxCore pressure event record error: {error}"),
            }
        }
        state.last_level = pressure.level;
    }

    fn scan_processes(&self, state: &mut EngineState, snapshot: &SystemSnapshot, epoch: f64) {
        let mut current: Vec<(u32, String, process_analyzer::ProcessReading)> = Vec::new();
        let mut seen: HashSet<u32> = HashSet::new();
        for process in &snapshot.processes {
            seen.insert(process.pid);
            let reading = match process_analyzer::read_process(process.pid) {
                Some(reading) => reading,
                None => continue,
            };
            state.tracker.record(
                process.pid,
                ProcessSample {
                    at_secs: epoch,
                    private_bytes: reading.private_bytes,
                    working_set_bytes: reading.working_set_bytes,
                },
            );
            current.push((process.pid, process.name.clone(), reading));
        }
        for pid in state.tracker.pids() {
            if !seen.contains(&pid) {
                state.tracker.forget(pid);
            }
        }
        state.current = current;
    }

    fn persist_new_recommendations(
        &self,
        state: &mut EngineState,
        recommendations: &[Recommendation],
        ts: &str,
    ) {
        for recommendation in recommendations {
            if !state.persisted_recommendations.insert(recommendation.id.clone()) {
                continue;
            }
            let mut stored = history::stored_recommendation(recommendation, ts);
            match self.db.record_recommendation(&stored) {
                Ok(id) => stored.id = id,
                Err(error) => {
                    eprintln!("FluxCore recommendation persistence error: {error}");
                    continue;
                }
            }
            state.stored_recommendations.push(stored);
            if state.stored_recommendations.len() > 50 {
                let overflow = state.stored_recommendations.len() - 50;
                state.stored_recommendations.drain(..overflow);
            }
        }
    }

    /// Executes one registry action after safety validation. Every outcome
    /// (success, failure or rejection) is audited and persisted.
    pub fn execute_action(&self, action_id: &str, confirmed: bool) -> Result<ActionResult, String> {
        let started = Instant::now();
        let ts = Utc::now().to_rfc3339();
        // Destructive operations are rejected by name even if they are not
        // part of the action registry at all.
        if safety::is_forbidden(action_id) {
            return Ok(self.audit(
                ActionResult {
                    action_id: action_id.to_string(),
                    status: ActionStatus::Rejected,
                    detail: format!(
                        "'{action_id}' is a destructive operation and is not supported by FluxCore."
                    ),
                    executed_at: ts,
                    duration_ms: 0,
                },
                confirmed,
            ));
        }
        let Some(descriptor) = optimizer::descriptor(action_id) else {
            return Ok(self.audit(ActionResult {
                action_id: action_id.to_string(),
                status: ActionStatus::Rejected,
                detail: "Unknown action id; nothing was executed.".to_string(),
                executed_at: ts,
                duration_ms: 0,
            }, confirmed));
        };
        let mode = self.mode();
        if let Err(reason) = safety::validate(mode, &descriptor, confirmed) {
            return Ok(self.audit(ActionResult {
                action_id: action_id.to_string(),
                status: ActionStatus::Rejected,
                detail: reason,
                executed_at: ts,
                duration_ms: started.elapsed().as_millis() as u64,
            }, confirmed));
        }

        let outcome = self.run_action(action_id);
        let result = match outcome {
            Ok(detail) => ActionResult {
                action_id: action_id.to_string(),
                status: ActionStatus::Success,
                detail,
                executed_at: ts.clone(),
                duration_ms: started.elapsed().as_millis() as u64,
            },
            Err(error) => ActionResult {
                action_id: action_id.to_string(),
                status: ActionStatus::Failed,
                detail: error,
                executed_at: ts.clone(),
                duration_ms: started.elapsed().as_millis() as u64,
            },
        };
        Ok(self.audit(result, confirmed))
    }

    fn run_action(&self, action_id: &str) -> Result<String, String> {
        let now = Instant::now();
        match action_id {
            recommendations::ACTION_CLEAR_CACHES => {
                let config = self.config();
                let pruned = self.db.prune_memory_data(config.retention_days)?;
                let Ok(mut state) = self.state.lock() else {
                    return Err("memory engine lock poisoned".into());
                };
                let before = state.window.samples().len() + state.history_points.len();
                state.history_points.retain(|point| point.timestamp >= cutoff_iso(config.retention_days));
                while state.window.samples().len() > 512 {
                    state.window.drop_oldest();
                }
                Ok(format!(
                    "Cleared FluxCore caches: {} in-memory samples trimmed to the retention window and {pruned} expired database rows pruned.",
                    before.saturating_sub(state.history_points.len())
                ))
            }
            recommendations::ACTION_RELEASE_RESOURCES => {
                let Ok(mut state) = self.state.lock() else {
                    return Err("memory engine lock poisoned".into());
                };
                state.window = prediction::SampleWindow::new(300.0, 512);
                state.tracker = process_analyzer::ProcessTracker::new();
                state.stored_recommendations.truncate(20);
                state.audit.trim_to(50);
                Ok("Released FluxCore internal buffers; trend windows will refill within a few minutes.".to_string())
            }
            recommendations::ACTION_PAUSE_BACKGROUND => {
                let Ok(mut state) = self.state.lock() else {
                    return Err("memory engine lock poisoned".into());
                };
                state.background_paused_until = Some(now + Duration::from_secs(300));
                Ok("FluxCore background scanning and persistence paused for 5 minutes.".to_string())
            }
            recommendations::ACTION_REFRESH_DATA => {
                let Ok(mut state) = self.state.lock() else {
                    return Err("memory engine lock poisoned".into());
                };
                state.force_scan = true;
                Ok("A fresh process scan is scheduled for the next monitoring tick.".to_string())
            }
            recommendations::ACTION_GUIDANCE_CLOSE_APPS => {
                let Ok(state) = self.state.lock() else {
                    return Err("memory engine lock poisoned".into());
                };
                let top: Vec<String> = state
                    .current_processes_view()
                    .iter()
                    .take(5)
                    .map(|process| format!("{} (PID {}, {} private)", process.name, process.pid, bytes_label(process.private_bytes)))
                    .collect();
                Ok(format!(
                    "Top private-memory consumers right now: {}. Review them in Task Manager and close only what you do not need; FluxCore never terminates processes.",
                    if top.is_empty() { "unavailable until the first scan completes".to_string() } else { top.join("; ") }
                ))
            }
            recommendations::ACTION_GUIDANCE_STARTUP => {
                let entries = optimizer::startup_entries();
                Ok(format!(
                    "Startup entries registered in the Run registry keys: {}. Disable unneeded ones via Windows Settings → Apps → Startup; FluxCore does not modify them.",
                    if entries.is_empty() { "none found".to_string() } else { entries.join(", ") }
                ))
            }
            recommendations::ACTION_GUIDANCE_PAGEFILE => {
                let config = self.config();
                let Ok(state) = self.state.lock() else {
                    return Err("memory engine lock poisoned".into());
                };
                let vm = state.snapshot.as_ref().map(|snapshot| snapshot.virtual_memory.clone());
                let layout: Vec<String> = vm
                    .map(|vm| vm.pagefile_config.iter().map(|entry| format!("{} ({})", entry.path, if entry.system_managed { "system managed".to_string() } else { format!("{}-{} MB", entry.initial_size_mb.unwrap_or(0), entry.maximum_size_mb.unwrap_or(0)) })).collect())
                    .unwrap_or_default();
                Ok(format!(
                    "Current pagefile layout: {}. To change it: System Properties → Advanced → Performance Settings → Advanced → Virtual memory → Change. Consequences: a too-small pagefile lowers the commit limit and can cause allocation failures; a fixed size on a slow volume can increase paging latency. Retention is {} days; FluxCore never changes these settings.",
                    if layout.is_empty() { "unavailable".to_string() } else { layout.join(", ") },
                    config.retention_days
                ))
            }
            _ => Err(format!("'{action_id}' is not an executable action.")),
        }
    }

    fn audit(&self, result: ActionResult, confirmed: bool) -> ActionResult {
        let mode = self.mode();
        let entry = AuditEntry {
            id: 0,
            executed_at: result.executed_at.clone(),
            mode,
            action_id: result.action_id.clone(),
            confirmed,
            status: result.status,
            detail: result.detail.clone(),
        };
        let id = self.db.record_action(&entry).unwrap_or(0);
        if let Ok(mut state) = self.state.lock() {
            let mut stored = entry;
            stored.id = id;
            state.audit.push(stored);
        }
        result
    }
}

fn cutoff_iso(retention_days: u32) -> String {
    (Utc::now() - chrono::Duration::days(i64::from(retention_days))).to_rfc3339()
}

fn bytes_label(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut value = bytes as f64;
    let mut index = 0;
    while value >= 1024.0 && index < UNITS.len() - 1 {
        value /= 1024.0;
        index += 1;
    }
    format!("{value:.1} {}", UNITS[index])
}

impl EngineState {
    /// Latest analyzed view of the process set, newest scan first.
    fn current_processes_view(&self) -> Vec<ProcessMemoryInfo> {
        let mut view: Vec<ProcessMemoryInfo> = self
            .current
            .iter()
            .map(|(pid, name, reading)| ProcessMemoryInfo {
                pid: *pid,
                name: name.clone(),
                private_bytes: reading.private_bytes,
                working_set_bytes: reading.working_set_bytes,
                private_working_set_bytes: reading.private_working_set_bytes,
                peak_working_set_bytes: reading.peak_working_set_bytes,
                virtual_bytes: None,
                cpu_percent: None,
                share_percent: 0.0,
                trend_bytes_per_min: 0.0,
                trend_direction: TrendDirection::Stable,
                samples: 0,
                observation_seconds: 0,
                leak: None,
                started_at: None,
            })
            .collect();
        view.sort_by(|a, b| b.private_bytes.cmp(&a.private_bytes));
        view
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::monitoring::{CpuMetrics, HardwareInfo, MemoryMetrics, ProcessMetrics};

    fn test_db() -> Arc<Database> {
        let directory = tempfile::tempdir().expect("temporary database directory");
        Arc::new(Database::open_at_path(directory.path().join("memory.sqlite3")).expect("open test database"))
    }

    fn snapshot(available_percent: f32) -> SystemSnapshot {
        let total = 32 * 1024u64.pow(3);
        SystemSnapshot {
            collected_at: "2026-01-01T00:00:00Z".into(),
            monitoring: true,
            cpu: CpuMetrics {
                name: "cpu".into(),
                vendor: "v".into(),
                architecture: None,
                physical_cores: Some(1),
                logical_processors: 2,
                usage_percent: Some(1.0),
                frequency_mhz: None,
                max_frequency_mhz: None,
                temperature_c: None,
                cores: vec![],
            },
            memory: MemoryMetrics {
                total_bytes: total,
                used_bytes: (total as f32 * (100.0 - available_percent) / 100.0) as u64,
                available_bytes: (total as f32 * available_percent / 100.0) as u64,
                free_bytes: None,
                usage_percent: Some(100.0 - available_percent),
                cached_bytes: Some(4 * 1024u64.pow(3)),
                committed_bytes: Some(20 * 1024u64.pow(3)),
                commit_limit_bytes: Some(40 * 1024u64.pow(3)),
                pagefile_used_bytes: None,
                pagefile_total_bytes: None,
            },
            gpus: vec![],
            storage: vec![],
            processes: vec![ProcessMetrics {
                pid: 4,
                name: "test.exe".into(),
                cpu_percent: Some(1.0),
                memory_bytes: 1024,
                memory_percent: Some(0.1),
                gpu_percent: None,
                gpu_memory_bytes: None,
                started_at: None,
            }],
            hardware: HardwareInfo {
                os_name: "w".into(),
                os_version: "w".into(),
                host_name: "h".into(),
                gpus: vec![],
            },
            errors: vec![],
        }
    }

    #[test]
    fn tick_produces_a_complete_intelligence_snapshot() {
        let engine = MemoryEngine::new(test_db());
        engine.tick(&snapshot(50.0));
        let intelligence = engine.intelligence().expect("snapshot after tick");
        assert_eq!(intelligence.analysis.total_bytes, 32 * 1024u64.pow(3));
        assert_eq!(intelligence.pressure.level, PressureLevel::Normal);
        assert!(intelligence.policies.active.iter().any(|policy| policy.id == policies::POLICY_BACKGROUND));
        assert!(intelligence.recommendations.is_empty());
        assert!(intelligence.prediction.disclaimer.contains("trends only"));
    }

    #[test]
    fn analysis_only_mode_rejects_execution_but_audits_it() {
        let engine = MemoryEngine::new(test_db());
        engine.set_mode(OptimizationMode::AnalysisOnly);
        let result = engine
            .execute_action(recommendations::ACTION_REFRESH_DATA, true)
            .expect("result");
        assert_eq!(result.status, ActionStatus::Rejected);
        let intelligence_history = engine.history_series(10).expect("history");
        assert!(intelligence_history.actions.iter().any(|entry| entry.status == ActionStatus::Rejected));
    }

    #[test]
    fn manual_mode_executes_fluxcore_scoped_actions_with_confirmation() {
        let engine = MemoryEngine::new(test_db());
        engine.set_mode(OptimizationMode::Manual);
        engine.tick(&snapshot(50.0));
        let unconfirmed = engine
            .execute_action(recommendations::ACTION_CLEAR_CACHES, false)
            .expect("result");
        assert_eq!(unconfirmed.status, ActionStatus::Rejected);
        let confirmed = engine
            .execute_action(recommendations::ACTION_CLEAR_CACHES, true)
            .expect("result");
        assert_eq!(confirmed.status, ActionStatus::Success);
        assert!(confirmed.detail.contains("pruned"));
    }

    #[test]
    fn forbidden_actions_are_rejected_in_every_mode() {
        let engine = MemoryEngine::new(test_db());
        engine.set_mode(OptimizationMode::Manual);
        let result = engine.execute_action("kill_process", true).expect("result");
        assert_eq!(result.status, ActionStatus::Rejected);
        assert!(result.detail.contains("not supported"));
    }

    #[test]
    fn policy_config_validation_rejects_inverted_thresholds() {
        let engine = MemoryEngine::new(test_db());
        let mut config = PolicyConfig::default();
        config.thresholds.critical_available_percent = 50.0;
        assert!(engine.set_config(config).is_err());
        let mut config = PolicyConfig::default();
        config.retention_days = 0;
        assert!(engine.set_config(config).is_err());
        assert!(engine.set_config(PolicyConfig::default()).is_ok());
    }
}
