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
