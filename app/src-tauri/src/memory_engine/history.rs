//! Optimization history: bounded in-memory rings plus the persistence
//! helpers that feed the SQLite tables created by migration 0002.

use super::models::{
    AuditEntry, MemoryAnalysis, MemoryHistoryPoint, PressureLevel, ProcessHistoryPoint,
    ProcessMemoryInfo, StoredRecommendation,
};

pub fn history_point(analysis: &MemoryAnalysis, level: PressureLevel) -> MemoryHistoryPoint {
    MemoryHistoryPoint {
        timestamp: analysis.collected_at.clone(),
        used_bytes: analysis.used_bytes,
        available_bytes: analysis.available_bytes,
        committed_bytes: analysis.committed_bytes,
        compressed_bytes: analysis.compressed_bytes,
        pagefile_used_bytes: analysis.pagefile_used_bytes,
        pressure_level: level,
    }
}

pub fn process_points(processes: &[ProcessMemoryInfo], timestamp: &str, limit: usize) -> Vec<ProcessHistoryPoint> {
    processes
        .iter()
        .take(limit)
        .map(|process| ProcessHistoryPoint {
            timestamp: timestamp.to_string(),
            pid: process.pid,
            name: process.name.clone(),
            private_bytes: process.private_bytes,
            working_set_bytes: process.working_set_bytes,
            trend_bytes_per_min: process.trend_bytes_per_min,
        })
        .collect()
}

pub fn stored_recommendation(recommendation: &super::models::Recommendation, created_at: &str) -> StoredRecommendation {
    StoredRecommendation {
        id: 0,
        created_at: created_at.to_string(),
        recommendation_id: recommendation.id.clone(),
        title: recommendation.title.clone(),
        reason: recommendation.reason.clone(),
        risk: recommendation.risk,
        category: recommendation.category.clone(),
    }
}

/// Bounded ring of audit entries kept for the Optimization Center.
pub struct AuditRing {
    entries: Vec<AuditEntry>,
    capacity: usize,
}

impl Default for AuditRing {
    fn default() -> Self {
        Self::new(200)
    }
}

impl AuditRing {
    pub fn new(capacity: usize) -> Self {
        Self { entries: Vec::new(), capacity }
    }

    pub fn push(&mut self, entry: AuditEntry) {
        self.entries.push(entry);
        if self.entries.len() > self.capacity {
            let overflow = self.entries.len() - self.capacity;
            self.entries.drain(..overflow);
        }
    }

    pub fn recent(&self, count: usize) -> Vec<AuditEntry> {
        self.entries.iter().rev().take(count).cloned().collect()
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn trim_to(&mut self, count: usize) {
        if self.entries.len() > count {
            let overflow = self.entries.len() - count;
            self.entries.drain(..overflow);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory_engine::models::{ActionStatus, OptimizationMode, TrendDirection};

    fn analysis() -> MemoryAnalysis {
        MemoryAnalysis {
            collected_at: "2026-01-01T00:00:00Z".into(),
            total_bytes: 32 * 1024u64.pow(3),
            used_bytes: 18 * 1024u64.pow(3),
            available_bytes: 14 * 1024u64.pow(3),
            free_bytes: None,
            usage_percent: 56.0,
            available_percent: 44.0,
            cached_bytes: None,
            standby_bytes: None,
            modified_bytes: None,
            compressed_bytes: Some(1),
            committed_bytes: Some(20 * 1024u64.pow(3)),
            commit_limit_bytes: Some(40 * 1024u64.pow(3)),
            commit_percent: Some(50.0),
            pagefile_used_bytes: Some(2 * 1024u64.pow(3)),
            pagefile_total_bytes: None,
            pagefile_percent: None,
            hard_faults_per_sec: None,
            page_faults_per_sec: None,
            memory_load_percent: None,
            sources: vec![],
            limitations: vec![],
        }
    }

    fn process(pid: u32, name: &str, private: u64) -> ProcessMemoryInfo {
        ProcessMemoryInfo {
            pid,
            name: name.to_string(),
            private_bytes: private,
            working_set_bytes: private,
            private_working_set_bytes: None,
            peak_working_set_bytes: private,
            virtual_bytes: None,
            cpu_percent: None,
            share_percent: 1.0,
            trend_bytes_per_min: 0.0,
            trend_direction: TrendDirection::Stable,
            samples: 1,
            observation_seconds: 60,
            leak: None,
            started_at: None,
        }
    }

    #[test]
    fn history_point_captures_the_displayed_series() {
        let point = history_point(&analysis(), PressureLevel::Elevated);
        assert_eq!(point.timestamp, "2026-01-01T00:00:00Z");
        assert_eq!(point.available_bytes, 14 * 1024u64.pow(3));
        assert_eq!(point.committed_bytes, Some(20 * 1024u64.pow(3)));
        assert_eq!(point.compressed_bytes, Some(1));
        assert_eq!(point.pressure_level, PressureLevel::Elevated);
    }

    #[test]
    fn process_points_are_capped() {
        let processes: Vec<ProcessMemoryInfo> = (0..30).map(|pid| process(pid, "p", pid as u64)).collect();
        let points = process_points(&processes, "t", 20);
        assert_eq!(points.len(), 20);
        assert_eq!(points[0].pid, 0);
    }

    #[test]
    fn audit_ring_stays_bounded_and_orders_newest_first() {
        let mut ring = AuditRing::new(5);
        for id in 0..10i64 {
            ring.push(AuditEntry {
                id,
                executed_at: "t".into(),
                mode: OptimizationMode::Manual,
                action_id: "a".into(),
                confirmed: true,
                status: ActionStatus::Success,
                detail: String::new(),
            });
        }
        assert_eq!(ring.len(), 5);
        let recent = ring.recent(3);
        assert_eq!(recent[0].id, 9);
        assert_eq!(recent[2].id, 7);
        ring.trim_to(2);
        assert_eq!(ring.len(), 2);
    }
}
