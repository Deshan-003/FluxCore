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
