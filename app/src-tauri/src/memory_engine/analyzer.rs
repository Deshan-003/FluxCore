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
