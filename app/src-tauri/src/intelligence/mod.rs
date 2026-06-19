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
