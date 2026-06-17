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
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn policy_limits_reject_unsafe_or_malformed_values() {
        assert!(Policy::default().validate().is_ok());
        let mut policy = Policy::default();
        policy.scan_interval_ms = 4_999;
        assert!(policy.validate().is_err());
        policy.scan_interval_ms = 15_000;
        policy.max_scan_interval_ms = 10_000;
        assert!(policy.validate().is_err());
        policy.max_scan_interval_ms = 30_000;
        policy.cooldown_seconds = 30;
        assert!(policy.validate().is_err());
        policy.cooldown_seconds = 300;
        policy.horizon_minutes = vec![1, 1];
        assert!(policy.validate().is_err());
        policy.horizon_minutes = vec![1, 5, 16];
        assert!(policy.validate().is_err());
    }
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanCounters {
    pub count: u64,
    pub elapsed_us: u64,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Observation {
    pub at: i64,
    pub values: BTreeMap<String, f64>,
    pub pressure: Option<String>,
    pub process_count: Option<usize>,
    pub scans: ScanCounters,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Severity {
    Information,
    Low,
    Medium,
    High,
    Critical,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Anomaly {
    pub id: String,
    pub metric: String,
    pub started_at: i64,
    pub ended_at: Option<i64>,
    pub resolution: Option<String>,
    pub severity: Severity,
    pub value: f64,
    pub threshold: f64,
    pub explanation: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Forecast {
    pub metric: String,
    pub observed_at: i64,
    pub target_at: i64,
    pub horizon_minutes: i64,
    pub value: Option<f64>,
    pub low: Option<f64>,
    pub high: Option<f64>,
    pub actual: Option<f64>,
    pub samples: usize,
    pub observation_seconds: i64,
    pub coverage: f64,
    pub confidence: String,
    pub method: String,
    pub explanation: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Recommendation {
    pub id: String,
    pub created_at: i64,
    pub resource: String,
    pub problem: String,
    pub evidence: Vec<String>,
    pub action: String,
    pub benefit: String,
    pub risk: String,
    pub confidence: String,
    pub requires_approval: bool,
    pub applicable: bool,
    pub severity: Severity,
    pub status: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Action {
    pub id: String,
    pub at: i64,
    pub mode: Mode,
    pub interval_ms: u64,
    pub previous_interval_ms: Option<u64>,
    pub recommendation: Option<String>,
    pub before: Vec<Observation>,
    pub outcome: Option<Outcome>,
    pub ended_at: Option<i64>,
    pub status: String,
    pub detail: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Outcome {
    pub verdict: String,
    pub detail: String,
    pub before: BTreeMap<String, f64>,
    pub after: BTreeMap<String, f64>,
