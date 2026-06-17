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
            mode: Mode::AnalysisOnly,
            paused: false,
            scan_interval_ms: 15_000,
            max_scan_interval_ms: 30_000,
            cooldown_seconds: 300,
            horizon_minutes: vec![1, 5, 15],
        }
    }
}

impl Policy {
    pub fn validate(&self) -> Result<(), String> {
        if !(5_000..=60_000).contains(&self.scan_interval_ms)
            || !(self.scan_interval_ms..=60_000).contains(&self.max_scan_interval_ms)
            || !(180..=3600).contains(&self.cooldown_seconds)
            || self.horizon_minutes.is_empty()
            || self.horizon_minutes.len() > 3
            || self.horizon_minutes.iter().any(|n| !(1..=15).contains(n))
            || self
                .horizon_minutes
                .windows(2)
                .any(|pair| pair[0] >= pair[1])
        {
            return Err("Use a process scan interval of 5–60 s within the configured maximum, a cooldown of 180–3600 s, and 1–3 distinct forecast horizons (1–15 min) in ascending order.".into());
        }
        Ok(())
