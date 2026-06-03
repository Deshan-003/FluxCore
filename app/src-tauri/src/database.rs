use rusqlite::{params, Connection, OptionalExtension};
use std::{fs, path::PathBuf, sync::Mutex};

use crate::memory_engine::models::{
    ActionStatus, AuditEntry, MemoryHistoryPoint, OptimizationMode, PolicyConfig, PressureEvent,
    PressureLevel, ProcessHistoryPoint, Prediction, RiskLevel, StoredRecommendation,
};
use crate::monitoring::{HistoryPoint, MonitoringSettings, SystemSnapshot};

pub struct Database {
    connection: Mutex<Connection>,
}

impl Database {
    pub fn open() -> Result<Self, String> {
        Self::open_at(database_path()?)
    }

    pub fn open_at_path(path: PathBuf) -> Result<Self, String> {
        Self::open_at(path)
    }

    fn open_at(path: PathBuf) -> Result<Self, String> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .map_err(|error| format!("create database directory: {error}"))?;
        }
        let connection =
            Connection::open(path).map_err(|error| format!("open database: {error}"))?;
        connection.execute_batch(
            "PRAGMA journal_mode=WAL;
             PRAGMA foreign_keys=ON;
             CREATE TABLE IF NOT EXISTS schema_migrations (version INTEGER PRIMARY KEY, applied_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP);
             CREATE TABLE IF NOT EXISTS settings (key TEXT PRIMARY KEY, value TEXT NOT NULL);
             CREATE TABLE IF NOT EXISTS hardware (id INTEGER PRIMARY KEY CHECK (id=1), payload TEXT NOT NULL, updated_at TEXT NOT NULL);
             CREATE TABLE IF NOT EXISTS performance_snapshots (
               id INTEGER PRIMARY KEY, collected_at TEXT NOT NULL,
               cpu_percent REAL NOT NULL, memory_percent REAL NOT NULL,
               gpu_percent REAL, vram_used_bytes INTEGER,
               disk_read_bytes_per_second INTEGER, disk_write_bytes_per_second INTEGER
             );
             CREATE INDEX IF NOT EXISTS idx_snapshots_collected_at ON performance_snapshots(collected_at);
             INSERT OR IGNORE INTO schema_migrations(version) VALUES (1);
             CREATE TABLE IF NOT EXISTS memory_snapshots (
               id INTEGER PRIMARY KEY, collected_at TEXT NOT NULL,
               used_bytes INTEGER NOT NULL, available_bytes INTEGER NOT NULL,
               committed_bytes INTEGER, compressed_bytes INTEGER, pagefile_used_bytes INTEGER,
               pressure_level TEXT NOT NULL
             );
             CREATE INDEX IF NOT EXISTS idx_memory_snapshots_at ON memory_snapshots(collected_at);
             CREATE TABLE IF NOT EXISTS memory_pressure_events (
               id INTEGER PRIMARY KEY, started_at TEXT NOT NULL, ended_at TEXT,
               level TEXT NOT NULL, reason TEXT NOT NULL,
               min_available_bytes INTEGER NOT NULL, peak_commit_percent REAL
             );
             CREATE TABLE IF NOT EXISTS process_memory_history (
               id INTEGER PRIMARY KEY, collected_at TEXT NOT NULL,
               pid INTEGER NOT NULL, name TEXT NOT NULL,
               private_bytes INTEGER NOT NULL, working_set_bytes INTEGER NOT NULL,
               trend_bytes_per_min REAL NOT NULL
             );
             CREATE INDEX IF NOT EXISTS idx_process_history_at ON process_memory_history(collected_at);
             CREATE TABLE IF NOT EXISTS optimization_recommendations (
               id INTEGER PRIMARY KEY, created_at TEXT NOT NULL,
               recommendation_id TEXT NOT NULL, title TEXT NOT NULL, reason TEXT NOT NULL,
               risk TEXT NOT NULL, category TEXT NOT NULL
             );
             CREATE TABLE IF NOT EXISTS optimization_actions (
               id INTEGER PRIMARY KEY, executed_at TEXT NOT NULL, mode TEXT NOT NULL,
               action_id TEXT NOT NULL, confirmed INTEGER NOT NULL,
               status TEXT NOT NULL, detail TEXT NOT NULL
             );
             CREATE TABLE IF NOT EXISTS optimization_policies (
               id INTEGER PRIMARY KEY CHECK (id=1), payload TEXT NOT NULL, updated_at TEXT NOT NULL
             );
             CREATE TABLE IF NOT EXISTS prediction_history (
               id INTEGER PRIMARY KEY, created_at TEXT NOT NULL,
               horizon_minutes INTEGER NOT NULL, available_slope REAL NOT NULL,
               commit_slope REAL, time_to_pressure_minutes REAL, confidence REAL NOT NULL
             );
             INSERT OR IGNORE INTO schema_migrations(version) VALUES (2);"
        ).map_err(|error| format!("apply database migration: {error}"))?;
        // Migration 3: snapshot percentages became nullable so a failed
        // collection is stored as NULL instead of a misleading zero. The
        // table is rebuilt in place; every existing row is copied over.
        let v3_applied: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM schema_migrations WHERE version = 3",
                [],
                |row| row.get(0),
            )
            .unwrap_or(0);
        if v3_applied == 0 {
            connection.execute_batch(
                "BEGIN;
                 CREATE TABLE performance_snapshots_v3 (
                   id INTEGER PRIMARY KEY, collected_at TEXT NOT NULL,
                   cpu_percent REAL, memory_percent REAL,
                   gpu_percent REAL, vram_used_bytes INTEGER,
                   disk_read_bytes_per_second INTEGER, disk_write_bytes_per_second INTEGER
                 );
                 INSERT INTO performance_snapshots_v3(id, collected_at, cpu_percent, memory_percent, gpu_percent, vram_used_bytes, disk_read_bytes_per_second, disk_write_bytes_per_second)
                   SELECT id, collected_at, cpu_percent, memory_percent, gpu_percent, vram_used_bytes, disk_read_bytes_per_second, disk_write_bytes_per_second FROM performance_snapshots;
                 DROP TABLE performance_snapshots;
                 ALTER TABLE performance_snapshots_v3 RENAME TO performance_snapshots;
                 CREATE INDEX IF NOT EXISTS idx_snapshots_collected_at ON performance_snapshots(collected_at);
                 INSERT OR IGNORE INTO schema_migrations(version) VALUES (3);
                 COMMIT;"
            ).map_err(|error| format!("apply database migration 3: {error}"))?;
        }
        Ok(Self {
            connection: Mutex::new(connection),
        })
    }

    pub fn load_settings(&self) -> Result<Option<MonitoringSettings>, String> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| "database lock poisoned".to_string())?;
        let value: Option<String> = connection
            .query_row(
                "SELECT value FROM settings WHERE key='preferences'",
                [],
                |row| row.get(0),
            )
            .optional()
            .map_err(|e| format!("read settings: {e}"))?;
        value
            .map(|json| serde_json::from_str(&json).map_err(|e| format!("decode settings: {e}")))
            .transpose()
    }

    pub fn save_settings(&self, settings: &MonitoringSettings) -> Result<(), String> {
        let json = serde_json::to_string(settings).map_err(|e| format!("encode settings: {e}"))?;
        let connection = self
            .connection
            .lock()
            .map_err(|_| "database lock poisoned".to_string())?;
        connection.execute("INSERT INTO settings(key,value) VALUES('preferences',?1) ON CONFLICT(key) DO UPDATE SET value=excluded.value", [json]).map_err(|e| format!("persist settings: {e}"))?;
        Ok(())
    }

    pub fn record_snapshot(&self, snapshot: &SystemSnapshot) -> Result<(), String> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| "database lock poisoned".to_string())?;
        let point = snapshot.history_point();
        connection.execute(
            "INSERT INTO performance_snapshots(collected_at,cpu_percent,memory_percent,gpu_percent,vram_used_bytes,disk_read_bytes_per_second,disk_write_bytes_per_second) VALUES(?1,?2,?3,?4,?5,?6,?7)",
            params![point.timestamp, point.cpu_percent, point.memory_percent, point.gpu_percent, point.vram_used_bytes, point.disk_read_bytes_per_second, point.disk_write_bytes_per_second]
        ).map_err(|e| format!("persist performance sample: {e}"))?;
        connection
            .execute(
                "DELETE FROM performance_snapshots WHERE collected_at < datetime('now','-7 days')",
                [],
            )
            .map_err(|e| format!("prune performance history: {e}"))?;
        connection.execute("INSERT INTO hardware(id,payload,updated_at) VALUES(1,?1,?2) ON CONFLICT(id) DO UPDATE SET payload=excluded.payload,updated_at=excluded.updated_at", params![serde_json::to_string(&snapshot.hardware).map_err(|e| format!("encode hardware info: {e}"))?, snapshot.collected_at]).map_err(|e| format!("persist hardware info: {e}"))?;
        Ok(())
    }

    pub fn history(&self) -> Result<Vec<HistoryPoint>, String> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| "database lock poisoned".to_string())?;
        let mut statement = connection.prepare("SELECT collected_at,cpu_percent,memory_percent,gpu_percent,vram_used_bytes,disk_read_bytes_per_second,disk_write_bytes_per_second FROM performance_snapshots ORDER BY id DESC LIMIT 3600").map_err(|e| format!("prepare history query: {e}"))?;
        let rows = statement
            .query_map([], |row| {
                Ok(HistoryPoint {
                    timestamp: row.get(0)?,
                    cpu_percent: row.get(1)?,
                    memory_percent: row.get(2)?,
                    gpu_percent: row.get(3)?,
                    vram_used_bytes: row.get(4)?,
                    disk_read_bytes_per_second: row.get(5)?,
                    disk_write_bytes_per_second: row.get(6)?,
                })
            })
            .map_err(|e| format!("read history: {e}"))?;
        let mut points = rows
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| format!("decode history: {e}"))?;
        points.reverse();
        Ok(points)
    }

    // ----- Phase 2: memory intelligence persistence -----

    pub fn record_memory_snapshot(&self, point: &MemoryHistoryPoint) -> Result<(), String> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| "database lock poisoned".to_string())?;
        connection
            .execute(
                "INSERT INTO memory_snapshots(collected_at,used_bytes,available_bytes,committed_bytes,compressed_bytes,pagefile_used_bytes,pressure_level) VALUES(?1,?2,?3,?4,?5,?6,?7)",
                params![
                    point.timestamp,
                    point.used_bytes as i64,
                    point.available_bytes as i64,
                    point.committed_bytes.map(|value| value as i64),
                    point.compressed_bytes.map(|value| value as i64),
                    point.pagefile_used_bytes.map(|value| value as i64),
                    point.pressure_level.as_str()
                ],
            )
            .map_err(|e| format!("persist memory snapshot: {e}"))?;
        Ok(())
    }

    pub fn memory_samples(&self, limit: usize) -> Result<Vec<MemoryHistoryPoint>, String> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| "database lock poisoned".to_string())?;
        let mut statement = connection
            .prepare(
                "SELECT collected_at,used_bytes,available_bytes,committed_bytes,compressed_bytes,pagefile_used_bytes,pressure_level FROM memory_snapshots ORDER BY id DESC LIMIT ?1",
            )
            .map_err(|e| format!("prepare memory history query: {e}"))?;
        let rows = statement
            .query_map([limit as i64], |row| {
                Ok((
                    MemoryHistoryPoint {
                        timestamp: row.get(0)?,
                        used_bytes: row.get::<_, i64>(1)? as u64,
                        available_bytes: row.get::<_, i64>(2)? as u64,
                        committed_bytes: row.get::<_, Option<i64>>(3)?.map(|value| value as u64),
                        compressed_bytes: row.get::<_, Option<i64>>(4)?.map(|value| value as u64),
                        pagefile_used_bytes: row.get::<_, Option<i64>>(5)?.map(|value| value as u64),
                        pressure_level: parse_level(&row.get::<_, String>(6)?),
                    },
                ))
            })
            .map_err(|e| format!("read memory history: {e}"))?;
        let mut points: Vec<MemoryHistoryPoint> = rows
            .map(|row| row.map(|(point,)| point))
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| format!("decode memory history: {e}"))?;
        points.reverse();
        Ok(points)
    }

    pub fn record_pressure_event(&self, event: &PressureEvent) -> Result<i64, String> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| "database lock poisoned".to_string())?;
        connection
            .execute(
                "INSERT INTO memory_pressure_events(started_at,ended_at,level,reason,min_available_bytes,peak_commit_percent) VALUES(?1,?2,?3,?4,?5,?6)",
                params![
                    event.started_at,
                    event.ended_at,
                    event.level.as_str(),
                    event.reason,
                    event.min_available_bytes as i64,
                    event.peak_commit_percent
                ],
            )
            .map_err(|e| format!("persist pressure event: {e}"))?;
        Ok(connection.last_insert_rowid())
    }

    pub fn close_pressure_event(&self, event: &PressureEvent) -> Result<(), String> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| "database lock poisoned".to_string())?;
        connection
            .execute(
                "UPDATE memory_pressure_events SET ended_at=?2, min_available_bytes=?3, peak_commit_percent=?4 WHERE id=?1",
                params![
                    event.id,
                    event.ended_at,
                    event.min_available_bytes as i64,
                    event.peak_commit_percent
                ],
            )
            .map_err(|e| format!("close pressure event: {e}"))?;
        Ok(())
    }

    pub fn pressure_events(&self, limit: usize) -> Result<Vec<PressureEvent>, String> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| "database lock poisoned".to_string())?;
        let mut statement = connection
            .prepare(
                "SELECT id,started_at,ended_at,level,reason,min_available_bytes,peak_commit_percent FROM memory_pressure_events ORDER BY id DESC LIMIT ?1",
            )
            .map_err(|e| format!("prepare pressure event query: {e}"))?;
        let rows = statement
            .query_map([limit as i64], |row| {
                Ok(PressureEvent {
                    id: row.get(0)?,
                    started_at: row.get(1)?,
                    ended_at: row.get(2)?,
                    level: parse_level(&row.get::<_, String>(3)?),
                    reason: row.get(4)?,
                    min_available_bytes: row.get::<_, i64>(5)? as u64,
                    peak_commit_percent: row.get(6)?,
                })
            })
            .map_err(|e| format!("read pressure events: {e}"))?;
        let mut events = rows
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| format!("decode pressure events: {e}"))?;
        events.reverse();
        Ok(events)
    }

    pub fn record_process_history(&self, points: &[ProcessHistoryPoint]) -> Result<(), String> {
        if points.is_empty() {
            return Ok(());
        }
        let mut connection = self
            .connection
            .lock()
            .map_err(|_| "database lock poisoned".to_string())?;
        let transaction = connection
            .transaction()
            .map_err(|e| format!("begin process history transaction: {e}"))?;
        for point in points {
            transaction
                .execute(
                    "INSERT INTO process_memory_history(collected_at,pid,name,private_bytes,working_set_bytes,trend_bytes_per_min) VALUES(?1,?2,?3,?4,?5,?6)",
                    params![
                        point.timestamp,
                        point.pid as i64,
                        point.name,
                        point.private_bytes as i64,
                        point.working_set_bytes as i64,
                        point.trend_bytes_per_min
                    ],
                )
                .map_err(|e| format!("persist process history: {e}"))?;
        }
        transaction
            .commit()
            .map_err(|e| format!("commit process history: {e}"))?;
        Ok(())
    }

    pub fn process_history(&self, limit: usize) -> Result<Vec<ProcessHistoryPoint>, String> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| "database lock poisoned".to_string())?;
        let mut statement = connection
            .prepare(
                "SELECT collected_at,pid,name,private_bytes,working_set_bytes,trend_bytes_per_min FROM process_memory_history ORDER BY id DESC LIMIT ?1",
            )
            .map_err(|e| format!("prepare process history query: {e}"))?;
        let rows = statement
            .query_map([limit as i64], |row| {
                Ok(ProcessHistoryPoint {
                    timestamp: row.get(0)?,
                    pid: row.get::<_, i64>(1)? as u32,
                    name: row.get(2)?,
                    private_bytes: row.get::<_, i64>(3)? as u64,
                    working_set_bytes: row.get::<_, i64>(4)? as u64,
                    trend_bytes_per_min: row.get(5)?,
                })
            })
            .map_err(|e| format!("read process history: {e}"))?;
        let mut points = rows
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| format!("decode process history: {e}"))?;
        points.reverse();
        Ok(points)
    }

    pub fn record_recommendation(&self, recommendation: &StoredRecommendation) -> Result<i64, String> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| "database lock poisoned".to_string())?;
        connection
            .execute(
                "INSERT INTO optimization_recommendations(created_at,recommendation_id,title,reason,risk,category) VALUES(?1,?2,?3,?4,?5,?6)",
                params![
                    recommendation.created_at,
                    recommendation.recommendation_id,
                    recommendation.title,
                    recommendation.reason,
                    serde_json::to_string(&recommendation.risk).map_err(|e| format!("encode risk: {e}"))?,
                    recommendation.category
                ],
            )
            .map_err(|e| format!("persist recommendation: {e}"))?;
        Ok(connection.last_insert_rowid())
    }

    pub fn stored_recommendations(&self, limit: usize) -> Result<Vec<StoredRecommendation>, String> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| "database lock poisoned".to_string())?;
        let mut statement = connection
            .prepare(
                "SELECT id,created_at,recommendation_id,title,reason,risk,category FROM optimization_recommendations ORDER BY id DESC LIMIT ?1",
            )
            .map_err(|e| format!("prepare recommendation query: {e}"))?;
        let rows = statement
            .query_map([limit as i64], |row| {
