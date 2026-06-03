//! Intelligent memory pressure detection.
//!
//! Pressure is never derived from RAM utilization alone: the detector weighs
//! available physical memory, commit charge, pagefile usage, hard-fault rate
//! and the available-memory trend. Hysteresis plus minimum-duration rules
//! stop the state from flapping between levels.

use super::models::{PressureLevel, PressureReport, PressureSignals, PressureThresholds};
use std::time::Instant;

pub struct PressureDetector {
    level: PressureLevel,
    level_since: Instant,
    level_since_ts: String,
    pending: Option<(PressureLevel, Instant)>,
}

impl Default for PressureDetector {
    fn default() -> Self {
        Self::new()
    }
}

impl PressureDetector {
    pub fn new() -> Self {
        Self {
            level: PressureLevel::Normal,
            level_since: Instant::now(),
            level_since_ts: chrono::Utc::now().to_rfc3339(),
            pending: None,
        }
    }

    /// Instantaneous level from all signals, plus human-readable reasons.
    pub fn evaluate_instant(
        analysis: &super::models::MemoryAnalysis,
        trend_bytes_per_min: f64,
        thresholds: &PressureThresholds,
    ) -> (PressureLevel, Vec<String>, PressureSignals) {
        let mut reasons: Vec<String> = Vec::new();
        let mut level = PressureLevel::Normal;
        macro_rules! consider {
            ($candidate:expr, $reason:expr $(,)?) => {
                if $candidate.rank() > level.rank() {
                    level = $candidate;
                    reasons.push($reason);
                }
            };
        }

        let available = analysis.available_percent;
        if available <= thresholds.critical_available_percent {
            consider!(
                PressureLevel::Critical,
                format!("Available physical memory is {available:.0}% of installed RAM."),
            );
        } else if available <= thresholds.high_available_percent {
            consider!(
                PressureLevel::High,
                format!("Available physical memory is {available:.0}% of installed RAM."),
            );
        } else if available <= thresholds.elevated_available_percent {
            consider!(
                PressureLevel::Elevated,
                format!("Available physical memory is {available:.0}% of installed RAM."),
            );
        }

        if let Some(commit) = analysis.commit_percent {
            if commit >= thresholds.critical_commit_percent {
                consider!(
                    PressureLevel::Critical,
                    format!("Commit charge is {commit:.0}% of the commit limit."),
                );
            } else if commit >= thresholds.high_commit_percent {
                consider!(
                    PressureLevel::High,
                    format!("Commit charge is {commit:.0}% of the commit limit."),
                );
            } else if commit >= thresholds.elevated_commit_percent {
                consider!(
                    PressureLevel::Elevated,
                    format!("Commit charge is {commit:.0}% of the commit limit."),
                );
            }
        }

        if let Some(pagefile) = analysis.pagefile_percent {
            if pagefile >= 90.0 {
                consider!(
                    PressureLevel::High,
                    format!("Pagefile usage is {pagefile:.0}% of pagefile capacity."),
                );
            } else if pagefile >= 70.0 {
                consider!(
                    PressureLevel::Elevated,
                    format!("Pagefile usage is {pagefile:.0}% of pagefile capacity."),
                );
            }
        }

        if let Some(faults) = analysis.hard_faults_per_sec {
            if faults >= thresholds.hard_faults_high_per_sec * 4.0
                && available <= thresholds.high_available_percent + 10.0
            {
                consider!(
                    PressureLevel::High,
                    format!("Sustained hard-fault rate of {faults:.0} page reads/s indicates active paging."),
                );
            } else if faults >= thresholds.hard_faults_high_per_sec {
                consider!(
                    PressureLevel::Elevated,
                    format!("Hard-fault rate of {faults:.0} page reads/s is above the impact threshold."),
                );
            }
        }

        // A steep decline in available memory escalates one step, but only
        // when availability is already below a comfortable margin, so that
        // ordinary allocation bursts on a healthy system stay NORMAL.
        let decline_floor = analysis.total_bytes as f64 * 0.015; // 1.5% of RAM per minute
        if trend_bytes_per_min < -decline_floor && available <= thresholds.elevated_available_percent + 15.0 {
            let escalated = PressureLevel::from_rank(level.rank().max(1));
            consider!(
                escalated,
                format!(
                    "Available memory is falling at {:.1} GB/min.",
                    (-trend_bytes_per_min) / 1024.0f64.powi(3)
                ),
            );
        }

        if level == PressureLevel::Normal {
            reasons.push(format!(
                "Available memory {available:.0}% and commit load are within normal bounds; cached and standby memory remain reclaimable."
            ));
        }

        let signals = PressureSignals {
            available_percent: available,
            commit_percent: analysis.commit_percent,
            pagefile_percent: analysis.pagefile_percent,
            hard_faults_per_sec: analysis.hard_faults_per_sec,
            available_trend_bytes_per_min: trend_bytes_per_min,
        };
        (level, reasons, signals)
    }

