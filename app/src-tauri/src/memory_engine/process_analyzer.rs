//! Per-process memory analysis built on documented Windows APIs.
//!
//! Samples are taken with `OpenProcess` plus `K32GetProcessMemoryInfo`
//! (`PROCESS_MEMORY_COUNTERS_EX2`, which exposes the private working set).
//! Samples are kept per PID in a bounded history so the trend can be derived
//! with `stats::linear_regression`; leak indicators and the process ranking
//! are reported alongside the raw counters.

use super::models::{LeakConfidence, LeakIndicator, ProcessMemoryInfo, ProcessSample};
use super::stats;
use std::collections::HashMap;

/// One raw counter reading for a single process.
#[derive(Clone, Copy, Debug, Default)]
pub struct ProcessReading {
    pub private_bytes: u64,
    pub working_set_bytes: u64,
    pub private_working_set_bytes: Option<u64>,
    pub peak_working_set_bytes: u64,
}

/// Reads the current counters for one process, if it is still alive.
#[cfg(windows)]
pub fn read_process(pid: u32) -> Option<ProcessReading> {
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::ProcessStatus::{K32GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS_EX2};
    use windows::Win32::System::Threading::{OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION};

    unsafe {
        let Ok(handle) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) else {
            return None;
        };
        if handle.is_invalid() {
            return None;
        }
        let mut counters = PROCESS_MEMORY_COUNTERS_EX2::default();
        counters.cb = std::mem::size_of::<PROCESS_MEMORY_COUNTERS_EX2>() as u32;
        let ok = K32GetProcessMemoryInfo(
            handle,
            &mut counters as *mut _ as *mut _,
            counters.cb,
        );
        let _ = CloseHandle(handle);
        if !ok.as_bool() {
            return None;
        }
        Some(ProcessReading {
            private_bytes: counters.PrivateUsage as u64,
            working_set_bytes: counters.WorkingSetSize as u64,
            private_working_set_bytes: Some(counters.PrivateWorkingSetSize as u64),
            peak_working_set_bytes: counters.PeakWorkingSetSize as u64,
        })
    }
}

#[cfg(not(windows))]
pub fn read_process(_pid: u32) -> Option<ProcessReading> {
    None
}

/// Bounded per-PID sample history plus the derived trend and leak analysis.
pub struct ProcessTracker {
    history: HashMap<u32, Vec<ProcessSample>>,
    capacity: usize,
}

impl Default for ProcessTracker {
    fn default() -> Self {
        Self::new()
    }
}

impl ProcessTracker {
    pub fn new() -> Self {
        Self { history: HashMap::new(), capacity: 96 }
    }

    pub fn record(&mut self, pid: u32, sample: ProcessSample) {
        let entry = self.history.entry(pid).or_default();
        entry.push(sample);
        if entry.len() > self.capacity {
            let overflow = entry.len() - self.capacity;
            entry.drain(..overflow);
        }
    }

    pub fn forget(&mut self, pid: u32) {
        self.history.remove(&pid);
    }

    pub fn samples(&self, pid: u32) -> &[ProcessSample] {
        self.history.get(&pid).map(Vec::as_slice).unwrap_or(&[])
    }

    pub fn pids(&self) -> Vec<u32> {
        self.history.keys().copied().collect()
    }

    fn points(&self, pid: u32) -> Vec<(f64, f64)> {
        self.samples(pid)
            .iter()
            .map(|sample| (sample.at_secs, sample.private_bytes as f64))
            .collect()
    }

    /// Private-memory growth rate in bytes per minute from the PID's history.
    pub fn slope_bytes_per_min(&self, pid: u32) -> f64 {
        let points = self.points(pid);
        stats::linear_regression(&points).map(|fit| fit.slope * 60.0).unwrap_or(0.0)
    }

