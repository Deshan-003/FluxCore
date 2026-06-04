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

    fn analysis() -> MemoryAnalysis {
        MemoryAnalysis {
            collected_at: "t".into(),
            total_bytes: 32 * 1024u64.pow(3),
            used_bytes: 0,
            available_bytes: 0,
            free_bytes: None,
            usage_percent: 50.0,
            available_percent: 50.0,
            cached_bytes: None,
            standby_bytes: None,
            modified_bytes: None,
            compressed_bytes: None,
            committed_bytes: None,
            commit_limit_bytes: None,
            commit_percent: None,
            pagefile_used_bytes: None,
            pagefile_total_bytes: None,
            pagefile_percent: None,
            hard_faults_per_sec: None,
            page_faults_per_sec: None,
            memory_load_percent: None,
            sources: vec![],
            limitations: vec![],
        }
    }

    fn process(name: &str, share: f32) -> ProcessMemoryInfo {
        ProcessMemoryInfo {
            pid: 1,
            name: name.to_string(),
            private_bytes: 0,
            working_set_bytes: 0,
            private_working_set_bytes: None,
            peak_working_set_bytes: 0,
            virtual_bytes: None,
            cpu_percent: None,
            share_percent: share,
            trend_bytes_per_min: 0.0,
            trend_direction: TrendDirection::Stable,
            samples: 0,
            observation_seconds: 0,
            leak: None,
            started_at: None,
        }
    }

    fn ids(policies: &[ActivePolicy]) -> Vec<&str> {
        policies.iter().map(|policy| policy.id.as_str()).collect()
    }

    #[test]
    fn normal_pressure_activates_only_background_policy() {
        let (active, boost) = evaluate(&report(PressureLevel::Normal), &analysis(), &[], false);
        assert_eq!(ids(&active), vec![POLICY_BACKGROUND]);
        assert!(!boost);
    }

    #[test]
    fn elevated_pressure_activates_low_memory_policy() {
        let (active, boost) = evaluate(
            &report(PressureLevel::Elevated),
            &analysis(),
            &[process("browser", 22.0)],
            false,
        );
        assert!(ids(&active).contains(&POLICY_LOW_MEMORY));
        assert!(active.iter().any(|policy| policy.detail.contains("browser")));
        assert!(!boost);
    }

    #[test]
    fn high_pressure_boosts_detail_and_critical_warns() {
        let (active, boost) = evaluate(&report(PressureLevel::High), &analysis(), &[], false);
        assert!(ids(&active).contains(&POLICY_HIGH_PRESSURE));
        assert!(boost);

        let (active, _) = evaluate(&report(PressureLevel::Critical), &analysis(), &[process("editor", 40.0)], false);
        assert!(ids(&active).contains(&POLICY_CRITICAL));
        assert!(active.iter().any(|policy| policy.detail.contains("editor")));
    }

    #[test]
    fn idle_policy_only_when_unattended() {
        let (active, _) = evaluate(&report(PressureLevel::Normal), &analysis(), &[], true);
        assert!(ids(&active).contains(&POLICY_IDLE));
        let (active, _) = evaluate(&report(PressureLevel::Normal), &analysis(), &[], false);
        assert!(!ids(&active).contains(&POLICY_IDLE));
    }
}
