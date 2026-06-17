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
