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
