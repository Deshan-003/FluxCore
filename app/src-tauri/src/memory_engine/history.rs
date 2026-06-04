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
