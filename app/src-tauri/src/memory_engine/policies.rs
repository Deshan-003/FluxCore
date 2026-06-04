//! Intelligent optimization policies.
//!
//! Policies map the detected pressure state onto FluxCore's own behavior and
//! user-facing guidance. They never act on other processes without an
//! explicit, confirmed user action.

use super::models::{ActivePolicy, MemoryAnalysis, PressureLevel, PressureReport, ProcessMemoryInfo};

pub const POLICY_LOW_MEMORY: &str = "low-memory";
pub const POLICY_HIGH_PRESSURE: &str = "high-pressure";
pub const POLICY_CRITICAL: &str = "critical-memory";
pub const POLICY_BACKGROUND: &str = "background-resource";
pub const POLICY_IDLE: &str = "idle";

/// Evaluates which policies are active for the current observation.
///
/// `detail_boost` reports whether the high-pressure policy wants the engine
/// to scan processes more often; `idle` reports whether the dashboard has
/// been unattended long enough for the idle policy to apply.
pub fn evaluate(
    pressure: &PressureReport,
    analysis: &MemoryAnalysis,
    processes: &[ProcessMemoryInfo],
    idle: bool,
) -> (Vec<ActivePolicy>, bool) {
    let mut active = Vec::new();
    let mut detail_boost = false;

    if pressure.level.rank() >= PressureLevel::Elevated.rank() {
        let top = processes.first();
        active.push(ActivePolicy {
            id: POLICY_LOW_MEMORY.to_string(),
            title: "Low memory policy".to_string(),
            detail: match top {
                Some(top) => format!(
                    "Available memory is {:.0}%; top consumer {} holds {:.1}% of physical RAM. Recommendations were generated from live samples.",
                    analysis.available_percent, top.name, top.share_percent
                ),
                None => format!(
                    "Available memory is {:.0}%; application consumption is being analyzed.",
                    analysis.available_percent
                ),
            },
        });
    }

    if pressure.level.rank() >= PressureLevel::High.rank() {
        detail_boost = true;
        active.push(ActivePolicy {
            id: POLICY_HIGH_PRESSURE.to_string(),
            title: "High memory pressure policy".to_string(),
            detail: "Sustained high pressure detected: process scanning runs at the boosted rate and recommendation detail increases.".to_string(),
        });
    }

    if pressure.level == PressureLevel::Critical {
        let heavy: Vec<String> = processes
            .iter()
            .take(3)
            .map(|process| format!("{} ({:.1}%)", process.name, process.share_percent))
            .collect();
        active.push(ActivePolicy {
            id: POLICY_CRITICAL.to_string(),
            title: "Critical memory policy".to_string(),
            detail: format!(
                "Memory is severely constrained. Heaviest consumers: {}. Safe user actions are recommended; FluxCore terminates nothing on its own.",
                if heavy.is_empty() { "unavailable".to_string() } else { heavy.join(", ") }
            ),
        });
    }

    active.push(ActivePolicy {
        id: POLICY_BACKGROUND.to_string(),
        title: "Background resource policy".to_string(),
        detail: if detail_boost {
            "Process scan interval is temporarily halved while pressure is high.".to_string()
        } else {
            "FluxCore monitors at the configured background interval.".to_string()
        },
    });

    if idle {
        active.push(ActivePolicy {
            id: POLICY_IDLE.to_string(),
            title: "Idle policy".to_string(),
            detail: "Dashboard unattended: process scans and chart updates run at the reduced idle rate.".to_string(),
        });
    }

    (active, detail_boost)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory_engine::models::{PressureSignals, TrendDirection};

    fn report(level: PressureLevel) -> PressureReport {
        PressureReport {
            level,
            level_since: "t".into(),
            reasons: vec![],
            signals: PressureSignals {
                available_percent: 50.0,
                commit_percent: None,
                pagefile_percent: None,
                hard_faults_per_sec: None,
                available_trend_bytes_per_min: 0.0,
            },
        }
    }
