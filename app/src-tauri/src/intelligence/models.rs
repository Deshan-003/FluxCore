use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Mode {
    AnalysisOnly,
    SmartAssist,
    Adaptive,
    Manual,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Policy {
    pub mode: Mode,
    pub paused: bool,
    pub scan_interval_ms: u64,
    pub max_scan_interval_ms: u64,
    pub cooldown_seconds: i64,
    pub horizon_minutes: Vec<i64>,
}

impl Default for Policy {
    fn default() -> Self {
        Self {
