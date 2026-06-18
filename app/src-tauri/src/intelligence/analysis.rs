use super::models::*;
use super::statistics::{baseline, MAX_GAP};
use crate::memory_engine::models::{MemoryIntelligenceSnapshot, PressureLevel};
use crate::monitoring::SystemSnapshot;
use std::collections::{BTreeMap, HashMap};

const MIB: f64 = 1_048_576.0;

pub fn observe(
    at: i64,
    snapshot: &SystemSnapshot,
    memory: Option<&MemoryIntelligenceSnapshot>,
    scans: ScanCounters,
) -> Observation {
    let mut row = Observation {
        at,
        scans,
        process_count: (!snapshot.processes.is_empty()).then_some(snapshot.processes.len()),
        ..Observation::default()
    };
    let mut set = |key: String, value: Option<f64>| {
        if let Some(value) = value.filter(|v| v.is_finite() && *v >= 0.0) {
            row.values.insert(key, value);
        }
    };
    set("cpu".into(), snapshot.cpu.usage_percent.map(f64::from));
    set(
        "cpuMHz".into(),
        snapshot.cpu.frequency_mhz.map(|v| v as f64),
    );
    set("cpuTemp".into(), snapshot.cpu.temperature_c.map(f64::from));
    set(
        "corePeak".into(),
        snapshot
            .cpu
            .cores
            .iter()
            .map(|c| f64::from(c.usage_percent))
            .reduce(f64::max),
    );
    set("ram".into(), snapshot.memory.usage_percent.map(f64::from));
    if snapshot.memory.total_bytes > 0 {
        set(
            "ramAvailable".into(),
            Some(snapshot.memory.available_bytes as f64),
        );
        set("ramTotal".into(), Some(snapshot.memory.total_bytes as f64));
        set("ramUsed".into(), Some(snapshot.memory.used_bytes as f64));
    }
    if let Some(memory) = memory.filter(|m| m.analysis.total_bytes > 0) {
        row.pressure = Some(memory.pressure.level.as_str().into());
        set(
            "commit".into(),
            memory.analysis.commit_percent.map(f64::from),
        );
        set(
            "commitBytes".into(),
            memory.analysis.committed_bytes.map(|v| v as f64),
        );
        set(
            "pageReads".into(),
            memory.analysis.hard_faults_per_sec.map(f64::from),
        );
        set(
            "pagefileBytes".into(),
            memory.analysis.pagefile_used_bytes.map(|v| v as f64),
        );
    }
    for (index, gpu) in snapshot.gpus.iter().take(8).enumerate() {
        let adapter = format!("{} #{}", gpu.name, index + 1);
        set(
            format!("gpu:{adapter}"),
            gpu.utilization_percent.map(f64::from),
        );
        set(
            format!("vram:{adapter}"),
            gpu.vram_usage_percent.map(f64::from),
        );
        set(
            format!("vramBytes:{adapter}"),
            gpu.dedicated_used_bytes.map(|v| v as f64),
        );
        set(
            format!("gpuShared:{adapter}"),
            gpu.shared_used_bytes.map(|v| v as f64),
        );
        set(
            format!("gpuTemp:{adapter}"),
            gpu.temperature_c.map(f64::from),
        );
    }
    for disk in snapshot.storage.iter().take(24) {
        set(
            format!("disk:{}", disk.name),
            disk.usage_percent.map(f64::from),
        );
        if disk.total_bytes > 0 {
            set(
                format!("diskFree:{}", disk.name),
                Some(disk.available_bytes as f64),
            );
        }
        set(
            format!("diskRead:{}", disk.name),
            disk.read_bytes_per_second.map(|v| v as f64),
        );
        set(
            format!("diskWrite:{}", disk.name),
            disk.write_bytes_per_second.map(|v| v as f64),
        );
    }
    if let Some(process) = snapshot
        .processes
        .iter()
        .find(|p| p.pid == std::process::id())
    {
        set("fluxCpu".into(), process.cpu_percent.map(f64::from));
        set("fluxRam".into(), Some(process.memory_bytes as f64));
    }
    row
}

pub fn forecast_metric(key: &str) -> bool {
    matches!(key, "cpu" | "ram" | "commit")
        || ["gpu:", "vram:", "vramBytes:", "disk:", "diskFree:"]
            .iter()
            .any(|prefix| key.starts_with(prefix))
}

