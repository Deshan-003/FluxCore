mod analysis;
pub mod models;
mod statistics;
mod storage;

use crate::{database::Database, memory_engine::MemoryEngine, monitoring::SystemSnapshot};
use chrono::{DateTime, Utc};
use models::*;
use serde::Deserialize;
use std::{
    sync::{Arc, Mutex, MutexGuard},
    time::{Duration, Instant},
};

const SAMPLE_SECONDS: i64 = 15;
const WINDOW_SECONDS: i64 = 1800;
const EVALUATE_SECONDS: i64 = 120;
const LEASE_SECONDS: i64 = 180;

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Decision {
    Apply,
    Reject,
    Dismiss,
}

struct Running {
    action: Action,
    expires: Instant,
    clear_since: Option<i64>,
}

#[derive(Default)]
struct State {
    policy: Policy,
    rows: Vec<Observation>,
    latest: Option<Observation>,
    forecasts: Vec<Forecast>,
    detector: analysis::Detector,
    anomaly_history: Vec<Anomaly>,
    recommendations: Vec<Recommendation>,
    health: String,
    insights: Vec<String>,
    status: Option<String>,
    monitoring: bool,
    history_enabled: bool,
    consent_required: bool,
    running: Option<Running>,
    last_action: Option<Instant>,
    forecast_at: Option<i64>,
    cleanup_at: Option<i64>,
}

pub struct IntelligenceEngine {
    db: Arc<Database>,
    memory: Arc<MemoryEngine>,
    state: Mutex<State>,
    gpu: std::sync::OnceLock<std::sync::Weak<crate::gpu_engine::GpuEngine>>,
}

impl IntelligenceEngine {
    pub fn new(db: Arc<Database>, memory: Arc<MemoryEngine>) -> Self {
        let mut state = State::default();
        match storage::policy(&db) {
            Ok(policy) => state.policy = policy,
            Err(error) => {
                state.status = Some(error);
                state.consent_required = true;
            }
        }
        if let Err(error) = Self::interrupt_records(&db) {
            state.status = Some(error);
            state.consent_required = true;
        }
        Self {
            db,
            memory,
            state: Mutex::new(state),
            gpu: Default::default(),
        }
    }

    pub fn attach_gpu(&self, gpu: &Arc<crate::gpu_engine::GpuEngine>) {
        let _ = self.gpu.set(Arc::downgrade(gpu));
    }

    fn interrupt_records(db: &Database) -> Result<(), String> {
        let now = Utc::now().timestamp();
        for mut action in storage::records::<Action>(db, "action", 100)?
            .into_iter()
            .filter(|a| a.ended_at.is_none())
        {
            action.status = "interrupted".into();
            action.ended_at = Some(now);
            action.detail = "FluxCore restarted; the temporary scan limit was not restored.".into();
            storage::decide(db, None, Some(&action))?;
        }
        for mut event in storage::records::<Anomaly>(db, "anomaly", 100)?
            .into_iter()
            .filter(|a| a.ended_at.is_none())
        {
            event.ended_at = Some(now);
            event.resolution = Some("FluxCore restarted; recovery not established".into());
            db.with_connection(|conn| {
                storage::record(conn, "anomaly", &event.id, event.started_at, &event)
            })?;
        }
        Ok(())
    }

