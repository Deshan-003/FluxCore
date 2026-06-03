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