    pub fn observation_seconds(&self, pid: u32) -> u64 {
        let samples = self.samples(pid);
        match (samples.first(), samples.last()) {
            (Some(first), Some(last)) if last.at_secs > first.at_secs => {
                (last.at_secs - first.at_secs) as u64
            }
            _ => 0,
        }
    }

    /// Fraction of consecutive deltas where private memory grew.
    fn positive_delta_ratio(&self, pid: u32) -> f32 {
        let samples = self.samples(pid);
        let deltas: Vec<i64> = samples
            .windows(2)
            .map(|window| window[1].private_bytes as i64 - window[0].private_bytes as i64)
            .collect();
        if deltas.is_empty() {
            return 0.0;
        }
        deltas.iter().filter(|delta| **delta > 0).count() as f32 / deltas.len() as f32
    }

    /// Slow-leak indicator: sustained, mostly-monotonic private growth with a
    /// statistically meaningful fit.
    ///
    /// Rule: at least 8 samples over at least 240 s, slope above
    /// max(1 MiB/min, 0.5% of current private memory per minute), at least
    /// 80% of deltas positive, and r² ≥ 0.6.
    pub fn leak_indicator(&self, pid: u32, current_private_bytes: u64) -> Option<LeakIndicator> {
        let samples = self.samples(pid);
        let observation = self.observation_seconds(pid);
        if samples.len() < 8 || observation < 240 {
            return None;
        }
        let points = self.points(pid);
        let fit = stats::linear_regression(&points)?;
        let slope = fit.slope * 60.0;
        let floor = (1024u64.pow(2) as f64).max(current_private_bytes as f64 * 0.005);
        let ratio = self.positive_delta_ratio(pid);
        if slope <= floor || ratio < 0.8 || fit.r2 < 0.6 {
            return None;
        }
        let confidence = if fit.r2 >= 0.9 && samples.len() >= 20 {
            LeakConfidence::High
        } else if fit.r2 >= 0.75 {
            LeakConfidence::Medium
        } else {
            LeakConfidence::Low
        };
        Some(LeakIndicator {
            confidence,
            growth_bytes_per_min: slope,
            reason: format!(
                "Private memory grew at {:.1} MB/min across {} samples over {} s (r² {:.2}, {}% of intervals increasing).",
                slope / 1024.0 / 1024.0,
                samples.len(),
                observation,
                fit.r2,
                (ratio * 100.0) as u32
            ),
        })
    }
}

