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
