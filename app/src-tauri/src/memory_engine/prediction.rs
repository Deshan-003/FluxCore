//! Lightweight memory pressure prediction.
//!
//! Ordinary least-squares trends over the retained system samples produce
//! forecasts for available memory and commit charge. Predictions are always
//! presented with their confidence, observation window and a disclaimer —
//! never as guaranteed outcomes.

use super::models::{Prediction, ProcessGrowth, ProcessMemoryInfo, SystemSample, TrendForecast};
use super::stats;
use std::collections::VecDeque;

/// Keeps the bounded sample window used for forecasting.
pub struct SampleWindow {
    samples: VecDeque<SystemSample>,
    max_seconds: f64,
    max_count: usize,
}

impl Default for SampleWindow {
    fn default() -> Self {
        Self::new(1800.0, 4096)
    }
}

impl SampleWindow {
    pub fn new(max_seconds: f64, max_count: usize) -> Self {
        Self { samples: VecDeque::new(), max_seconds, max_count }
    }

    pub fn record(&mut self, sample: SystemSample) {
        self.samples.push_back(sample);
        while self.samples.len() > self.max_count {
            self.samples.pop_front();
        }
        if let Some(newest) = self.samples.back() {
            let cutoff = newest.at_secs - self.max_seconds;
            while self.samples.front().map(|oldest| oldest.at_secs < cutoff).unwrap_or(false) {
                self.samples.pop_front();
            }
        }
    }

    pub fn samples(&self) -> &VecDeque<SystemSample> {
        &self.samples
    }

    pub fn drop_oldest(&mut self) {
        self.samples.pop_front();
    }

    fn forecast(&self, pick: impl Fn(&SystemSample) -> Option<f64>) -> TrendForecast {
        let points: Vec<(f64, f64)> = self
            .samples
            .iter()
            .filter_map(|sample| pick(sample).map(|value| (sample.at_secs, value)))
            .collect();
        match stats::linear_regression(&points) {
            Some(fit) => TrendForecast {
                slope_bytes_per_min: fit.slope * 60.0,
                current_bytes: points.last().map(|(_, value)| *value as u64),
                projected_bytes: None,
                r2: fit.r2,
                samples: fit.samples,
            },
            None => TrendForecast {
                slope_bytes_per_min: 0.0,
                current_bytes: points.last().map(|(_, value)| *value as u64),
                projected_bytes: None,
                r2: 0.0,
                samples: points.len() as u32,
            },
        }
    }

    pub fn available_forecast(&self) -> TrendForecast {
        self.forecast(|sample| Some(sample.available_bytes as f64))
    }

    pub fn commit_forecast(&self) -> TrendForecast {
        self.forecast(|sample| sample.committed_bytes.map(|value| value as f64))
    }

    pub fn observation_minutes(&self) -> f32 {
        match (self.samples.front(), self.samples.back()) {
            (Some(first), Some(last)) if last.at_secs > first.at_secs => {
                ((last.at_secs - first.at_secs) / 60.0) as f32
            }
            _ => 0.0,
        }
    }
}

fn project(forecast: &TrendForecast, horizon_minutes: u32) -> Option<f64> {
    forecast
        .current_bytes
        .map(|current| current as f64 + forecast.slope_bytes_per_min * f64::from(horizon_minutes))
}

/// Minutes until available memory crosses `threshold_bytes` at the current
/// slope, if the slope is meaningfully negative.
fn minutes_until_below(forecast: &TrendForecast, threshold_bytes: u64, noise_floor: f64) -> Option<f32> {
    let slope = forecast.slope_bytes_per_min;
    let current = forecast.current_bytes? as f64;
    if slope >= -noise_floor || current <= threshold_bytes as f64 {
        return None;
    }
    Some(((current - threshold_bytes as f64) / -slope) as f32)
}

pub fn predict(
    window: &SampleWindow,
    processes: &[ProcessMemoryInfo],
    total_bytes: u64,
    high_available_percent: f32,
    horizon_minutes: u32,
    generated_at: &str,
) -> Prediction {
    let mut available = window.available_forecast();
    let mut commit = window.commit_forecast();
    available.projected_bytes = project(&available, horizon_minutes);
    commit.projected_bytes = project(&commit, horizon_minutes);

    let noise_floor = total_bytes as f64 * 0.002; // 0.2% of RAM per minute
    let threshold_bytes = (total_bytes as f64 * high_available_percent as f64 / 100.0) as u64;
    let time_to_high_pressure_minutes = minutes_until_below(&available, threshold_bytes, noise_floor);

    let growing_processes: Vec<ProcessGrowth> = processes
        .iter()
        .filter(|process| process.leak.is_some() || process.trend_bytes_per_min > noise_floor.max(1024.0 * 1024.0))
        .map(|process| ProcessGrowth {
