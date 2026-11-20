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

    /// Phase 3: lets `gpu_engine::persistence` run its own SQL against the same
    /// mutex-guarded connection without duplicating lock handling. The closure is
    /// kept short: GPU writes happen on task completion, never on the monitor tick.
    pub fn with_connection<T>(
        &self,
        f: impl FnOnce(&Connection) -> rusqlite::Result<T>,
    ) -> Result<T, String> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| "database lock poisoned".to_string())?;
        f(&connection).map_err(|error| format!("gpu database operation: {error}"))
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
        // Migration 4: Phase 3 GPU acceleration tables. Additive only; no
        // existing table is touched. The authoritative schema copy also lives
        // in migrations/0004_gpu_acceleration.sql for reference.
        let v4_applied: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM schema_migrations WHERE version = 4",
                [],
                |row| row.get(0),
            )
            .unwrap_or(0);
        if v4_applied == 0 {
            connection.execute_batch(
                "BEGIN;
                 CREATE TABLE IF NOT EXISTS gpu_providers (
                   provider_id TEXT PRIMARY KEY,
                   label TEXT NOT NULL,
                   kind TEXT NOT NULL,
                   available INTEGER NOT NULL,
                   compute_ready INTEGER NOT NULL,
                   detail TEXT NOT NULL,
                   device_count INTEGER NOT NULL,
                   capabilities TEXT NOT NULL,
                   last_seen TEXT NOT NULL
                 );
                 CREATE TABLE IF NOT EXISTS gpu_provider_cache (
                   id INTEGER PRIMARY KEY CHECK (id = 1),
                   payload TEXT NOT NULL,
                   updated_at TEXT NOT NULL
                 );
                 CREATE TABLE IF NOT EXISTS gpu_acceleration_settings (
                   id INTEGER PRIMARY KEY CHECK (id = 1),
                   mode TEXT NOT NULL,
                   payload TEXT NOT NULL,
                   updated_at TEXT NOT NULL
                 );
                 CREATE TABLE IF NOT EXISTS gpu_workload_settings (
                   workload_id TEXT PRIMARY KEY,
                   mode TEXT NOT NULL,
                   backend TEXT NOT NULL,
                   enabled INTEGER NOT NULL,
                   selected_size INTEGER NOT NULL,
                   updated_at TEXT NOT NULL
                 );
                 CREATE TABLE IF NOT EXISTS gpu_task_history (
                   id INTEGER PRIMARY KEY,
                   task_ref TEXT,
                   workload_id TEXT NOT NULL,
                   backend TEXT NOT NULL,
                   provider_id TEXT NOT NULL,
                   device_id TEXT NOT NULL,
                   status TEXT NOT NULL,
                   priority TEXT NOT NULL,
                   queued_at TEXT NOT NULL,
                   started_at TEXT,
                   finished_at TEXT,
                   duration_ms INTEGER,
                   queue_ms INTEGER,
                   cpu_time_ms REAL,
                   gpu_time_ms REAL,
                   transfer_time_ms REAL,
                   total_time_ms REAL,
                   speedup REAL,
                   vram_used_bytes INTEGER,
                   max_abs_error REAL,
                   validated INTEGER NOT NULL,
                   fell_back INTEGER NOT NULL,
                   error TEXT,
                   warnings TEXT NOT NULL,
                   payload TEXT NOT NULL
                 );
                 CREATE INDEX IF NOT EXISTS idx_gpu_task_history_finished ON gpu_task_history(finished_at);
                 CREATE INDEX IF NOT EXISTS idx_gpu_task_history_workload ON gpu_task_history(workload_id, finished_at);
                 CREATE TABLE IF NOT EXISTS gpu_benchmark_results (
                   id INTEGER PRIMARY KEY,
                   created_at TEXT NOT NULL,
                   workload_id TEXT NOT NULL,
                   params TEXT NOT NULL,
                   backend TEXT NOT NULL,
                   provider_id TEXT NOT NULL,
                   device_id TEXT NOT NULL,
                   cpu TEXT NOT NULL,
                   gpu TEXT,
                   transfer_ms REAL,
                   speedup REAL,
                   vram_used_bytes INTEGER,
                   validated INTEGER NOT NULL,
                   max_abs_error REAL,
                   verdict TEXT NOT NULL,
                   environment TEXT NOT NULL,
                   payload TEXT NOT NULL
                 );
                 CREATE INDEX IF NOT EXISTS idx_gpu_benchmark_created ON gpu_benchmark_results(created_at);
                 CREATE INDEX IF NOT EXISTS idx_gpu_benchmark_workload ON gpu_benchmark_results(workload_id, created_at);
                 INSERT OR IGNORE INTO schema_migrations(version) VALUES (4);
                 COMMIT;"
            ).map_err(|error| format!("apply database migration 4: {error}"))?;
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
                Ok(StoredRecommendation {
                    id: row.get(0)?,
                    created_at: row.get(1)?,
                    recommendation_id: row.get(2)?,
                    title: row.get(3)?,
                    reason: row.get(4)?,
                    risk: parse_risk(&row.get::<_, String>(5)?),
                    category: row.get(6)?,
                })
            })
            .map_err(|e| format!("read recommendations: {e}"))?;
        let mut recommendations = rows
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| format!("decode recommendations: {e}"))?;
        recommendations.reverse();
        Ok(recommendations)
    }

    pub fn record_action(&self, entry: &AuditEntry) -> Result<i64, String> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| "database lock poisoned".to_string())?;
        connection
            .execute(
                "INSERT INTO optimization_actions(executed_at,mode,action_id,confirmed,status,detail) VALUES(?1,?2,?3,?4,?5,?6)",
                params![
                    entry.executed_at,
                    encode_enum(&entry.mode),
                    entry.action_id,
                    i64::from(entry.confirmed),
                    encode_enum(&entry.status),
                    entry.detail
                ],
            )
            .map_err(|e| format!("persist optimization action: {e}"))?;
        Ok(connection.last_insert_rowid())
    }

    pub fn recent_actions(&self, limit: usize) -> Result<Vec<AuditEntry>, String> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| "database lock poisoned".to_string())?;
        let mut statement = connection
            .prepare(
                "SELECT id,executed_at,mode,action_id,confirmed,status,detail FROM optimization_actions ORDER BY id DESC LIMIT ?1",
            )
            .map_err(|e| format!("prepare action query: {e}"))?;
        let rows = statement
            .query_map([limit as i64], |row| {
                Ok(AuditEntry {
                    id: row.get(0)?,
                    executed_at: row.get(1)?,
                    mode: parse_mode(&row.get::<_, String>(2)?),
                    action_id: row.get(3)?,
                    confirmed: row.get::<_, i64>(4)? != 0,
                    status: parse_status(&row.get::<_, String>(5)?),
                    detail: row.get(6)?,
                })
            })
            .map_err(|e| format!("read optimization actions: {e}"))?;
        let mut entries = rows
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| format!("decode optimization actions: {e}"))?;
        entries.reverse();
        Ok(entries)
    }

    pub fn record_prediction(&self, prediction: &Prediction) -> Result<(), String> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| "database lock poisoned".to_string())?;
        connection
            .execute(
                "INSERT INTO prediction_history(created_at,horizon_minutes,available_slope,commit_slope,time_to_pressure_minutes,confidence) VALUES(?1,?2,?3,?4,?5,?6)",
                params![
                    prediction.generated_at,
                    prediction.horizon_minutes as i64,
                    prediction.available_forecast.slope_bytes_per_min,
                    prediction.commit_forecast.slope_bytes_per_min,
                    prediction.time_to_high_pressure_minutes,
                    prediction.confidence
                ],
            )
            .map_err(|e| format!("persist prediction: {e}"))?;
        Ok(())
    }

    pub fn prune_memory_data(&self, retention_days: u32) -> Result<usize, String> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| "database lock poisoned".to_string())?;
        let cutoff = format!("-{} days", retention_days.max(1));
        let mut pruned = 0;
        for (table, column) in [
            ("memory_snapshots", "collected_at"),
            ("process_memory_history", "collected_at"),
            ("prediction_history", "created_at"),
            ("optimization_recommendations", "created_at"),
        ] {
            pruned += connection
                .execute(
                    &format!(
                        "DELETE FROM {table} WHERE {column} < datetime('now', ?1) AND rowid NOT IN (SELECT rowid FROM {table} ORDER BY rowid DESC LIMIT 20000)"
                    ),
                    params![cutoff],
                )
                .map_err(|e| format!("prune {table}: {e}"))?;
        }
        pruned += connection
            .execute(
                "DELETE FROM optimization_actions WHERE executed_at < datetime('now','-90 days')",
                [],
            )
            .map_err(|e| format!("prune optimization_actions: {e}"))?;
        // Phase 3: additive retention. A row floor prevents pruning the entire
        // history on a machine that was idle for longer than the retention window.
        pruned += connection
            .execute(
                "DELETE FROM gpu_task_history WHERE finished_at < datetime('now', ?1) AND rowid NOT IN (SELECT rowid FROM gpu_task_history ORDER BY rowid DESC LIMIT 5000)",
                params![cutoff],
            )
            .map_err(|e| format!("prune gpu_task_history: {e}"))?;
        pruned += connection
            .execute(
                "DELETE FROM gpu_benchmark_results WHERE created_at < datetime('now', ?1) AND rowid NOT IN (SELECT rowid FROM gpu_benchmark_results ORDER BY rowid DESC LIMIT 2000)",
                params![cutoff],
            )
            .map_err(|e| format!("prune gpu_benchmark_results: {e}"))?;
        Ok(pruned)
    }

    pub fn load_memory_policy_config(&self) -> Result<Option<PolicyConfig>, String> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| "database lock poisoned".to_string())?;
        let payload: Option<String> = connection
            .query_row("SELECT payload FROM optimization_policies WHERE id=1", [], |row| row.get(0))
            .optional()
            .map_err(|e| format!("read policy config: {e}"))?;
        payload
            .map(|json| serde_json::from_str(&json).map_err(|e| format!("decode policy config: {e}")))
            .transpose()
    }

    pub fn save_memory_policy_config(&self, config: &PolicyConfig) -> Result<(), String> {
        let payload = serde_json::to_string(config).map_err(|e| format!("encode policy config: {e}"))?;
        let connection = self
            .connection
            .lock()
            .map_err(|_| "database lock poisoned".to_string())?;
        connection
            .execute(
                "INSERT INTO optimization_policies(id,payload,updated_at) VALUES(1,?1,?2) ON CONFLICT(id) DO UPDATE SET payload=excluded.payload,updated_at=excluded.updated_at",
                params![payload, chrono::Utc::now().to_rfc3339()],
            )
            .map_err(|e| format!("persist policy config: {e}"))?;
        Ok(())
    }
}

