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
    Some((center, 1.4826 * median(&mut values)))
}

fn model(points: &[(f64, f64)], kind: usize, at: f64) -> f64 {
    match kind {
        0 => {
            let last = &points[points.len().saturating_sub(5)..];
            last.iter().map(|p| p.1).sum::<f64>() / last.len() as f64
        }
        1 => points.windows(2).fold(points[0].1, |previous, pair| {
            let alpha = 1.0 - (-(pair[1].0 - pair[0].0) / 45.0).exp();
            previous + alpha * (pair[1].1 - previous)
        }),
        _ => {
            let (sx, sy) = points
                .iter()
                .fold((0.0, 0.0), |(sx, sy), (x, y)| (sx + x, sy + y));
            let (mx, my) = (sx / points.len() as f64, sy / points.len() as f64);
            let denom: f64 = points.iter().map(|(x, _)| (x - mx).powi(2)).sum();
            let slope = if denom > 0.0 {
                points.iter().map(|(x, y)| (x - mx) * (y - my)).sum::<f64>() / denom
            } else {
                0.0
            };
            my + slope * (at - mx)
        }