    /// Applies hysteresis: a worse level must persist for `upgrade_min_seconds`
    /// and a better level for `downgrade_min_seconds` before it is adopted.
    pub fn update(
        &mut self,
        analysis: &super::models::MemoryAnalysis,
        trend_bytes_per_min: f64,
        thresholds: &PressureThresholds,
        upgrade_min_seconds: u64,
        downgrade_min_seconds: u64,
        now: Instant,
        now_ts: &str,
    ) -> PressureReport {
        let (instant, reasons, signals) =
            Self::evaluate_instant(analysis, trend_bytes_per_min, thresholds);

        if instant.rank() > self.level.rank() {
            self.consider(instant, upgrade_min_seconds, now, now_ts);
        } else if instant.rank() < self.level.rank() {
            self.consider(instant, downgrade_min_seconds, now, now_ts);
        } else {
            self.pending = None;
        }

        PressureReport {
            level: self.level,
            level_since: self.level_since_ts.clone(),
            reasons,
            signals,
        }
    }

    /// Tracks a candidate level across updates: it is adopted only once it has
    /// been observed continuously for `min_seconds`. A zero minimum is
    /// satisfied immediately, so the candidate is adopted on the spot.
    fn consider(&mut self, instant: PressureLevel, min_seconds: u64, now: Instant, now_ts: &str) {
        let sustained = match self.pending {
            Some((pending_level, since)) if pending_level == instant => {
                now.duration_since(since).as_secs() >= min_seconds
            }
            _ => {
                self.pending = Some((instant, now));
                min_seconds == 0
            }
        };
        if sustained {
            self.level = instant;
            self.level_since = now;
            self.level_since_ts = now_ts.to_string();
            self.pending = None;
        }
    }