fn thresholds(metric: &str) -> Option<(f64, f64, Severity)> {
    match metric.split(':').next()? {
        "cpu" | "gpu" => Some((90.0, 20.0, Severity::Medium)),
        "corePeak" => Some((97.0, 25.0, Severity::Low)),
        "ram" | "vram" => Some((92.0, 10.0, Severity::Medium)),
        "commit" => Some((90.0, 10.0, Severity::High)),
        "pageReads" => Some((f64::INFINITY, 20.0, Severity::Low)),
        "disk" => Some((95.0, 8.0, Severity::High)),
        "diskRead" | "diskWrite" => Some((f64::INFINITY, 16.0 * MIB, Severity::Information)),
        _ => None,
    }
}

#[derive(Clone, Default)]
struct Signal {
    last: i64,
    pending: Option<i64>,
    clear_since: Option<i64>,
    active: Option<Anomaly>,
}

#[derive(Default)]
pub struct Detector(HashMap<String, Signal>);

impl Detector {
    pub fn update(&mut self, history: &[Observation], current: &Observation) -> Vec<Anomaly> {
        let mut changes = Vec::new();
        self.0.retain(|key, signal| {
            if current.values.contains_key(key) && current.at - signal.last <= MAX_GAP {
                return true;
            }
            if let Some(mut event) = signal.active.take() {
                event.ended_at = Some(current.at);
                event.resolution = Some("Measurement interrupted; recovery not established".into());
                changes.push(event);
            }
            false
        });
        for (key, &value) in &current.values {
            let Some((absolute, delta, severity)) = thresholds(key) else {
                continue;
            };
            let signal = self.0.entry(key.clone()).or_default();
            signal.last = current.at;
            if let Some(event) = signal.active.as_ref() {
                if value < event.threshold - delta / 2.0 {
                    if current.at - *signal.clear_since.get_or_insert(current.at) >= 45 {
                        let mut event = signal.active.take().unwrap();
                        event.ended_at = Some(current.at);
                        event.resolution = Some("Recovered below the hysteresis threshold".into());
                        changes.push(event);
                        signal.pending = None;
                        signal.clear_since = None;
                    }
                } else {
                    signal.clear_since = None;
                }
                continue;
            }
            let relative = baseline(history, key, current.at)
                .map_or(f64::INFINITY, |(center, mad)| center + delta.max(4.0 * mad));
            let threshold = if absolute.is_finite() {
                absolute.min(relative)
            } else {
                relative
            };
            if value < threshold {
                signal.pending = None;
                continue;
            }
            let since = *signal.pending.get_or_insert(current.at);
            if current.at - since < 45 {
                continue;
            }
            let severity = if key == "commit" && value >= 98.0 {
                Severity::Critical
            } else if value < absolute {
                Severity::Low.min(severity)
            } else {
                severity
            };
            let event = Anomaly {
                id: format!("{key}@{}", current.at),
                metric: key.clone(),
                started_at: since,
                ended_at: None,
                resolution: None,
                severity,
                value,
                threshold,
                explanation: if relative < absolute {
                    "Sustained increase over the recent median/MAD baseline."
                } else {
                    "Sustained reading beyond a resource pressure threshold."
                }
                .into(),
            };
            signal.active = Some(event.clone());
            changes.push(event);
        }
        changes
    }

    pub fn active(&self) -> Vec<Anomaly> {
        let mut events: Vec<_> = self.0.values().filter_map(|s| s.active.clone()).collect();
        events.sort_by(|a, b| {
            b.severity
                .cmp(&a.severity)
                .then_with(|| a.metric.cmp(&b.metric))
        });
        events
    }
}

pub struct Findings {
    pub health: String,
    pub insights: Vec<String>,
    pub recommendations: Vec<Recommendation>,
    pub cpu_pressure: bool,
}

pub fn findings(
    row: &Observation,
    snapshot: &SystemSnapshot,
    memory: Option<&MemoryIntelligenceSnapshot>,
    events: &[Anomaly],
    forecasts: &[Forecast],
    candidate: Option<u64>,
) -> Findings {
    let mut insights = Vec::new();
    let mut recommendations = Vec::new();
    let mut suggest = |key: &str,
                       resource: &str,
                       severity: Severity,
                       problem: &str,
                       evidence: Vec<String>,
                       action: &str,
                       benefit: &str,
                       risk: &str,
                       applicable: bool| {
        recommendations.push(Recommendation {
            id: key.into(),
            created_at: row.at,
            resource: resource.into(),
            problem: problem.into(),
            evidence,
            severity,
