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
