//! Advanced RAM analysis built on documented Windows APIs:
//! - `GlobalMemoryStatusEx` for physical totals and pagefile-backed capacity.
//! - `K32GetPerformanceInfo` (via the Phase 1 snapshot) for commit charge and system cache.
//! - PDH performance counters ("Memory" and "Paging File" objects) for standby,
//!   modified, compressed memory and fault rates.
//!
//! Cached and standby memory are reported separately and are never classified
//! as permanently unavailable memory.

use super::models::MemoryAnalysis;
use crate::monitoring::SystemSnapshot;

/// One PDH collection cycle over the counters FluxCore needs.
#[derive(Clone, Debug, Default)]
pub struct CounterReading {
    pub compressed_bytes: Option<u64>,
    pub standby_bytes: Option<u64>,
    pub modified_bytes: Option<u64>,
    pub cache_bytes: Option<u64>,
    pub free_zero_bytes: Option<u64>,
    pub page_reads_per_sec: Option<f32>,
    pub page_faults_per_sec: Option<f32>,
    pub paging_file_percent: Option<f32>,
    pub missing: Vec<String>,
}

/// Physical + pagefile accounting from `MEMORYSTATUSEX`.
#[derive(Clone, Copy, Debug, Default)]
pub struct MemoryStatus {
    pub memory_load_percent: Option<u32>,
    pub total_phys: Option<u64>,
    pub avail_phys: Option<u64>,
    pub total_pagefile: Option<u64>,
    pub avail_pagefile: Option<u64>,
}

#[cfg(windows)]
pub fn memory_status() -> MemoryStatus {
    use windows::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
    unsafe {
        let mut status = MEMORYSTATUSEX::default();
        status.dwLength = std::mem::size_of::<MEMORYSTATUSEX>() as u32;
        if GlobalMemoryStatusEx(&mut status).is_ok() {
            MemoryStatus {
                memory_load_percent: Some(status.dwMemoryLoad),
                total_phys: Some(status.ullTotalPhys),
                avail_phys: Some(status.ullAvailPhys),
                total_pagefile: Some(status.ullTotalPageFile),
                avail_pagefile: Some(status.ullAvailPageFile),
            }
        } else {
            MemoryStatus::default()
        }
    }
}

#[cfg(not(windows))]
pub fn memory_status() -> MemoryStatus {
    MemoryStatus::default()
}

#[cfg(windows)]
mod pdh {
    use super::CounterReading;
    use windows::core::w;
    use windows::Win32::System::Performance::{
        PdhAddEnglishCounterW, PdhCloseQuery, PdhCollectQueryData, PdhGetFormattedCounterValue,
        PdhOpenQueryW, PDH_FMT_COUNTERVALUE, PDH_FMT_DOUBLE, PDH_FMT_LARGE,
    };

    const ERROR_SUCCESS: u32 = 0;

    struct Counter {
        name: &'static str,
        handle: isize,
        format: u32,
    }

    /// Owns a PDH query. Counters that fail to register on this Windows
    /// build are reported as missing instead of failing the whole reading.
    pub struct CounterSet {
        query: isize,
        counters: Vec<Counter>,
        pub missing: Vec<String>,
    }

