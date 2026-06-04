//! Optimization recommendation generation.
//!
//! Every recommendation is derived from live measurements. Expected benefits
//! are qualitative — FluxCore never invents byte savings.

use super::models::{
    CompressionInfo, MemoryAnalysis, PressureLevel, PressureReport, ProcessMemoryInfo,
    Recommendation, RiskLevel, VirtualMemoryInfo,
};

pub const ACTION_CLEAR_CACHES: &str = "clear_fluxcore_caches";
pub const ACTION_RELEASE_RESOURCES: &str = "release_fluxcore_resources";
pub const ACTION_PAUSE_BACKGROUND: &str = "pause_background_activity";
pub const ACTION_REFRESH_DATA: &str = "refresh_stale_data";
pub const ACTION_GUIDANCE_CLOSE_APPS: &str = "guidance_close_apps";
pub const ACTION_GUIDANCE_STARTUP: &str = "guidance_startup_apps";
pub const ACTION_GUIDANCE_PAGEFILE: &str = "guidance_pagefile";

pub fn generate(
    pressure: &PressureReport,
    analysis: &MemoryAnalysis,
    virtual_memory: &VirtualMemoryInfo,
    processes: &[ProcessMemoryInfo],
    compression: &CompressionInfo,
) -> Vec<Recommendation> {
    let mut recommendations = Vec::new();

    if pressure.level.rank() >= PressureLevel::High.rank() {
        let heavy: Vec<String> = processes
            .iter()
            .take(3)
            .map(|process| format!("{} ({})", process.name, bytes_label(process.private_bytes)))
            .collect();
        recommendations.push(Recommendation {
            id: "review-heavy-apps".to_string(),
            title: "Review the heaviest memory consumers".to_string(),
            reason: format!(
                "Memory pressure is {} with {:.0}% available. Largest private memory consumers right now: {}.",
                pressure.level.as_str(),
                analysis.available_percent,
                if heavy.is_empty() { "unavailable".to_string() } else { heavy.join(", ") }
            ),
            expected_benefit: "Closing or restarting apps you are not using returns their committed memory to the system.".to_string(),
            risk: RiskLevel::None,
            category: "applications".to_string(),
            action_id: Some(ACTION_GUIDANCE_CLOSE_APPS.to_string()),
            requires_approval: false,
        });
    }

    if let Some(process) = processes.iter().find(|process| process.leak.is_some()) {
        let leak = process.leak.as_ref().expect("checked");
        recommendations.push(Recommendation {
            id: format!("leak-{}", process.pid),
            title: format!("{} shows a sustained memory growth pattern", process.name),
            reason: leak.reason.clone(),
            expected_benefit: "Restarting the application typically releases the accumulated private memory; the pattern may also be worth reporting to the vendor.".to_string(),
            risk: RiskLevel::None,
            category: "applications".to_string(),
            action_id: Some(ACTION_GUIDANCE_CLOSE_APPS.to_string()),
            requires_approval: false,
        });
    }

    if virtual_memory.commit_percent.map(|percent| percent >= 80.0).unwrap_or(false) {
        recommendations.push(Recommendation {
            id: "commit-headroom".to_string(),
            title: "Commit charge is approaching the commit limit".to_string(),
            reason: format!(
                "Commit charge is {} of the {} commit limit, leaving {} of headroom.",
                bytes_label(virtual_memory.commit_charge_bytes.unwrap_or(0)),
                bytes_label(virtual_memory.commit_limit_bytes.unwrap_or(0)),
                bytes_label(virtual_memory.available_commit_bytes.unwrap_or(0))
            ),
            expected_benefit: "Reviewing pagefile configuration (a manual Windows setting) can restore commit headroom; FluxCore only explains the steps.".to_string(),
            risk: RiskLevel::Low,
            category: "virtual-memory".to_string(),
            action_id: Some(ACTION_GUIDANCE_PAGEFILE.to_string()),
            requires_approval: false,
        });
    }

    if virtual_memory.pagefile_percent_usage.map(|percent| percent >= 80.0).unwrap_or(false) {
        recommendations.push(Recommendation {
            id: "pagefile-usage".to_string(),
            title: "Paging file usage is high".to_string(),
            reason: format!(
                "The paging file is {:.0}% used; combined with {} of commit charge this indicates sustained commit pressure.",
                virtual_memory.pagefile_percent_usage.unwrap_or(0.0),
                bytes_label(virtual_memory.commit_charge_bytes.unwrap_or(0))
            ),
            expected_benefit: "Reducing committed workload (closing unused apps) lowers paging; pagefile resizing remains a manual, user-approved Windows change.".to_string(),
            risk: RiskLevel::Low,
            category: "virtual-memory".to_string(),
            action_id: Some(ACTION_GUIDANCE_PAGEFILE.to_string()),
            requires_approval: false,
        });
    }

    if compression.available && pressure.level.rank() >= PressureLevel::Elevated.rank() {
        recommendations.push(Recommendation {
            id: "compression-active".to_string(),
            title: "Windows memory compression is absorbing pressure".to_string(),
            reason: compression.pressure_relationship.clone(),
            expected_benefit: "No action needed: Windows manages compression. Reducing memory pressure lowers the CPU time spent compressing.".to_string(),
            risk: RiskLevel::None,
            category: "compression".to_string(),
            action_id: None,
            requires_approval: false,
        });
    }

    if pressure.level.rank() >= PressureLevel::Elevated.rank() {
        recommendations.push(Recommendation {
            id: "startup-review".to_string(),
            title: "Review startup applications".to_string(),
            reason: format!(
                "Available memory is {:.0}%; background and startup applications reserve memory from boot.",
                analysis.available_percent
            ),
            expected_benefit: "Disabling unneeded startup entries (a manual Windows setting) reduces baseline memory consumption.".to_string(),
            risk: RiskLevel::None,
            category: "applications".to_string(),
            action_id: Some(ACTION_GUIDANCE_STARTUP.to_string()),
            requires_approval: false,
        });
        recommendations.push(Recommendation {
            id: "fluxcore-self-trim".to_string(),
            title: "Let FluxCore trim its own footprint".to_string(),
            reason: "Under memory pressure FluxCore can clear its internal caches and pause background scans; this releases only memory FluxCore itself holds.".to_string(),
            expected_benefit: "Returns FluxCore's own cached history buffers to the system; monitoring continues at reduced detail.".to_string(),
            risk: RiskLevel::Low,
            category: "fluxcore".to_string(),
            action_id: Some(ACTION_CLEAR_CACHES.to_string()),
            requires_approval: true,
        });
    }

    recommendations
}

