//! FluxCore Phase 4 benchmark harness.
//!
//! Measures the cost the intelligence engine adds to the existing Phase 1/2
//! monitor loop, using the real OS collector:
//!   1. In-memory Phase 4 sample: analysis, forecasts, anomalies, findings.
//!   2. Full Phase 4 sample: the same work plus the SQLite transaction.
//!   3. Persistence delta: the storage-write share of a sample.
//!   4. Gated tick: the per-monitoring-tick cost inside the 15-second window.
//!   5. Process footprint and throwaway-database size.
//!
//! Every metric value is a live reading. Only the snapshot timestamp is shifted
//! by the sampling cadence so each iteration performs a full sample instead of
//! the early return. The benchmark never modifies system state; its database is
//! a temporary file that is removed on exit.
//!
//! Usage: cargo run --release --example intelligence_benchmark [iterations]

use std::sync::Arc;

use fluxcore_lib::database::Database;
use fluxcore_lib::intelligence::IntelligenceEngine;
use fluxcore_lib::memory_engine::process_analyzer::read_process;
use fluxcore_lib::memory_engine::MemoryEngine;
use fluxcore_lib::monitoring::benchmark_intelligence;

fn mib(bytes: u64) -> f64 {
    bytes as f64 / (1024.0 * 1024.0)
}

/// Resident/peak footprint from /proc on non-Windows platforms, in MiB.
#[cfg(not(windows))]
fn linux_footprint_mib() -> Option<(f64, f64)> {
    let status = std::fs::read_to_string("/proc/self/status").ok()?;
    let kb = |key: &str| {
        status
            .lines()
            .find(|line| line.starts_with(key))
            .and_then(|line| line.split_whitespace().nth(1))
            .and_then(|value| value.parse::<u64>().ok())
    };
    Some((kb("VmRSS:")? as f64 / 1024.0, kb("VmHWM:")? as f64 / 1024.0))
}

#[cfg(windows)]
fn linux_footprint_mib() -> Option<(f64, f64)> {
    None
}