    impl CounterSet {
        pub fn open() -> Option<Self> {
            let mut query: isize = 0;
            unsafe {
                if PdhOpenQueryW(None, 0, &mut query) != ERROR_SUCCESS {
                    return None;
                }
            }
            // (counter path, PDH format): byte counters as 64-bit integers,
            // rates and percentages as doubles.
            let wanted: &[(&str, windows::core::PCWSTR, u32)] = &[
                ("compressed_bytes", w!(r"\Memory\Compressed Bytes"), PDH_FMT_LARGE.0),
                ("standby_reserve", w!(r"\Memory\Standby Cache Reserve Bytes"), PDH_FMT_LARGE.0),
                ("standby_normal", w!(r"\Memory\Standby Cache Normal Priority Bytes"), PDH_FMT_LARGE.0),
                ("standby_core", w!(r"\Memory\Standby Cache Core Bytes"), PDH_FMT_LARGE.0),
                ("modified_bytes", w!(r"\Memory\Modified Page List Bytes"), PDH_FMT_LARGE.0),
                ("cache_bytes", w!(r"\Memory\Cache Bytes"), PDH_FMT_LARGE.0),
                ("free_zero_bytes", w!(r"\Memory\Free & Zero Page List Bytes"), PDH_FMT_LARGE.0),
                ("page_reads_per_sec", w!(r"\Memory\Page Reads/sec"), PDH_FMT_DOUBLE.0),
                ("page_faults_per_sec", w!(r"\Memory\Page Faults/sec"), PDH_FMT_DOUBLE.0),
                ("paging_file_percent", w!(r"\Paging File(_Total)\% Usage"), PDH_FMT_DOUBLE.0),
            ];
            let mut counters = Vec::new();
            let mut missing = Vec::new();
            for (name, path, format) in wanted {
                let mut handle: isize = 0;
                let ok = unsafe { PdhAddEnglishCounterW(query, *path, 0, &mut handle) == ERROR_SUCCESS };
                if ok {
                    counters.push(Counter { name, handle, format: *format });
                } else {
                    missing.push((*name).to_string());
                }
            }
            Some(Self { query, counters, missing })
        }

        pub fn read(&mut self) -> CounterReading {
            let mut reading = CounterReading { missing: self.missing.clone(), ..Default::default() };
            if unsafe { PdhCollectQueryData(self.query) } != ERROR_SUCCESS {
                return reading;
            }
            let mut standby: Option<u64> = Some(0);
            for counter in &self.counters {
                let mut value = PDH_FMT_COUNTERVALUE::default();
                let ok = unsafe {
                    PdhGetFormattedCounterValue(
                        counter.handle,
                        windows::Win32::System::Performance::PDH_FMT(counter.format),
                        None,
                        &mut value,
                    ) == ERROR_SUCCESS
                };
                if !ok || value.CStatus != 0 {
                    continue;
                }
                let raw = unsafe {
                    if counter.format == PDH_FMT_LARGE.0 {
                        value.Anonymous.largeValue as f64
                    } else {
                        value.Anonymous.doubleValue
                    }
                };
                match counter.name {
                    "compressed_bytes" => reading.compressed_bytes = Some(raw.max(0.0) as u64),
                    "standby_reserve" | "standby_normal" | "standby_core" => {
                        standby = Some(standby.unwrap_or(0).saturating_add(raw.max(0.0) as u64))
                    }
                    "modified_bytes" => reading.modified_bytes = Some(raw.max(0.0) as u64),
                    "cache_bytes" => reading.cache_bytes = Some(raw.max(0.0) as u64),
                    "free_zero_bytes" => reading.free_zero_bytes = Some(raw.max(0.0) as u64),
                    "page_reads_per_sec" => reading.page_reads_per_sec = Some(raw.max(0.0) as f32),
                    "page_faults_per_sec" => reading.page_faults_per_sec = Some(raw.max(0.0) as f32),
                    "paging_file_percent" => reading.paging_file_percent = Some(raw.clamp(0.0, 100.0) as f32),
                    _ => {}
                }
            }
            reading.standby_bytes = standby.filter(|value| *value > 0);
            reading
        }
    }

    impl Drop for CounterSet {
        fn drop(&mut self) {
            unsafe { PdhCloseQuery(self.query) };
        }
    }
}

/// Handle to the platform counter source. On Windows this owns a PDH query;
/// elsewhere every reading is reported as unavailable.
pub struct WindowsMemoryCounters {
    #[cfg(windows)]
    inner: Option<pdh::CounterSet>,
    #[cfg(windows)]
    open_failed: bool,
}

impl WindowsMemoryCounters {
    pub fn new() -> Self {
        #[cfg(windows)]
        {
            let inner = pdh::CounterSet::open();
            let open_failed = inner.is_none();
            Self { inner, open_failed }
        }
        #[cfg(not(windows))]
        {
            Self {}
        }
    }

