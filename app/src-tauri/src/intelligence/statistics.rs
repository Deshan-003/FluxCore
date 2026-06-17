use super::models::{Forecast, Observation};

pub const MAX_GAP: i64 = 90;

pub fn median(values: &mut [f64]) -> f64 {
    values.sort_unstable_by(f64::total_cmp);
    let mid = values.len() / 2;
    if values.len() % 2 == 0 {
        (values[mid - 1] + values[mid]) / 2.0
    } else {
        values[mid]
    }
}

pub fn baseline(history: &[Observation], metric: &str, end: i64) -> Option<(f64, f64)> {
    let mut values: Vec<_> = history
        .iter()
        .filter(|p| (end - 1800..end - 90).contains(&p.at))
        .filter_map(|p| p.values.get(metric).copied())
        .collect();
    if values.len() < 8 {
        return None;
    }
    let center = median(&mut values);
    for value in &mut values {
        *value = (*value - center).abs();
    }