fn main() {
    let iterations: u32 = std::env::args()
        .nth(1)
        .and_then(|value| value.parse().ok())
        .unwrap_or(20)
        .clamp(2, 40);

    println!("FluxCore Phase 4 benchmark — {iterations} sampled observations per pass");
    println!("--------------------------------------------------------------------------------");

    let db_path = std::env::temp_dir().join(format!(
        "fluxcore-intelligence-benchmark-{}.sqlite",
        std::process::id()
    ));
    let database = Arc::new(
        Database::open_at_path(db_path.clone()).expect("failed to open benchmark database"),
    );
    let memory = Arc::new(MemoryEngine::new(Arc::clone(&database)));
    memory.set_dashboard_active(true);
    let intelligence = IntelligenceEngine::new(Arc::clone(&database), Arc::clone(&memory));

    let result = benchmark_intelligence(iterations, &memory, &intelligence);

    println!(
        "[phase 1] collector pass            avg {:>9} μs",
        result.collect_us
    );
    println!(
        "[phase 2] memory engine tick        avg {:>9} μs",
        result.memory_tick_us
    );
    println!(
        "[phase 4] analysis-only sample      avg {:>9} μs   (no SQLite write)",
        result.analysis_us
    );
    println!(
        "[phase 4] full sample + persistence avg {:>9} μs",
        result.sample_us
    );
    println!(
        "[phase 4] persistence delta         avg {:>9} μs   ({:.1}% of a full sample)",
        result.persistence_us,
        if result.sample_us > 0 {
            result.persistence_us as f64 * 100.0 / result.sample_us as f64
        } else {
            0.0
        }
    );
    println!(
        "[phase 4] gated tick (<15 s window) avg {:>9} μs   (paid on every monitoring tick)",
        result.gate_us
    );

    let phase12 = result.collect_us.saturating_add(result.memory_tick_us);
    let overhead = if phase12 > 0 {
        result.sample_us as f64 * 100.0 / phase12 as f64
    } else {
        0.0
    };
    println!(
        "[combined] Phase 4 overhead         {overhead:>8.1}% of the Phase 1+2 pipeline per sample"
    );
    println!(
        "[combined] sample cadence           one full Phase 4 sample every 15 s; gated ticks otherwise"
    );

    match std::fs::metadata(&db_path) {
        Ok(metadata) => println!(
            "[database] throwaway file            {:>9.1} KiB after {} observations",
            metadata.len() as f64 / 1024.0,
            result.iterations * 2
        ),
        Err(error) => println!("[database] size unavailable: {error}"),
    }

    match read_process(std::process::id()) {
        Some(reading) => println!(
            "[footprint] fluxcore benchmark     private {:.1} MiB · working set {:.1} MiB · peak WS {:.1} MiB",
            mib(reading.private_bytes),
            mib(reading.working_set_bytes),
            mib(reading.peak_working_set_bytes)
        ),
        None => match linux_footprint_mib() {
            Some((rss, peak)) => println!(
                "[footprint] fluxcore benchmark     resident {:.1} MiB · peak resident {:.1} MiB (non-Windows platform)",
                rss, peak
            ),
            None => println!("[footprint] unavailable on this platform"),
        },
    }

    match intelligence.state() {
        Ok(state) => {
            println!(
                "[engine] serving live intelligence: health=\"{}\" forecasts={} anomalies={} recommendations={}",
                state.health,
                state.forecasts.len(),
                state.anomalies.len(),
                state.recommendations.len()
            );

            // Calibration sample. The run above is a controlled, idle workload:
            // no anomaly is injected, so any alert is a false positive. Forecasts
            // whose target time has passed carry the observed `actual` value.
            let mature: Vec<_> = state
                .forecasts
                .iter()
                .filter(|forecast| forecast.actual.is_some() && forecast.value.is_some())
                .collect();
            if mature.is_empty() {
                println!("[calibration] no matured forecasts in this run");
            } else {
                let count = mature.len() as f64;
                let mut abs_error = 0.0f64;
                let mut pct_error = 0.0f64;
                let mut covered = 0usize;
                let mut bounded = 0usize;
                for forecast in &mature {
                    let actual = forecast.actual.unwrap();
                    let predicted = forecast.value.unwrap();
                    abs_error += (actual - predicted).abs();
                    if actual.abs() > f64::EPSILON {
                        pct_error += ((actual - predicted).abs() / actual.abs()).min(10.0);
                    }
                    if let (Some(low), Some(high)) = (forecast.low, forecast.high) {
                        bounded += 1;
                        if actual >= low && actual <= high {
                            covered += 1;
                        }
                    }
                }
                println!(
                    "[calibration] matured forecasts  {}/{} ({} min horizon)",
                    mature.len(),
                    state.forecasts.len(),
                    mature.first().map_or(0, |forecast| forecast.horizon_minutes)
                );
                println!("[calibration] MAE               {:.3}", abs_error / count);
                println!("[calibration] MAPE (capped 10x) {:.1}%", pct_error / count * 100.0);
                if bounded > 0 {
                    println!(
                        "[calibration] interval coverage {}/{} ({:.1}%)",
                        covered,
                        bounded,
                        covered as f64 * 100.0 / bounded as f64
                    );
                }
            }
            let samples = u32::from(iterations) * 2;
            println!(
                "[calibration] anomalies on idle  {} over {} samples ({:.3} per sample)",
                state.anomalies.len(),
                samples,
                state.anomalies.len() as f64 / samples as f64
            );
        }
        Err(error) => println!("[engine] intelligence state unavailable: {error}"),
    }

    drop(intelligence);
    drop(memory);
    drop(database);
    let removed = [
        db_path.clone(),
        db_path.with_extension("sqlite-wal"),
        db_path.with_extension("sqlite-shm"),
    ]
    .iter()
    .map(|path| std::fs::remove_file(path).map(|_| 1).unwrap_or(0))
    .sum::<u32>();
    println!("--------------------------------------------------------------------------------");
    println!("benchmark database removed ({removed} file(s)); no system state was modified");
}
