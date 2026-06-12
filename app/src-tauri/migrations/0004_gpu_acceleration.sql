-- Phase 3: GPU Workload Acceleration & Intelligent CPU/GPU Workload Offloading.
-- Authoritative copy lives in src-tauri/src/database.rs (migration block "Migration 4");
-- this file documents the exact schema. Existing Phase 1/2 tables are untouched.

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