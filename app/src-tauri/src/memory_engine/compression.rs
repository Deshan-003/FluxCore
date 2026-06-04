//! Windows memory compression monitoring.
//!
//! Windows already manages memory compression; FluxCore only observes it and
//! explains its relationship to memory pressure. No competing compression
//! system is created and compression is never disabled.

use super::models::{CompressionInfo, MemoryAnalysis, ProcessMemoryInfo};

pub fn analyze(analysis: &MemoryAnalysis, processes: &[ProcessMemoryInfo]) -> CompressionInfo {
    let compression_process = processes
        .iter()
        .find(|process| process.name.eq_ignore_ascii_case("Memory Compression"))
        .map(|process| process.working_set_bytes);

    match analysis.compressed_bytes {
        Some(compressed) => {
            let pressure_relationship = if analysis.available_percent <= 25.0 {
                format!(
                    "Compression is holding {} MiB while available memory is {:.0}%: Windows is actively compressing to delay paging.",
                    compressed / 1024u64.pow(2),
                    analysis.available_percent
                )
            } else {
                format!(
                    "Compression holds {} MiB; with {:.0}% available memory it is routine background activity, not a pressure response.",
                    compressed / 1024u64.pow(2),
                    analysis.available_percent
                )
            };
            CompressionInfo {
                available: true,
                compressed_bytes: Some(compressed),
                compression_process_bytes: compression_process,
                pressure_relationship,
                cpu_impact_note: "Compressing and decompressing pages costs CPU time; under sustained pressure this can compete with foreground workloads."
                    .to_string(),
                limitation: None,
            }
        }
        None => CompressionInfo {
            available: false,
            compressed_bytes: None,
            compression_process_bytes: compression_process,
            pressure_relationship: "Compressed-memory size is not exposed by this Windows build; only the compression process working set is shown when visible."
                .to_string(),
            cpu_impact_note: "Windows manages memory compression; FluxCore does not modify it.".to_string(),
            limitation: Some("PDH \\Memory\\Compressed Bytes unavailable on this system.".to_string()),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory_engine::models::ProcessMemoryInfo;
    use crate::memory_engine::models::TrendDirection;

    fn analysis(compressed: Option<u64>, available_percent: f32) -> MemoryAnalysis {
        MemoryAnalysis {
            collected_at: "t".into(),
            total_bytes: 32 * 1024u64.pow(3),
            used_bytes: 0,
            available_bytes: 0,
            free_bytes: None,
            usage_percent: 100.0 - available_percent,
            available_percent,
            cached_bytes: None,
            standby_bytes: None,
            modified_bytes: None,
            compressed_bytes: compressed,
            committed_bytes: None,
            commit_limit_bytes: None,
            commit_percent: None,
            pagefile_used_bytes: None,
            pagefile_total_bytes: None,
            pagefile_percent: None,
            hard_faults_per_sec: None,
            page_faults_per_sec: None,
            memory_load_percent: None,
            sources: vec![],
            limitations: vec![],
        }
    }

    fn process(name: &str, working_set: u64) -> ProcessMemoryInfo {
        ProcessMemoryInfo {
            pid: 1,
            name: name.to_string(),
            private_bytes: working_set / 2,
            working_set_bytes: working_set,
            private_working_set_bytes: None,
            peak_working_set_bytes: working_set,
            virtual_bytes: None,
            cpu_percent: None,
            share_percent: 1.0,
            trend_bytes_per_min: 0.0,
            trend_direction: TrendDirection::Stable,
            samples: 0,
            observation_seconds: 0,
            leak: None,
            started_at: None,
        }
    }

    #[test]
    fn compression_is_reported_with_pressure_context() {
        let info = analyze(&analysis(Some(512 * 1024u64.pow(2)), 12.0), &[]);
        assert!(info.available);
        assert_eq!(info.compressed_bytes, Some(512 * 1024u64.pow(2)));
        assert!(info.pressure_relationship.contains("actively compressing"));
        assert!(info.cpu_impact_note.contains("CPU"));
    }

    #[test]
    fn compression_process_working_set_is_attached() {
        let info = analyze(
            &analysis(Some(1), 60.0),
            &[process("Memory Compression", 300 * 1024u64.pow(2))],
        );
        assert_eq!(info.compression_process_bytes, Some(300 * 1024u64.pow(2)));
        assert!(info.pressure_relationship.contains("routine background activity"));
    }

    #[test]
    fn missing_counter_is_a_limitation_not_zero() {
        let info = analyze(&analysis(None, 50.0), &[]);
        assert!(!info.available);
        assert!(info.compressed_bytes.is_none());
        assert!(info.limitation.is_some());
    }
}