    pub fn read(&mut self) -> CounterReading {
        #[cfg(windows)]
        {
            match self.inner.as_mut() {
                Some(set) => set.read(),
                None => CounterReading {
                    missing: if self.open_failed {
                        vec!["pdh query unavailable".to_string()]
                    } else {
                        Vec::new()
                    },
                    ..Default::default()
                },
            }
        }
        #[cfg(not(windows))]
        {
            CounterReading {
                missing: vec!["PDH counters are Windows-only".to_string()],
                ..Default::default()
            }
        }
    }
}

impl Default for WindowsMemoryCounters {
    fn default() -> Self {
        Self::new()
    }
}

fn percent(part: u64, total: u64) -> f32 {
    if total == 0 {
        0.0
    } else {
        part as f32 * 100.0 / total as f32
    }
}

/// Combines the Phase 1 snapshot with the Phase 2 counter reading into one
/// audited memory analysis.
pub fn analyze(
    snapshot: &SystemSnapshot,
    reading: &CounterReading,
    status: &MemoryStatus,
    collected_at: &str,
) -> MemoryAnalysis {
    let total = snapshot.memory.total_bytes;
    let available = snapshot.memory.available_bytes;
    let used = snapshot.memory.used_bytes;
    let committed = snapshot.memory.committed_bytes;
    let commit_limit = snapshot.memory.commit_limit_bytes;

    let pagefile_total = status.total_pagefile.or(snapshot.memory.pagefile_total_bytes);
    let pagefile_used = match (status.total_pagefile, status.avail_pagefile) {
        (Some(total_pf), Some(avail_pf)) => Some(total_pf.saturating_sub(avail_pf)),
        _ => snapshot.memory.pagefile_used_bytes,
    };

    let mut sources = vec![
        "GlobalMemoryStatusEx (physical totals, pagefile-backed capacity)".to_string(),
        "K32GetPerformanceInfo (commit charge, commit limit, system cache)".to_string(),
    ];
    let mut limitations = Vec::new();

    let cached = snapshot.memory.cached_bytes.or(reading.cache_bytes);
    if reading.cache_bytes.is_some() || reading.standby_bytes.is_some() {
        sources.push("PDH Memory object (standby, modified, compressed, fault rates)".to_string());
    } else {
        limitations.push("PDH Memory counters unavailable; standby/modified/compressed memory not reported.".to_string());
    }
    if reading.paging_file_percent.is_some() {
        sources.push("PDH Paging File(_Total) % Usage".to_string());
    }
    if status.total_pagefile.is_none() {
        limitations.push("MEMORYSTATUSEX pagefile accounting unavailable on this system.".to_string());
    }
    limitations.extend(reading.missing.iter().map(|name| format!("PDH counter unavailable: {name}")));

    MemoryAnalysis {
        collected_at: collected_at.to_string(),
        total_bytes: total,
        used_bytes: used,
        available_bytes: available,
        free_bytes: reading.free_zero_bytes,
        usage_percent: percent(used, total),
        available_percent: percent(available, total),
        cached_bytes: cached,
        standby_bytes: reading.standby_bytes,
        modified_bytes: reading.modified_bytes,
        compressed_bytes: reading.compressed_bytes,
        committed_bytes: committed,
        commit_limit_bytes: commit_limit,
        commit_percent: match (committed, commit_limit) {
            (Some(commit), Some(limit)) if limit > 0 => Some(percent(commit, limit)),
            _ => None,
        },
        pagefile_used_bytes: pagefile_used,
        pagefile_total_bytes: pagefile_total,
        pagefile_percent: reading
            .paging_file_percent
            .or_else(|| match (pagefile_used, pagefile_total) {
                (Some(used_pf), Some(total_pf)) if total_pf > 0 => Some(percent(used_pf, total_pf)),
                _ => None,
            }),
        hard_faults_per_sec: reading.page_reads_per_sec,
        page_faults_per_sec: reading.page_faults_per_sec,
        memory_load_percent: status.memory_load_percent,
        sources,
        limitations,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::monitoring::{CpuMetrics, HardwareInfo, MemoryMetrics};

    fn snapshot(memory: MemoryMetrics) -> SystemSnapshot {
        SystemSnapshot {
            collected_at: "2026-01-01T00:00:00Z".into(),
            monitoring: true,
            cpu: CpuMetrics {
                name: "cpu".into(),
                vendor: "v".into(),
                architecture: None,
                physical_cores: Some(1),
                logical_processors: 2,
                usage_percent: Some(1.0),
                frequency_mhz: None,
                max_frequency_mhz: None,
                temperature_c: None,
                cores: vec![],
            },
            memory,
            gpus: vec![],
            storage: vec![],
            processes: vec![],
            hardware: HardwareInfo {
                os_name: "w".into(),
                os_version: "w".into(),
                host_name: "h".into(),
                gpus: vec![],
            },
            errors: vec![],
        }
    }

    fn memory() -> MemoryMetrics {
        MemoryMetrics {
            total_bytes: 32 * 1024u64.pow(3),
            used_bytes: 18 * 1024u64.pow(3),
            available_bytes: 14 * 1024u64.pow(3),
            free_bytes: None,
            usage_percent: Some(56.25),
            cached_bytes: Some(4 * 1024u64.pow(3)),
            committed_bytes: Some(20 * 1024u64.pow(3)),
            commit_limit_bytes: Some(40 * 1024u64.pow(3)),
            pagefile_used_bytes: None,
            pagefile_total_bytes: None,
        }
    }

    #[test]
    fn commit_percent_uses_commit_limit() {
        let analysis = analyze(&snapshot(memory()), &CounterReading::default(), &MemoryStatus::default(), "t");
        assert_eq!(analysis.commit_percent, Some(50.0));
    }

    #[test]
    fn pagefile_usage_derived_from_memory_status() {
        let status = MemoryStatus {
            memory_load_percent: Some(56),
            total_phys: Some(32 * 1024u64.pow(3)),
            avail_phys: Some(14 * 1024u64.pow(3)),
            total_pagefile: Some(48 * 1024u64.pow(3)),
            avail_pagefile: Some(40 * 1024u64.pow(3)),
        };
        let analysis = analyze(&snapshot(memory()), &CounterReading::default(), &status, "t");
        assert_eq!(analysis.pagefile_total_bytes, Some(48 * 1024u64.pow(3)));
        assert_eq!(analysis.pagefile_used_bytes, Some(8 * 1024u64.pow(3)));
        assert_eq!(analysis.memory_load_percent, Some(56));
    }

    #[test]
    fn standby_counters_are_summed_and_reported_separately() {
        let reading = CounterReading {
            standby_bytes: Some(3 * 1024u64.pow(3)),
            modified_bytes: Some(512 * 1024u64.pow(2)),
            compressed_bytes: Some(256 * 1024u64.pow(2)),
            ..Default::default()
        };
        let analysis = analyze(&snapshot(memory()), &reading, &MemoryStatus::default(), "t");
        assert_eq!(analysis.standby_bytes, Some(3 * 1024u64.pow(3)));
        assert_eq!(analysis.compressed_bytes, Some(256 * 1024u64.pow(2)));
        // Standby must not inflate "used" memory.
        assert_eq!(analysis.used_bytes, 18 * 1024u64.pow(3));
    }

    #[test]
    fn missing_counters_become_limitations_not_zeros() {
        let reading = CounterReading {
            missing: vec!["compressed_bytes".into()],
            ..Default::default()
        };
        let analysis = analyze(&snapshot(memory()), &reading, &MemoryStatus::default(), "t");
        assert!(analysis.compressed_bytes.is_none());
        assert!(analysis
            .limitations
            .iter()
            .any(|limitation| limitation.contains("compressed_bytes")));
    }
}