    fn lock(&self) -> Result<MutexGuard<'_, State>, String> {
        self.state
            .lock()
            .map_err(|_| "resource intelligence lock poisoned".into())
    }

    pub fn state(&self) -> Result<ResourceState, String> {
        let state = self.lock()?;
        let mut anomalies = state.detector.active();
        anomalies.extend(
            state
                .anomaly_history
                .iter()
                .filter(|a| a.ended_at.is_some())
                .take(40)
                .cloned(),
        );
        Ok(ResourceState {
            policy: state.policy.clone(),
            monitoring: state.monitoring,
            history_enabled: state.history_enabled,
            latest: state.latest.clone(),
            health: if state.monitoring && state.latest.is_some() {
                state.health.clone()
            } else {
                "Waiting for live measurements".into()
            },
            insights: state.insights.clone(),
            forecasts: state.forecasts.clone(),
            anomalies,
            recommendations: state.recommendations.clone(),
            active_interval_ms: state
                .running
                .as_ref()
                .filter(|r| Instant::now() < r.expires)
                .map(|r| r.action.interval_ms),
            status: state.status.clone(),
        })
    }

    pub fn history(&self, metric: &str, horizon: i64) -> Result<ResourceHistory, String> {
        if metric.len() > 256 || !(1..=15).contains(&horizon) {
            return Err("invalid metric or forecast horizon".into());
        }
        let state = self.lock()?;
        Ok(ResourceHistory {
            observations: state
                .rows
                .iter()
                .rev()
                .take(121)
                .cloned()
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .collect(),
            comparisons: if state.history_enabled {
                storage::comparisons(&self.db, metric, horizon)?
            } else {
                Vec::new()
            },
            actions: storage::records(&self.db, "action", 40)?,
            decisions: storage::records(&self.db, "recommendation", 40)?,
        })
    }

    pub fn configure(&self, policy: Policy, confirmed: bool) -> Result<(), String> {
        policy.validate()?;
        let mut state = self.lock()?;
        if policy.mode == Mode::Adaptive
            && (state.policy.mode != Mode::Adaptive || state.consent_required)
            && !confirmed
        {
            return Err("Explicitly confirm enabling Adaptive Optimization".into());
        }
        let closed = state.running.as_ref().map(|r| {
            Self::closed(
                r,
                Utc::now().timestamp(),
                "reverted",
                "Policy changed; restored the Phase 2 scan cadence",
            )
        });
        storage::configure(&self.db, &policy, closed.as_ref())?;
        if closed.is_some() {
            self.remove_override(&mut state);
        }
        state.policy = policy;
        state.consent_required = false;
        state.forecast_at = None;
        state.status = None;
        Ok(())
    }

    fn can_change(state: &State) -> Result<(), String> {
        if !state.monitoring
            || !state.history_enabled
            || state.policy.paused
            || state.consent_required
            || state.status.is_some()
        {
            return Err(
                "Actions require current monitoring, local history, an active policy and a healthy database"
                    .into(),
            );
        }
        if state.running.is_some() {
            return Err("Revert the active scan policy before applying another".into());
        }
        if state.last_action.is_some_and(|at| {
            at.elapsed() < Duration::from_secs(state.policy.cooldown_seconds as u64)
        }) {
            return Err("The optimization cooldown is active".into());
        }
        Ok(())
    }

    fn fresh(state: &State, now: i64) -> bool {
        state
            .latest
            .as_ref()
            .is_some_and(|row| (0..=statistics::MAX_GAP).contains(&(now - row.at)))
    }

    pub fn respond(&self, id: &str, decision: Decision, confirmed: bool) -> Result<(), String> {
        let mut state = self.lock()?;
        let mut rec = state
            .recommendations
            .iter()
            .find(|r| r.id == id && r.status == "pending")
            .cloned()
            .ok_or("Recommendation is no longer pending; refresh the dashboard")?;
        if matches!(decision, Decision::Apply) {
            if !confirmed || state.policy.mode != Mode::SmartAssist || !rec.applicable {
                return Err("This recommendation is not approvable in the current mode".into());
            }
            let now = Utc::now().timestamp();
            if !Self::fresh(&state, now) {
                return Err("Measurements are stale".into());
            }
            rec.status = "approved".into();
            let interval = state.policy.scan_interval_ms;
            self.start(&mut state, interval, now, Some(&rec))?;
        } else {
            rec.status = if matches!(decision, Decision::Reject) {
                "rejected"
            } else {
                "dismissed"
            }
            .into();
            if state.history_enabled {
                storage::decide(&self.db, Some(&rec), None)?;
            }
        }
        if let Some(item) = state.recommendations.iter_mut().find(|r| r.id == id) {
            *item = rec;
        }
        Ok(())
    }

    pub fn manual(&self, confirmed: bool) -> Result<(), String> {
        let mut state = self.lock()?;
        let now = Utc::now().timestamp();
        if !confirmed || state.policy.mode != Mode::Manual || !Self::fresh(&state, now) {
            return Err("Manual mode, confirmation and current measurements are required".into());
        }
        let interval = state.policy.scan_interval_ms;
        self.start(&mut state, interval, now, None)
    }

    fn start(
        &self,
        state: &mut State,
        interval: u64,
        at: i64,
        rec: Option<&Recommendation>,
    ) -> Result<(), String> {
        Self::can_change(state)?;
        let limit = self.memory.config().process_scan_interval_ms;
        if interval <= limit || interval > state.policy.max_scan_interval_ms {
            return Err("The requested minimum scan interval is not above the Phase 2 configured interval or exceeds your limit".into());
        }
        if state
            .latest
            .as_ref()
            .and_then(|r| r.pressure.as_deref())
            .is_none_or(|p| p == "high" || p == "critical")
        {
            return Err("The scan limit is blocked when memory pressure is high or unknown".into());
        }
        let expires = Instant::now() + Duration::from_secs(LEASE_SECONDS as u64);
        let action = Action {
            id: format!(
                "scan@{}",
                Utc::now()
                    .timestamp_nanos_opt()
                    .unwrap_or(at * 1_000_000_000)
            ),
            at,
            mode: state.policy.mode,
            interval_ms: interval,
            previous_interval_ms: Some(limit),
            recommendation: rec.map(|r| r.id.clone()),
            before: state
                .rows
                .iter()
                .filter(|row| row.at >= at - EVALUATE_SECONDS)
                .cloned()
                .collect(),
            outcome: None,
            ended_at: None,
            status: "applied".into(),
            detail: "Temporary minimum interval applied to detailed FluxCore process scans only"
                .into(),
        };
        storage::decide(&self.db, rec, Some(&action))?;
        self.memory.set_scan_override(Some((interval, expires)));
        state.last_action = Some(Instant::now());
        state.running = Some(Running {
            action,
            expires,
            clear_since: None,
        });
        Ok(())
    }

    fn remove_override(&self, state: &mut State) {
        self.memory.set_scan_override(None);
        state.running = None;
        state.last_action = Some(Instant::now());
    }

    fn closed(running: &Running, at: i64, status: &str, detail: &str) -> Action {
        let mut action = running.action.clone();
        action.ended_at = Some(at);
        action.status = status.into();
        action.detail = detail.into();
        action
    }

    fn finish(&self, state: &mut State, at: i64, status: &str, detail: &str) -> Result<(), String> {
        if let Some(running) = state.running.as_ref() {
            let action = Self::closed(running, at, status, detail);
            self.remove_override(state);
            if let Err(error) = storage::decide(&self.db, None, Some(&action)) {
                state.consent_required = true;
                state.status = Some(error.clone());
                return Err(error);
            }
        }
        Ok(())
    }

    pub fn revert(&self, id: &str) -> Result<(), String> {
        let mut state = self.lock()?;
        if state.running.as_ref().is_none_or(|r| r.action.id != id) {
            return Err("This change is no longer active".into());
        }
        self.finish(
            &mut state,
            Utc::now().timestamp(),
            "reverted",
            "User restored the Phase 2 scan cadence",
        )
    }

    pub(crate) fn collection_status(&self, monitoring: bool, history_enabled: bool) {
        let Ok(mut state) = self.lock() else { return };
        if state.monitoring == monitoring && state.history_enabled == history_enabled {
            return;
        }
        if !monitoring || !history_enabled {
            let _ = self.finish(
                &mut state,
                Utc::now().timestamp(),
                "reverted",
                "Monitoring or local history was turned off",
            );
        }
        if !monitoring || state.history_enabled != history_enabled {
            let now = Utc::now().timestamp();
            let mut events = state.detector.active();
            for event in &mut events {
                event.ended_at = Some(now);
                event.resolution = Some("Collection changed; recovery not established".into());
                if state.history_enabled {
                    if let Err(error) = self.db.with_connection(|conn| {
                        storage::record(conn, "anomaly", &event.id, event.started_at, event)
                    }) {
                        state.status = Some(error);
                    }
                }
            }
            state.latest = None;
            state.rows.clear();
            state.forecasts.clear();
            state.forecast_at = None;
            state.detector = analysis::Detector::default();
            state.anomaly_history.clear();
            state.recommendations.clear();
            state.insights.clear();
        }
        if monitoring && history_enabled {
            let now = Utc::now().timestamp();
            match storage::observations(&self.db, now - WINDOW_SECONDS, 121) {
                Ok(rows) => state.rows = rows.into_iter().filter(|r| r.at <= now).collect(),
                Err(error) => state.status = Some(error),
            }
            match storage::records(&self.db, "anomaly", 40) {
                Ok(events) => state.anomaly_history = events,
                Err(error) => state.status = Some(error),
            }
        }
        state.monitoring = monitoring;
        state.history_enabled = history_enabled;
    }

    pub fn tick(&self, snapshot: &SystemSnapshot) {
        let Ok(date) = DateTime::parse_from_rfc3339(&snapshot.collected_at) else {
            return;
        };
        let at = date.timestamp();
        let Ok(mut state) = self.lock() else { return };
        if !state.monitoring
            || state
                .latest
                .as_ref()
                .is_some_and(|row| at - row.at < SAMPLE_SECONDS && at >= row.at)
        {
            return;
        }
        if state.rows.last().is_some_and(|r| r.at >= at) {
            state.rows.retain(|r| r.at < at);
        }
        let memory = self.memory.intelligence().ok();
        let mut current =
            analysis::observe(at, snapshot, memory.as_ref(), self.memory.scan_counters());
        if let Some(gpu) = self.gpu.get().and_then(std::sync::Weak::upgrade) {
            let (resources, tasks) = gpu.intelligence_resources();
            current
                .values
                .insert("gpuRunningTasks".into(), resources.running_gpu_tasks as f64);
            current
                .values
                .insert("gpuQueuedTasks".into(), resources.queued_gpu_tasks as f64);
            current.values.insert(
                "gpuBudgetRemaining".into(),
                resources.remaining_budget_bytes as f64,
            );
            let since = state
                .latest
                .as_ref()
                .map_or(at - SAMPLE_SECONDS, |row| row.at)
                .max(at - 60);
            for task in tasks
                .into_iter()
                .filter(|task| task.validated && !task.fell_back)
            {
                if let (Some(ms), Some(backend), Some(finished)) =
                    (task.total_time_ms, task.selected_backend, task.finished_at)
                {
                    let finished = DateTime::parse_from_rfc3339(&finished)
                        .ok()
                        .map(|date| date.timestamp());
                    if (0.0..=1e9).contains(&ms) && finished.is_some_and(|t| t > since && t <= at) {
                        let key = format!(
                            "workloadMs:{} · {} · {:?}",
                            task.workload_id,
                            backend.as_str(),
                            task.params
                        );
                        current.values.entry(key).or_insert(ms);
                    }
                }
            }
        }
        let State { detector, rows, .. } = &mut *state;
        let changes = detector.update(rows, &current);
        for event in &changes {
            if event.ended_at.is_some() {
                state.anomaly_history.insert(0, event.clone());
                state.anomaly_history.truncate(40);
            }
        }
        state.rows.retain(|row| at - row.at <= WINDOW_SECONDS);
        state.rows.push(current.clone());
        let forecast_due = state
            .forecast_at
            .is_none_or(|last| !(0..60).contains(&(at - last)));
        if forecast_due {
            state.forecasts = current
                .values
                .keys()
                .filter(|key| analysis::forecast_metric(key))
                .flat_map(|key| {
                    statistics::forecasts(&state.rows, key, &state.policy.horizon_minutes)
                })
                .collect();
            state.forecast_at = Some(at);
        } else {
            state
                .forecasts
                .retain(|forecast| current.values.contains_key(&forecast.metric));
        }
        let candidate = (state.policy.scan_interval_ms
            > self.memory.config().process_scan_interval_ms
            && state.history_enabled)
            .then_some(state.policy.scan_interval_ms);
        let mut findings = analysis::findings(
            &current,
            snapshot,
            memory.as_ref(),
            &state.detector.active(),
            &state.forecasts,
            candidate,
        );
        let mut new = Vec::new();
        for rec in &mut findings.recommendations {
            let prefix = format!("{}@", rec.id);
            if let Some(previous) = state
                .recommendations
                .iter()
                .find(|r| r.id.starts_with(&prefix))
            {
                rec.id.clone_from(&previous.id);
                rec.created_at = previous.created_at;
                rec.status.clone_from(&previous.status);
            } else {
                rec.id = format!("{prefix}{at}");
                new.push(rec.clone());
            }
        }
        state.recommendations = findings.recommendations;
        state.health = findings.health;
        state.insights = findings.insights;
        if state.history_enabled {
            let cleanup = state
                .cleanup_at
                .is_none_or(|last| at - last >= 3600 || at < last);
            match storage::sample(
                &self.db,
                &current,
                if forecast_due { &state.forecasts } else { &[] },
                &changes,
                &new,
                cleanup,
            ) {
                Ok(()) => {
                    if cleanup {
                        state.cleanup_at = Some(at);
                    }
                    if !state.consent_required {
                        state.status = None;
                    }
                }
                Err(error) => {
                    state.status = Some(error);
                    state.consent_required = true;
                    let _ = self.finish(&mut state, at, "reverted", "Local history unavailable");
                }
            }
        }
        state.latest = Some(current.clone());
        self.manage(&mut state, &current, findings.cpu_pressure);
    }

    fn manage(&self, state: &mut State, current: &Observation, cpu_pressure: bool) {
        let now = current.at;
        if let Some(running) = state.running.as_mut() {
            if current
                .pressure
                .as_deref()
                .is_none_or(|p| p == "high" || p == "critical")
            {
                let _ = self.finish(
                    state,
                    now,
                    "reverted",
                    "Memory pressure is high or unavailable; detailed scans take priority",
                );
                return;
            }
            let expired = Instant::now() >= running.expires;
            if running.action.outcome.is_none() && now - running.action.at >= EVALUATE_SECONDS {
                let after: Vec<_> = state
                    .rows
                    .iter()
                    .filter(|row| {
                        row.at > running.action.at && row.at <= running.action.at + LEASE_SECONDS
                    })
                    .cloned()
                    .collect();
                let outcome = analysis::evaluate(&running.action.before, &after);
                let regression = outcome.verdict == "regression";
                running.action.outcome = Some(outcome);
                if regression && state.policy.mode == Mode::Adaptive {
                    let _ = self.finish(
                        state,
                        now,
                        "reverted",
                        "Adaptive policy reverted after overhead regressed",
                    );
                    return;
                }
                if let Err(error) = storage::decide(&self.db, None, Some(&running.action)) {
                    state.status = Some(error);
                    let _ = self.finish(
                        state,
                        now,
                        "reverted",
                        "Could not save the optimization evaluation",
                    );
                    return;
                }
            }
            let clear = if running.action.mode == Mode::Adaptive
                && current.values.get("cpu").is_some_and(|v| *v < 70.0)
            {
                now - *running.clear_since.get_or_insert(now) >= 60
            } else {
                running.clear_since = None;
                false
            };
            if clear || expired {
                let _ = self.finish(
                    state,
                    now,
                    if clear { "reverted" } else { "expired" },
                    if clear {
                        "CPU load recovered for at least 60 seconds"
                    } else {
                        "Temporary scan limit expired; restored the Phase 2 cadence"
                    },
                );
            }
            return;
        }
        if state.policy.mode == Mode::Adaptive
            && cpu_pressure
            && Self::can_change(state).is_ok()
            && current.pressure.as_deref() == Some("normal")
        {
            let rec = state
                .recommendations
                .iter()
                .find(|r| r.applicable && r.status == "pending")
                .cloned();
            if let Some(mut rec) = rec {
                rec.status = "appliedAutomatically".into();
                match self.start(state, state.policy.scan_interval_ms, now, Some(&rec)) {
                    Ok(()) => {
                        if let Some(item) =
                            state.recommendations.iter_mut().find(|r| r.id == rec.id)
                        {
                            *item = rec;
                        }
                    }
                    Err(error) => {
                        state.status = Some(error);
                        state.consent_required = true;
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::monitoring::{CpuMetrics, HardwareInfo, MemoryMetrics};

    fn setup() -> (
        tempfile::TempDir,
        Arc<Database>,
        Arc<MemoryEngine>,
        IntelligenceEngine,
    ) {
        let dir = tempfile::tempdir().unwrap();
        let db = Arc::new(Database::open_at_path(dir.path().join("intelligence.sqlite")).unwrap());
        let memory = Arc::new(MemoryEngine::new(Arc::clone(&db)));
        let intelligence = IntelligenceEngine::new(Arc::clone(&db), Arc::clone(&memory));
        (dir, db, memory, intelligence)
    }

    fn snapshot() -> SystemSnapshot {
        SystemSnapshot {
            collected_at: Utc::now().to_rfc3339(),
            monitoring: true,
            cpu: CpuMetrics {
                name: "test CPU".into(),
                vendor: "test".into(),
                architecture: None,
                physical_cores: Some(2),
                logical_processors: 4,
                usage_percent: Some(30.0),
                frequency_mhz: None,
                max_frequency_mhz: None,
                temperature_c: None,
                cores: vec![],
            },
            memory: MemoryMetrics {
                total_bytes: 8 * 1024u64.pow(3),
                used_bytes: 4 * 1024u64.pow(3),
                available_bytes: 4 * 1024u64.pow(3),
                free_bytes: None,
                usage_percent: Some(50.0),
                cached_bytes: None,
                committed_bytes: None,
                commit_limit_bytes: None,
                pagefile_used_bytes: None,
                pagefile_total_bytes: None,
            },
            gpus: vec![],
            storage: vec![],
            processes: vec![],
            hardware: HardwareInfo {
                os_name: "test OS".into(),
                os_version: "test".into(),
                host_name: "test host".into(),
                gpus: vec![],
            },
            errors: vec![],
        }
    }

    #[test]
    fn manual_policy_is_explicit_reversible_bounded_and_persisted() {
        let (_dir, db, _memory, intelligence) = setup();
        let mut policy = Policy {
            mode: Mode::Manual,
            ..Policy::default()
        };
        policy.cooldown_seconds = 180;
        intelligence.configure(policy.clone(), false).unwrap();
        intelligence.collection_status(true, true);
        let snapshot = snapshot();
        let epoch = DateTime::parse_from_rfc3339(&snapshot.collected_at)
            .unwrap()
            .timestamp();
        intelligence.memory.tick(&snapshot);
        intelligence.tick(&snapshot);
        assert!(intelligence.manual(false).is_err());
        assert!(intelligence.manual(true).is_ok());
        let state = intelligence.state().unwrap();
        let action = storage::records::<Action>(&db, "action", 2)
            .unwrap()
            .remove(0);
        assert_eq!(state.active_interval_ms, Some(policy.scan_interval_ms));
        assert_eq!(action.previous_interval_ms, Some(5000));
        assert_eq!(action.ended_at, None);
        assert!(action.at >= epoch - 1 && action.at <= Utc::now().timestamp());
        intelligence.revert(&action.id).unwrap();
        assert_eq!(intelligence.state().unwrap().active_interval_ms, None);
        let action = storage::records::<Action>(&db, "action", 1)
            .unwrap()
            .remove(0);
        assert_eq!(action.status, "reverted");
        assert!(action.ended_at.is_some());
        assert_eq!(storage::policy(&db).unwrap().mode, Mode::Manual);
    }

    #[test]
    fn smart_assist_approval_requires_confirmation_for_a_supported_action() {
        let (_dir, db, _memory, intelligence) = setup();
        intelligence
            .configure(
                Policy {
                    mode: Mode::SmartAssist,
                    ..Policy::default()
                },
                false,
            )
            .unwrap();
        intelligence.collection_status(true, true);
        let now = Utc::now().timestamp();
        {
            let mut state = intelligence.state.lock().unwrap();
            state.latest = Some(Observation {
                at: now,
                values: [("cpu".into(), 95.0)].into(),
                pressure: Some("normal".into()),
                ..Default::default()
            });
            state.recommendations.push(Recommendation {
                id: "scan@test".into(),
                created_at: now,
                resource: "Monitoring".into(),
                problem: "Optional process scans compete for CPU time".into(),
                evidence: vec!["Sustained CPU pressure".into()],
                action: "Reduce optional process scan frequency".into(),
                benefit: "May reduce FluxCore scan overhead".into(),
                risk: "Process details update less often".into(),
                confidence: "moderate".into(),
                requires_approval: true,
                applicable: true,
                severity: Severity::Low,
                status: "pending".into(),
            });
        }
        assert!(intelligence
            .respond("scan@test", Decision::Apply, false)
            .is_err());
        intelligence
            .respond("scan@test", Decision::Apply, true)
            .unwrap();
        assert_eq!(
            intelligence.state().unwrap().active_interval_ms,
            Some(15_000)
        );
        let action = storage::records::<Action>(&db, "action", 1)
            .unwrap()
            .remove(0);
        assert_eq!(action.recommendation.as_deref(), Some("scan@test"));
        assert_eq!(action.status, "applied");
        intelligence.revert(&action.id).unwrap();
    }

    #[test]
    fn adaptive_mode_requires_confirmation_and_stale_measurements_block_actions() {
        let (_dir, _db, _memory, intelligence) = setup();
        let policy = Policy {
            mode: Mode::Adaptive,
            ..Policy::default()
        };
        assert!(intelligence.configure(policy.clone(), false).is_err());
        intelligence.configure(policy, true).unwrap();
        intelligence.collection_status(true, true);
        let mut snapshot = snapshot();
        snapshot.collected_at = (Utc::now() - chrono::Duration::seconds(120)).to_rfc3339();
        intelligence.memory.tick(&snapshot);
        intelligence.tick(&snapshot);
        assert!(intelligence.manual(true).is_err());
    }

    #[test]
    fn restart_marks_an_unfinished_override_interrupted() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("restart.sqlite");
        let db = Arc::new(Database::open_at_path(path.clone()).unwrap());
        let memory = Arc::new(MemoryEngine::new(Arc::clone(&db)));
        let intelligence = IntelligenceEngine::new(Arc::clone(&db), Arc::clone(&memory));
        intelligence
            .configure(
                Policy {
                    mode: Mode::Manual,
                    ..Policy::default()
                },
                false,
            )
            .unwrap();
        intelligence.collection_status(true, true);
        let snapshot = snapshot();
        intelligence.memory.tick(&snapshot);
        intelligence.tick(&snapshot);
        intelligence.manual(true).unwrap();
        let running = storage::records::<Action>(&db, "action", 1)
            .unwrap()
            .remove(0);
        assert_eq!(running.status, "applied");
        assert_eq!(
            intelligence.state().unwrap().active_interval_ms,
            Some(15_000)
        );
        drop(intelligence);
        drop(memory);
        drop(db);

        let db = Arc::new(Database::open_at_path(path).unwrap());
        let memory = Arc::new(MemoryEngine::new(Arc::clone(&db)));
        let intelligence = IntelligenceEngine::new(Arc::clone(&db), Arc::clone(&memory));
        let action = storage::records::<Action>(&db, "action", 1)
            .unwrap()
            .remove(0);
        assert_eq!(action.id, running.id);
        assert_eq!(action.status, "interrupted");
        assert!(action.ended_at.is_some());
        assert_eq!(intelligence.state().unwrap().active_interval_ms, None);
    }

    #[test]
    fn disabling_monitoring_clears_the_live_view_and_fails_closed() {
        let (_dir, _db, _memory, intelligence) = setup();
        intelligence.collection_status(true, true);
        let snapshot = snapshot();
        intelligence.memory.tick(&snapshot);
        intelligence.tick(&snapshot);
        assert!(intelligence.state().unwrap().latest.is_some());
        intelligence.collection_status(false, true);
        let state = intelligence.state().unwrap();
        assert!(state.latest.is_none());
        assert!(state.forecasts.is_empty());
        assert_eq!(state.health, "Waiting for live measurements");
    }
}
