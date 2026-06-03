-- Phase 2: Intelligent Memory Engine persistence.
-- Applied by database.rs on top of migration 0001; Phase 1 tables are untouched.
CREATE TABLE IF NOT EXISTS memory_snapshots (
  id INTEGER PRIMARY KEY,
  collected_at TEXT NOT NULL,
  used_bytes INTEGER NOT NULL,
  available_bytes INTEGER NOT NULL,
  committed_bytes INTEGER,
  compressed_bytes INTEGER,
  pagefile_used_bytes INTEGER,
  pressure_level TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_memory_snapshots_at ON memory_snapshots(collected_at);

CREATE TABLE IF NOT EXISTS memory_pressure_events (
  id INTEGER PRIMARY KEY,
  started_at TEXT NOT NULL,
  ended_at TEXT,
  level TEXT NOT NULL,
  reason TEXT NOT NULL,
  min_available_bytes INTEGER NOT NULL,
  peak_commit_percent REAL
);

CREATE TABLE IF NOT EXISTS process_memory_history (
  id INTEGER PRIMARY KEY,
  collected_at TEXT NOT NULL,
  pid INTEGER NOT NULL,
  name TEXT NOT NULL,
  private_bytes INTEGER NOT NULL,
  working_set_bytes INTEGER NOT NULL,
  trend_bytes_per_min REAL NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_process_history_at ON process_memory_history(collected_at);

CREATE TABLE IF NOT EXISTS optimization_recommendations (
  id INTEGER PRIMARY KEY,
  created_at TEXT NOT NULL,
  recommendation_id TEXT NOT NULL,
  title TEXT NOT NULL,
  reason TEXT NOT NULL,
  risk TEXT NOT NULL,
  category TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS optimization_actions (
  id INTEGER PRIMARY KEY,
  executed_at TEXT NOT NULL,
  mode TEXT NOT NULL,
  action_id TEXT NOT NULL,
  confirmed INTEGER NOT NULL,
  status TEXT NOT NULL,
  detail TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS optimization_policies (
  id INTEGER PRIMARY KEY CHECK (id = 1),
  payload TEXT NOT NULL,
  updated_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS prediction_history (
  id INTEGER PRIMARY KEY,
  created_at TEXT NOT NULL,
  horizon_minutes INTEGER NOT NULL,
  available_slope REAL NOT NULL,
  commit_slope REAL,
  time_to_pressure_minutes REAL,
  confidence REAL NOT NULL
);

INSERT OR IGNORE INTO schema_migrations(version) VALUES (2);
