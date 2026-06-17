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
    }
}

pub fn forecasts(history: &[Observation], metric: &str, horizons: &[i64]) -> Vec<Forecast> {
    let now = history.last().map_or(0, |p| p.at);
    let mut start = history.len().saturating_sub(1);
    while start > 0
        && now - history[start - 1].at <= 1800
        && history[start].at - history[start - 1].at <= MAX_GAP
    {
        start -= 1;
    }
    let window = &history[start..];
    let points: Vec<_> = window
        .iter()
        .filter_map(|p| p.values.get(metric).map(|v| ((p.at - now) as f64, *v)))
        .collect();
    let duration = points
        .first()
        .zip(points.last())
        .map_or(0, |(a, b)| (b.0 - a.0) as i64);
    let coverage = points.len() as f64 / window.len().max(1) as f64;
    let current = points.last().is_some_and(|p| p.0 == 0.0);
    let continuous = points.windows(2).all(|p| p[1].0 - p[0].0 <= MAX_GAP as f64);
    horizons.iter().map(|&horizon| {
        let seconds = horizon * 60;
        let mut result = Forecast { metric: metric.into(), observed_at: now, target_at: now + seconds,
            horizon_minutes: horizon, actual: None, samples: points.len(), observation_seconds: duration,
            coverage, confidence: "unavailable".into(), method: "none".into(), value: None,
            low: None, high: None, explanation: "Needs a current measurement and at least eight recent samples spanning two minutes, without large gaps.".into() };
        if !current || !continuous || points.len() < 8 || duration < 120 || coverage < 0.75 { return result; }
        let names = ["rollingMean", "exponentialSmoothing", "linearTrend"];
        let mut best = (f64::INFINITY, 0, 0);
        for kind in 0..3 {
            let mut error = 0.0;
            let mut count = 0;
            for end in (5..points.len()).rev().take(12) {
                // Score recent walk-forward targets at the requested lead or the largest supported lead.
                let lead = (seconds as f64).min(duration as f64 / 4.0);
                let train_end = points[..end].partition_point(|p| p.0 <= points[end].0 - lead);
                if train_end < 4 { continue; }
                let residual = model(&points[..train_end], kind, points[end].0) - points[end].1;
                error += residual * residual;
                count += 1;
            }
            if count > 0 && (error / f64::from(count)).sqrt() < best.0 { best = ((error / f64::from(count)).sqrt(), kind, count); }
        }
        if best.2 == 0 { return result; }
        let is_percent = matches!(metric, "cpu" | "ram" | "commit")
            || ["gpu:", "vram:", "disk:"]
                .iter()
                .any(|prefix| metric.starts_with(prefix));
        let clamp = |value: f64| if is_percent { value.clamp(0.0, 100.0) } else { value.max(0.0) };
        let raw = model(&points, best.1, seconds as f64);
        let value = clamp(raw);
        let extrapolation = 1.0 + seconds as f64 / duration as f64;
        let floor = if is_percent { 2.0 } else { 16_777_216.0 };
        let width = (best.0 * 2.0 + floor) * extrapolation;
        result.method = names[best.1].into();
        result.value = Some(value);
        result.low = Some(clamp(value - width));
        result.high = Some(clamp(value + width));
        result.confidence = if seconds > duration / 4 || best.2 < 6 || coverage < 0.9 || width > floor * 8.0 || raw != value { "low" } else { "moderate" }.into();
        result.explanation = "Selected by walk-forward error against recent observations. The range is a heuristic, not a calibrated probability; workload changes can invalidate it.".into();
        result
    }).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(at: i64, value: f64) -> Observation {
        Observation {
            at,
            values: [("cpu".into(), value)].into(),
            ..Default::default()
        }
    }