fn bytes_label(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut value = bytes as f64;
    let mut index = 0;
    while value >= 1024.0 && index < UNITS.len() - 1 {
        value /= 1024.0;
        index += 1;
    }
    format!("{value:.1} {}", UNITS[index])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory_engine::models::{PressureSignals, TrendDirection};

    fn report(level: PressureLevel) -> PressureReport {
        PressureReport {
            level,
            level_since: "t".into(),
            reasons: vec![],
            signals: PressureSignals {
                available_percent: 10.0,
                commit_percent: None,
                pagefile_percent: None,
                hard_faults_per_sec: None,
                available_trend_bytes_per_min: 0.0,
            },
        }
    }

    fn analysis() -> MemoryAnalysis {
        MemoryAnalysis {
            collected_at: "t".into(),
            total_bytes: 32 * 1024u64.pow(3),
            used_bytes: 0,
            available_bytes: 0,
            free_bytes: None,
            usage_percent: 90.0,
            available_percent: 10.0,
            cached_bytes: None,
            standby_bytes: None,
            modified_bytes: None,
            compressed_bytes: None,
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

    fn vm(commit_percent: Option<f32>, pagefile_percent: Option<f32>) -> VirtualMemoryInfo {
        VirtualMemoryInfo {
            commit_charge_bytes: Some(36 * 1024u64.pow(3)),
            commit_limit_bytes: Some(40 * 1024u64.pow(3)),
            commit_percent,
            available_commit_bytes: Some(4 * 1024u64.pow(3)),
            pagefile_percent_usage: pagefile_percent,
            pagefile_config: vec![],
            notes: vec![],
            limitations: vec![],
        }
    }

    fn compression(available: bool) -> CompressionInfo {
        CompressionInfo {
            available,
            compressed_bytes: available.then_some(1024u64.pow(2)),
            compression_process_bytes: None,
            pressure_relationship: "test".to_string(),
            cpu_impact_note: "test".to_string(),
            limitation: None,
        }
    }

    fn leaking_process() -> ProcessMemoryInfo {
        ProcessMemoryInfo {
            pid: 42,
            name: "leaky.exe".to_string(),
            private_bytes: 2 * 1024u64.pow(3),
            working_set_bytes: 2 * 1024u64.pow(3),
            private_working_set_bytes: None,
            peak_working_set_bytes: 2 * 1024u64.pow(3),
            virtual_bytes: None,
            cpu_percent: None,
            share_percent: 6.0,
            trend_bytes_per_min: 50.0 * 1024.0 * 1024.0,
            trend_direction: TrendDirection::Rising,
            samples: 24,
            observation_seconds: 600,
            leak: Some(super::super::models::LeakIndicator {
                confidence: super::super::models::LeakConfidence::High,
                growth_bytes_per_min: 50.0 * 1024.0 * 1024.0,
                reason: "grew steadily".to_string(),
            }),
            started_at: None,
        }
    }

    #[test]
    fn normal_pressure_recommends_nothing() {
        let recommendations = generate(
            &report(PressureLevel::Normal),
            &analysis(),
            &vm(Some(40.0), Some(20.0)),
            &[],
            &compression(false),
        );
        assert!(recommendations.is_empty(), "healthy systems get no recommendations");
    }

    #[test]
    fn high_pressure_recommends_reviewing_apps() {
        let recommendations = generate(
            &report(PressureLevel::High),
            &analysis(),
            &vm(Some(40.0), Some(20.0)),
            &[leaking_process()],
            &compression(false),
        );
        assert!(recommendations.iter().any(|rec| rec.id == "review-heavy-apps"));
        assert!(recommendations.iter().any(|rec| rec.id == "leak-42"));
        // No recommendation may promise a fabricated byte saving.
        assert!(recommendations.iter().all(|rec| !rec.expected_benefit.contains("GB freed")));
    }

    #[test]
    fn commit_and_pagefile_pressure_recommend_guidance() {
        let recommendations = generate(
            &report(PressureLevel::Elevated),
            &analysis(),
            &vm(Some(90.0), Some(85.0)),
            &[],
            &compression(true),
        );
        assert!(recommendations.iter().any(|rec| rec.id == "commit-headroom"));
        assert!(recommendations.iter().any(|rec| rec.id == "pagefile-usage"));
        assert!(recommendations.iter().any(|rec| rec.id == "compression-active"));
        assert!(recommendations.iter().any(|rec| rec.id == "fluxcore-self-trim" && rec.requires_approval));
    }
}