fn parse_level(value: &str) -> PressureLevel {
    match value {
        "elevated" => PressureLevel::Elevated,
        "high" => PressureLevel::High,
        "critical" => PressureLevel::Critical,
        _ => PressureLevel::Normal,
    }
}

fn parse_mode(value: &str) -> OptimizationMode {
    match value {
        "smartAssist" => OptimizationMode::SmartAssist,
        "manual" => OptimizationMode::Manual,
        _ => OptimizationMode::AnalysisOnly,
    }
}

fn parse_status(value: &str) -> ActionStatus {
    match value {
        "success" => ActionStatus::Success,
        "failed" => ActionStatus::Failed,
        "cancelled" => ActionStatus::Cancelled,
        _ => ActionStatus::Rejected,
    }
}

fn parse_risk(value: &str) -> RiskLevel {
    match value {
        "low" => RiskLevel::Low,
        "medium" => RiskLevel::Medium,
        _ => RiskLevel::None,
    }
}

fn encode_enum<T: serde::Serialize>(value: &T) -> String {
    serde_json::to_string(value)
        .unwrap_or_else(|_| "null".to_string())
        .trim_matches('"')
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::monitoring::{CpuMetrics, HardwareInfo, MemoryMetrics};

    fn test_snapshot() -> SystemSnapshot {
        SystemSnapshot {
            collected_at: chrono::Utc::now().to_rfc3339(),
            monitoring: true,
            cpu: CpuMetrics {
                name: "test cpu".into(),
                vendor: "test".into(),
                architecture: Some("x86-64".into()),
                physical_cores: Some(2),
                logical_processors: 4,
                usage_percent: Some(12.5),
                frequency_mhz: Some(3200),
                max_frequency_mhz: None,
                temperature_c: None,
                cores: vec![],
            },
            memory: MemoryMetrics {
                total_bytes: 1000,
                used_bytes: 250,
                available_bytes: 750,
                free_bytes: None,
                usage_percent: Some(25.0),
                cached_bytes: None,
                committed_bytes: None,
                commit_limit_bytes: None,
                pagefile_used_bytes: None,
                pagefile_total_bytes: None,
            },
            gpus: vec![],
            storage: vec![],
            processes: vec![],
            hardware: HardwareInfo {
                os_name: "Windows".into(),
                os_version: "test".into(),
                host_name: "test-host".into(),
                gpus: vec![],
            },
            errors: vec![],
        }
    }

    #[test]
    fn settings_and_history_persist_in_sqlite() {
        let directory = tempfile::tempdir().expect("temporary database directory");
        let database =
            Database::open_at(directory.path().join("test.sqlite3")).expect("open test database");
        let settings = MonitoringSettings {
            refresh_interval_ms: 2000,
            history_duration_minutes: 30,
            persist_history: true,
            monitoring_enabled: true,
            start_on_login: false,
            minimize_to_tray: false,
            theme: "dark".into(),
        };
        database.save_settings(&settings).expect("save preferences");
        assert_eq!(
            database
                .load_settings()
                .expect("load preferences")
                .unwrap()
                .refresh_interval_ms,
            2000
        );
        database
            .record_snapshot(&test_snapshot())
            .expect("record snapshot");
        let history = database.history().expect("read history");
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].cpu_percent, Some(12.5));
        assert_eq!(history[0].memory_percent, Some(25.0));
    }

    #[test]
    fn null_percentages_roundtrip_through_history() {
        let directory = tempfile::tempdir().expect("temporary database directory");
        let database =
            Database::open_at(directory.path().join("nulls.sqlite3")).expect("open test database");
        let mut snapshot = test_snapshot();
        snapshot.cpu.usage_percent = None;
        snapshot.memory.usage_percent = None;
        database.record_snapshot(&snapshot).expect("record snapshot");
        let history = database.history().expect("read history");
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].cpu_percent, None, "failed collections stay NULL");
        assert_eq!(history[0].memory_percent, None);
    }

    #[test]
    fn migration_three_preserves_legacy_rows() {
        let directory = tempfile::tempdir().expect("temporary database directory");
        let path = directory.path().join("legacy.sqlite3");
        {
            let connection = rusqlite::Connection::open(&path).expect("open legacy database");
            connection
                .execute_batch(
                    "CREATE TABLE schema_migrations (version INTEGER PRIMARY KEY, applied_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP);
                     INSERT INTO schema_migrations(version) VALUES (1),(2);
                     CREATE TABLE performance_snapshots (
                       id INTEGER PRIMARY KEY, collected_at TEXT NOT NULL,
                       cpu_percent REAL NOT NULL, memory_percent REAL NOT NULL,
                       gpu_percent REAL, vram_used_bytes INTEGER,
                       disk_read_bytes_per_second INTEGER, disk_write_bytes_per_second INTEGER);
                     INSERT INTO performance_snapshots(collected_at, cpu_percent, memory_percent)
                       VALUES ('2026-01-01T00:00:00Z', 12.5, 42.0);",
                )
                .expect("seed legacy schema");
        }
        let database = Database::open_at(path.clone()).expect("open with migration 3");
        let history = database.history().expect("read history");
        assert_eq!(history.len(), 1, "legacy rows survive the rebuild");
        assert_eq!(history[0].cpu_percent, Some(12.5));
        assert_eq!(history[0].memory_percent, Some(42.0));
        let again = Database::open_at(path).expect("reopen");
        assert_eq!(again.history().expect("read history").len(), 1, "migration is idempotent");
    }
}

fn database_path() -> Result<PathBuf, String> {
    #[cfg(windows)]
    {
        let root = std::env::var_os("APPDATA").ok_or("APPDATA is not set")?;
        Ok(PathBuf::from(root)
            .join("FluxCore")
            .join("fluxcore.sqlite3"))
    }
    #[cfg(not(windows))]
    {
        let root = std::env::var_os("XDG_DATA_HOME")
            .or_else(|| {
                std::env::var_os("HOME")
                    .map(|h| PathBuf::from(h).join(".local/share").into_os_string())
            })
            .ok_or("cannot locate application data directory")?;
        Ok(PathBuf::from(root)
            .join("FluxCore")
            .join("fluxcore.sqlite3"))
    }
}