/// Ranks the current process set by private memory and annotates each entry
/// with its trend, leak indicator and share of physical memory.
pub fn analyze(
    current: &[(u32, String, ProcessReading)],
    tracker: &ProcessTracker,
    total_physical: u64,
    cpu_percent: &HashMap<u32, f32>,
    started_at: &HashMap<u32, String>,
) -> Vec<ProcessMemoryInfo> {
    let mut entries: Vec<ProcessMemoryInfo> = current
        .iter()
        .map(|(pid, name, reading)| {
            let slope = tracker.slope_bytes_per_min(*pid);
            let noise_floor = (512.0f64 * 1024.0).max(reading.private_bytes as f64 * 0.0025);
            ProcessMemoryInfo {
                pid: *pid,
                name: name.clone(),
                private_bytes: reading.private_bytes,
                working_set_bytes: reading.working_set_bytes,
                private_working_set_bytes: reading.private_working_set_bytes,
                peak_working_set_bytes: reading.peak_working_set_bytes,
                virtual_bytes: None,
                cpu_percent: cpu_percent.get(pid).copied(),
                share_percent: if total_physical == 0 {
                    0.0
                } else {
                    reading.private_bytes as f32 * 100.0 / total_physical as f32
                },
                trend_bytes_per_min: slope,
                trend_direction: stats::direction_of(slope, noise_floor),
                samples: tracker.samples(*pid).len() as u32,
                observation_seconds: tracker.observation_seconds(*pid),
                leak: tracker.leak_indicator(*pid, reading.private_bytes),
                started_at: started_at.get(pid).cloned(),
            }
        })
        .collect();
    entries.sort_by(|a, b| b.private_bytes.cmp(&a.private_bytes));
    entries
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::models::TrendDirection;

    fn sample(at_secs: f64, private: u64) -> ProcessSample {
        ProcessSample { at_secs, private_bytes: private, working_set_bytes: private * 2 }
    }

    fn growing_tracker() -> ProcessTracker {
        let mut tracker = ProcessTracker::new();
        // 8 MiB per minute for ten minutes, one sample per 60 s — comfortably
        // above the leak floor for a 1 GiB process (max(1 MiB, 0.5% of private)).
        for minute in 0..10u64 {
            tracker.record(7, sample(minute as f64 * 60.0, 1024u64.pow(3) + minute * 8 * 1024u64.pow(2)));
        }
        tracker
    }

    #[test]
    fn trend_is_derived_from_history() {
        let tracker = growing_tracker();
        let slope = tracker.slope_bytes_per_min(7);
        assert!((slope - 8.0 * 1024u64.pow(2) as f64).abs() < 4096.0);
        assert_eq!(tracker.observation_seconds(7), 540);
    }

    #[test]
    fn leak_indicator_needs_sustained_growth() {
        let tracker = growing_tracker();
        let leak = tracker.leak_indicator(7, 1024u64.pow(3)).expect("sustained growth is a leak candidate");
        assert!(leak.growth_bytes_per_min > 1024u64.pow(2) as f64);
        assert!(leak.reason.contains("10 samples"));

        let mut flappy = ProcessTracker::new();
        let values = [10u64, 20, 15, 30, 25, 40, 35, 50, 45, 60];
        for (minute, value) in values.iter().enumerate() {
            flappy.record(9, sample(minute as f64 * 60.0, *value));
        }
        assert!(flappy.leak_indicator(9, 60).is_none(), "alternating drops fail the delta rule");
    }

    #[test]
    fn leak_indicator_requires_minimum_window() {
        let mut tracker = ProcessTracker::new();
        for minute in 0..4u64 {
            tracker.record(3, sample(minute as f64 * 60.0, minute * 1024u64.pow(2)));
        }
        assert!(tracker.leak_indicator(3, 4 * 1024u64.pow(2)).is_none(), "too few samples");
    }

    #[test]
    fn ranking_is_by_private_memory_with_annotations() {
        let tracker = growing_tracker();
        let current = vec![
            (1, "small".to_string(), ProcessReading { private_bytes: 100, working_set_bytes: 200, private_working_set_bytes: Some(50), peak_working_set_bytes: 300 }),
            (7, "leaky".to_string(), ProcessReading { private_bytes: 1024u64.pow(3), working_set_bytes: 2 * 1024u64.pow(3), private_working_set_bytes: Some(1024u64.pow(3)), peak_working_set_bytes: 2 * 1024u64.pow(3) }),
        ];
        let cpu = HashMap::from([(7u32, 12.5f32)]);
        let started = HashMap::from([(7u32, "2026-01-01T00:00:00Z".to_string())]);
        let ranked = analyze(&current, &tracker, 8 * 1024u64.pow(3), &cpu, &started);
        assert_eq!(ranked[0].name, "leaky");
        assert_eq!(ranked[0].cpu_percent, Some(12.5));
        assert_eq!(ranked[0].started_at.as_deref(), Some("2026-01-01T00:00:00Z"));
        assert!(ranked[0].leak.is_some());
        assert_eq!(ranked[0].trend_direction, TrendDirection::Rising);
        assert!(ranked[1].share_percent < ranked[0].share_percent);
    }

    #[test]
    fn history_is_bounded() {
        let mut tracker = ProcessTracker::new();
        for index in 0..300u64 {
            tracker.record(3, sample(index as f64, index));
        }
        assert!(tracker.samples(3).len() <= 96);
    }
}
