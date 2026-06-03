/// Lightweight statistical helpers shared by trend analysis, leak detection
/// and the prediction engine. Ordinary least squares only — no external
/// machine-learning dependencies, per the Phase 2 constraints.
#[derive(Clone, Copy, Debug, Default)]
pub struct Regression {
    pub slope: f64,
    pub intercept: f64,
    pub r2: f32,
    pub samples: u32,
}

/// Fits y = intercept + slope * x over the provided points.
/// Returns None when fewer than two distinct x values exist.
pub fn linear_regression(points: &[(f64, f64)]) -> Option<Regression> {
    let n = points.len();
    if n < 2 {
        return None;
    }
    let n_f = n as f64;
    let mut sum_x = 0.0;
    let mut sum_y = 0.0;
    for (x, y) in points {
        sum_x += x;
        sum_y += y;
    }
    let mean_x = sum_x / n_f;
    let mean_y = sum_y / n_f;
    let mut sxx = 0.0;
    let mut sxy = 0.0;
    let mut syy = 0.0;
    for (x, y) in points {
        let dx = x - mean_x;
        let dy = y - mean_y;
        sxx += dx * dx;
        sxy += dx * dy;
        syy += dy * dy;
    }
    if sxx <= f64::EPSILON {
        return None;
    }
    let slope = sxy / sxx;
    let intercept = mean_y - slope * mean_x;
    let r2 = if syy <= f64::EPSILON {
        // A perfectly flat series is fully explained by the fit.
        1.0
    } else {
        (sxy * sxy / (sxx * syy)).clamp(0.0, 1.0)
    };
    Some(Regression {
        slope,
        intercept,
        r2: r2 as f32,
        samples: n as u32,
    })
}

/// Classifies a slope (bytes per minute) against a noise floor so that
/// measurement jitter is reported as "stable" instead of a trend.
pub fn direction_of(slope: f64, noise_floor: f64) -> super::models::TrendDirection {
    use super::models::TrendDirection;
    if slope > noise_floor {
        TrendDirection::Rising
    } else if slope < -noise_floor {
        TrendDirection::Falling
    } else {
        TrendDirection::Stable
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn regression_recovers_exact_line() {
        let points: Vec<(f64, f64)> = (0..10).map(|i| (i as f64, 100.0 + 5.0 * i as f64)).collect();
        let fit = linear_regression(&points).expect("fit");
        assert!((fit.slope - 5.0).abs() < 1e-9);
        assert!((fit.intercept - 100.0).abs() < 1e-9);
        assert!((fit.r2 - 1.0).abs() < 1e-6);
    }

    #[test]
    fn regression_on_flat_series_is_stable() {
        let points: Vec<(f64, f64)> = (0..8).map(|i| (i as f64, 42.0)).collect();
        let fit = linear_regression(&points).expect("fit");
        assert!(fit.slope.abs() < 1e-9);
        assert_eq!(direction_of(fit.slope, 1.0), super::super::models::TrendDirection::Stable);
    }

    #[test]
    fn regression_needs_two_distinct_points() {
        assert!(linear_regression(&[(1.0, 2.0)]).is_none());
        assert!(linear_regression(&[(1.0, 2.0), (1.0, 3.0)]).is_none());
    }

    #[test]
    fn noisy_series_has_low_r2() {
        let points: Vec<(f64, f64)> = vec![
            (0.0, 0.0),
            (1.0, 10.0),
            (2.0, 1.0),
            (3.0, 11.0),
            (4.0, 2.0),
            (5.0, 12.0),
        ];
        let fit = linear_regression(&points).expect("fit");
        assert!(fit.r2 < 0.5, "alternating noise must not look like a trend, r2={}", fit.r2);
    }
}
