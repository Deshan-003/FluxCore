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
