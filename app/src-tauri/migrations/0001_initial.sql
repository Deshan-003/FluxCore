CREATE TABLE IF NOT EXISTS schema_migrations (
  version INTEGER PRIMARY KEY,
  applied_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);
CREATE TABLE IF NOT EXISTS settings (
  key TEXT PRIMARY KEY,
  value TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS hardware (
  id INTEGER PRIMARY KEY CHECK (id = 1),
  payload TEXT NOT NULL,
  updated_at TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS performance_snapshots (
  id INTEGER PRIMARY KEY,
  collected_at TEXT NOT NULL,
  cpu_percent REAL NOT NULL,
  memory_percent REAL NOT NULL,
  gpu_percent REAL,
  vram_used_bytes INTEGER,
  disk_read_bytes_per_second INTEGER,
  disk_write_bytes_per_second INTEGER
);
CREATE INDEX IF NOT EXISTS idx_snapshots_collected_at ON performance_snapshots(collected_at);
INSERT OR IGNORE INTO schema_migrations(version) VALUES (1);
