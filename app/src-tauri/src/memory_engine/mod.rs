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
