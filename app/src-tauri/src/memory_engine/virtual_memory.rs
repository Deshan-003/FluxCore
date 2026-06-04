//! Virtual memory and pagefile analysis.
//!
//! Commit charge and commit limit come from the Phase 1 snapshot
//! (`K32GetPerformanceInfo`); pagefile usage comes from `MEMORYSTATUSEX` and
//! the PDH paging-file counter. The configured pagefile layout is read from
//! the documented registry value `Memory Management\PagingFiles` — read-only,
//! never modified.

use super::models::{MemoryAnalysis, PagefileConfigEntry, VirtualMemoryInfo};

#[cfg(windows)]
pub fn pagefile_config() -> Vec<PagefileConfigEntry> {
    use windows::core::w;
    use windows::Win32::System::Registry::{
        RegCloseKey, RegOpenKeyExW, RegQueryValueExW, HKEY, HKEY_LOCAL_MACHINE, KEY_READ,
    };
    let mut key = HKEY::default();
    let opened = unsafe {
        RegOpenKeyExW(
            HKEY_LOCAL_MACHINE,
            w!(r"SYSTEM\CurrentControlSet\Control\Session Manager\Memory Management"),
            0,
            KEY_READ,
            &mut key,
        )
    };
    if opened.is_err() {
        return Vec::new();
    }
    let mut size = 0u32;
    let queried = unsafe {
        RegQueryValueExW(key, w!("PagingFiles"), None, None, None, Some(&mut size))
    };
    let mut entries = Vec::new();
    if queried.is_ok() && size > 0 {
        let mut buffer = vec![0u8; size as usize];
        let read = unsafe {
            RegQueryValueExW(
                key,
                w!("PagingFiles"),
                None,
                None,
                Some(buffer.as_mut_ptr()),
                Some(&mut size),
            )
        };
        if read.is_ok() {
            // REG_MULTI_SZ: NUL-separated UTF-16 strings, double-NUL terminated.
            let wide: Vec<u16> = buffer
                .chunks_exact(2)
                .map(|chunk| u16::from_le_bytes([chunk[0], chunk[1]]))
                .collect();
            let mut start = 0;
            for (index, unit) in wide.iter().enumerate() {
                if *unit == 0 {
                    let slice = &wide[start..index];
                    if !slice.is_empty() {
                        if let Some(entry) = parse_paging_file_entry(&String::from_utf16_lossy(slice)) {
                            entries.push(entry);
                        }
                    }
                    start = index + 1;
                }
            }
        }
    }
    unsafe {
        let _ = RegCloseKey(key);
    }
    entries
}

/// Parses one `PagingFiles` entry such as `C:\pagefile.sys 1024 2048` or
/// `D:\pagefile.sys 0 0` (system managed).
pub fn parse_paging_file_entry(raw: &str) -> Option<PagefileConfigEntry> {
    let mut parts = raw.split_whitespace();
    let path = parts.next()?.to_string();
    let initial = parts.next();
    let maximum = parts.next();
    let (initial_mb, maximum_mb) = match (initial, maximum) {
        (Some(i), Some(m)) => (i.parse::<u64>().ok(), m.parse::<u64>().ok()),
        _ => (None, None),
    };
    let system_managed = initial_mb == Some(0) && maximum_mb == Some(0);
    Some(PagefileConfigEntry {
        path,
        system_managed,
        initial_size_mb: initial_mb,
        maximum_size_mb: maximum_mb,
    })
}

#[cfg(not(windows))]
pub fn pagefile_config() -> Vec<PagefileConfigEntry> {
    Vec::new()
}

pub fn analyze(analysis: &MemoryAnalysis, config: Vec<PagefileConfigEntry>) -> VirtualMemoryInfo {
    let mut notes = Vec::new();
    let mut limitations = Vec::new();

    let available_commit = match (analysis.committed_bytes, analysis.commit_limit_bytes) {
        (Some(committed), Some(limit)) => Some(limit.saturating_sub(committed)),
        _ => {
            limitations.push("Commit charge counters unavailable; commit headroom not reported.".to_string());
            None
        }
    };

    if let Some(commit_percent) = analysis.commit_percent {
        if commit_percent >= 85.0 {
            notes.push(format!(
                "Commit charge is at {commit_percent:.0}% of the commit limit; new allocations may fail before physical RAM is exhausted."
            ));
        }
    }
    if let Some(pagefile_percent) = analysis.pagefile_percent {
        if pagefile_percent >= 80.0 {
            notes.push(format!(
                "Paging file usage is at {pagefile_percent:.0}%; sustained paging at this level usually indicates commit pressure."
            ));
        }
    }
    if config.is_empty() {
        limitations.push("Pagefile configuration could not be read from the registry.".to_string());
    }

    VirtualMemoryInfo {
        commit_charge_bytes: analysis.committed_bytes,
        commit_limit_bytes: analysis.commit_limit_bytes,
        commit_percent: analysis.commit_percent,
        available_commit_bytes: available_commit,
        pagefile_percent_usage: analysis.pagefile_percent,
        pagefile_config: config,
        notes,
        limitations,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn analysis(commit_percent: Option<f32>, pagefile_percent: Option<f32>) -> MemoryAnalysis {
        MemoryAnalysis {
            collected_at: "t".into(),
            total_bytes: 32 * 1024u64.pow(3),
            used_bytes: 0,
            available_bytes: 0,
            free_bytes: None,
            usage_percent: 0.0,
            available_percent: 100.0,
            cached_bytes: None,
            standby_bytes: None,
            modified_bytes: None,
            compressed_bytes: None,
            committed_bytes: Some(34 * 1024u64.pow(3)),
            commit_limit_bytes: Some(40 * 1024u64.pow(3)),
            commit_percent,
            pagefile_used_bytes: None,
            pagefile_total_bytes: None,
            pagefile_percent,
            hard_faults_per_sec: None,
            page_faults_per_sec: None,
            memory_load_percent: None,
            sources: vec![],
            limitations: vec![],
        }
    }

    #[test]
    fn paging_file_entries_parse() {
        let managed = parse_paging_file_entry("C:\\pagefile.sys 0 0").expect("system managed entry");
        assert!(managed.system_managed);
        assert_eq!(managed.path, "C:\\pagefile.sys");
        let fixed = parse_paging_file_entry("D:\\pagefile.sys 1024 2048").expect("fixed entry");
        assert!(!fixed.system_managed);
        assert_eq!(fixed.initial_size_mb, Some(1024));
        assert_eq!(fixed.maximum_size_mb, Some(2048));
        assert!(parse_paging_file_entry("").is_none());
    }

    #[test]
    fn commit_headroom_is_limit_minus_charge() {
        let info = analyze(&analysis(Some(85.0), None), Vec::new());
        assert_eq!(info.available_commit_bytes, Some(6 * 1024u64.pow(3)));
        assert!(info.notes.iter().any(|note| note.contains("85%")));
    }

    #[test]
    fn high_pagefile_usage_is_flagged() {
        let info = analyze(&analysis(Some(50.0), Some(90.0)), Vec::new());
        assert!(info.notes.iter().any(|note| note.contains("90%")));
    }

    #[test]
    fn missing_config_is_a_limitation() {
        let info = analyze(&analysis(None, None), Vec::new());
        assert!(info.limitations.iter().any(|limitation| limitation.contains("Pagefile configuration")));
    }
}