    pub fn current_level(&self) -> PressureLevel {
        self.level
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory_engine::models::MemoryAnalysis;

    fn analysis(available_percent: f32, commit_percent: Option<f32>) -> MemoryAnalysis {
        let total = 32u64 * 1024u64.pow(3);
        MemoryAnalysis {
            collected_at: "t".into(),
            total_bytes: total,
            used_bytes: (total as f32 * (100.0 - available_percent) / 100.0) as u64,
            available_bytes: (total as f32 * available_percent / 100.0) as u64,
            free_bytes: None,
            usage_percent: 100.0 - available_percent,
            available_percent,
            cached_bytes: Some(4 * 1024u64.pow(3)),
            standby_bytes: Some(2 * 1024u64.pow(3)),
            modified_bytes: None,
            compressed_bytes: None,
            committed_bytes: None,
            commit_limit_bytes: None,
            commit_percent,
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

    fn level_available(level: PressureLevel, thresholds: &PressureThresholds) -> f32 {
        match level {
            PressureLevel::Normal => thresholds.elevated_available_percent + 20.0,
            PressureLevel::Elevated => thresholds.elevated_available_percent - 1.0,
            PressureLevel::High => thresholds.high_available_percent - 1.0,
            PressureLevel::Critical => thresholds.critical_available_percent - 1.0,
        }
    }

    #[test]
    fn high_cache_does_not_raise_pressure() {
        let thresholds = PressureThresholds::default();
        let mut detector = PressureDetector::new();
        let mut healthy = analysis(60.0, Some(40.0));
        healthy.cached_bytes = Some(12 * 1024u64.pow(3));
        healthy.standby_bytes = Some(6 * 1024u64.pow(3));
        let out = detector.update(&healthy, 0.0, &thresholds, 15, 45, Instant::now(), "t");
        assert_eq!(out.level, PressureLevel::Normal);
    }

    #[test]
    fn commit_charge_alone_can_raise_pressure() {
        let thresholds = PressureThresholds::default();
        let (instant, _, _) = PressureDetector::evaluate_instant(&analysis(50.0, Some(96.0)), 0.0, &thresholds);
        assert_eq!(instant, PressureLevel::Critical);
    }

    #[test]
    fn upgrade_requires_minimum_duration() {
        let thresholds = PressureThresholds::default();
        let mut detector = PressureDetector::new();
        let start = Instant::now();
        let critical = analysis(5.0, None);
        // First observation: pending, not yet adopted.
        let out = detector.update(&critical, 0.0, &thresholds, 15, 45, start, "t0");
        assert_eq!(out.level, PressureLevel::Normal);
        // Still inside the hysteresis window.
        let out = detector.update(
            &critical,
            0.0,
            &thresholds,
            15,
            45,
            start + std::time::Duration::from_secs(10),
            "t1",
        );
        assert_eq!(out.level, PressureLevel::Normal);
        // Sustained past the window: adopted.
        let out = detector.update(
            &critical,
            0.0,
            &thresholds,
            15,
            45,
            start + std::time::Duration::from_secs(16),
            "t2",
        );
        assert_eq!(out.level, PressureLevel::Critical);
    }

    #[test]
    fn downgrade_is_slower_than_upgrade() {
        let thresholds = PressureThresholds::default();
        let mut detector = PressureDetector::new();
        let start = Instant::now();
        let critical = analysis(5.0, None);
        detector.update(&critical, 0.0, &thresholds, 0, 45, start, "t0");
        assert_eq!(detector.current_level(), PressureLevel::Critical);
        let healthy = analysis(60.0, None);
        let out = detector.update(
            &healthy,
            0.0,
            &thresholds,
            0,
            45,
            start + std::time::Duration::from_secs(30),
            "t1",
        );
        assert_eq!(out.level, PressureLevel::Critical, "downgrade must wait out its window");
        // 46 s after the first healthy observation: the 45 s window has elapsed.
        let out = detector.update(
            &healthy,
            0.0,
            &thresholds,
            0,
            45,
            start + std::time::Duration::from_secs(76),
            "t2",
        );
        assert_eq!(out.level, PressureLevel::Normal);
    }

    #[test]
    fn brief_spike_does_not_flap_state() {
        let thresholds = PressureThresholds::default();
        let mut detector = PressureDetector::new();
        let start = Instant::now();
        let critical = analysis(5.0, None);
        let healthy = analysis(60.0, None);
        detector.update(&critical, 0.0, &thresholds, 15, 45, start, "t0");
        // A single healthy sample cancels the pending upgrade entirely.
        let out = detector.update(
            &healthy,
            0.0,
            &thresholds,
            15,
            45,
            start + std::time::Duration::from_secs(5),
            "t1",
        );
        assert_eq!(out.level, PressureLevel::Normal);
        let out = detector.update(
            &critical,
            0.0,
            &thresholds,
            15,
            45,
            start + std::time::Duration::from_secs(20),
            "t2",
        );
        assert_eq!(out.level, PressureLevel::Normal, "pending timer restarted, window not elapsed");
    }

    #[test]
    fn hard_faults_need_low_availability_to_reach_high() {
        let thresholds = PressureThresholds::default();
        let mut faulty = analysis(50.0, None);
        faulty.hard_faults_per_sec = Some(3000.0);
        let (instant, reasons, _) = PressureDetector::evaluate_instant(&faulty, 0.0, &thresholds);
        assert_eq!(instant, PressureLevel::Elevated, "faults alone escalate one step");
        assert!(reasons.iter().any(|reason| reason.contains("page reads/s")));
    }

    #[test]
    fn level_available_helper_matches_levels() {
        let thresholds = PressureThresholds::default();
        assert!(level_available(PressureLevel::Normal, &thresholds) > thresholds.elevated_available_percent);
        assert!(level_available(PressureLevel::Critical, &thresholds) <= thresholds.critical_available_percent);
    }
}
