-- Migration 3: performance snapshot percentages become nullable.
-- A failed collection is stored as NULL instead of a misleading zero.
-- The table is rebuilt in place and every existing row is copied over;
-- no data is dropped. Mirrors the conditional batch in database.rs.
BEGIN;
CREATE TABLE IF NOT EXISTS performance_snapshots_v3 (
  id INTEGER PRIMARY KEY, collected_at TEXT NOT NULL,
  cpu_percent REAL, memory_percent REAL,
  gpu_percent REAL, vram_used_bytes INTEGER,
  disk_read_bytes_per_second INTEGER, disk_write_bytes_per_second INTEGER
);
INSERT INTO performance_snapshots_v3(id, collected_at, cpu_percent, memory_percent, gpu_percent, vram_used_bytes, disk_read_bytes_per_second, disk_write_bytes_per_second)
  SELECT id, collected_at, cpu_percent, memory_percent, gpu_percent, vram_used_bytes, disk_read_bytes_per_second, disk_write_bytes_per_second
  FROM performance_snapshots;
DROP TABLE performance_snapshots;
ALTER TABLE performance_snapshots_v3 RENAME TO performance_snapshots;
CREATE INDEX IF NOT EXISTS idx_snapshots_collected_at ON performance_snapshots(collected_at);
INSERT OR IGNORE INTO schema_migrations(version) VALUES (3);
COMMIT;
