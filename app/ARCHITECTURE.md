# Architecture

## Boundaries

- `src/` is the React/TypeScript presentation layer. Zustand owns the latest snapshot, bounded
  chart windows, preferences, process-table controls and (Phase 2) the memory store
  (`src/stores/memory.ts`). The monitoring store is event-driven: it subscribes to the backend
  `fluxcore-snapshot` Tauri event and only falls back to interval polling when event
  subscription fails. Recharts receives only collected samples. Pure display helpers live in
  `src/lib/format.ts`, process search/sort rules in `src/lib/processes.ts`, and shared
  presentational components in `src/components/monitoring.tsx`; all three carry colocated
  vitest tests (`src/**/*.test.ts[x]`).
- `src/services/monitoring.ts` and `src/services/memory.ts` are the typed Tauri IPC boundary.
  Browser preview detects the absence of Tauri and shows an unsupported state instead of
  manufacturing metrics.
- `src-tauri/src/monitoring.rs` owns Phase 1 collection and serialization. A named Rust worker
  thread refreshes system metrics, processes, mounted disks and optional GPU providers. Its
  shared snapshot/settings use `RwLock`; the SQLite connection is serialized through a mutex.
  The thread is signaled and joined on application state drop.
- `src-tauri/src/memory_engine/` (Phase 2) is the Intelligent Memory Engine: analyzer, pressure
  detector, process analyzer, compression/virtual-memory reporting, prediction, policies,
  recommendations, the action registry, the safety validator and the orchestrator
  (`MemoryEngine`). Module detail: [MEMORY_ENGINE.md](MEMORY_ENGINE.md).
- `src-tauri/src/database.rs` owns local SQLite schema and storage under
  `%APPDATA%\FluxCore\fluxcore.sqlite3`. Migrations are idempotent; Phase 1 samples persist
  every five seconds with seven-day retention; Phase 2 migration 0002 adds memory-intelligence
  tables without touching Phase 1 tables; migration 0003 relaxes `performance_snapshots`
  cpu/memory percent columns to nullable via an in-place table rebuild that preserves existing
  rows, so a collection tick that could not compute a percentage stores NULL instead of a
  fabricated 0.

## Integration point

`MemoryEngine` is constructed once in `lib.rs::run()`, shared as `Arc<MemoryEngine>` with
`MonitorEngine`, and ticked from the existing monitor worker loop after each collection:

```
monitor_loop:  collector.collect() → store snapshot → db.record(snapshot)
                                     ├→ memory.tick(&snapshot)
                                     └→ app.emit("fluxcore-snapshot", snapshot)
```

The engine never spawns its own polling threads and never triggers additional hardware queries
beyond its single PDH collection per tick; the UI polls engine state, so no page can cause
redundant collection. `ProcessMetrics` gained one additive field (`started_at`) for process
uptime context.

## Collection and IPC

Phase 1 commands: `get_snapshot`, `get_settings`, `save_settings`, `get_history`,
`check_hardware`. Phase 2 commands: `get_memory_intelligence`, `get_memory_history_series`,
`get_optimization_mode`, `set_optimization_mode`, `get_optimization_actions`,
`execute_optimization_action`, `get_memory_policy_config`, `update_memory_policy_config`,
`set_dashboard_active`. The Rust worker continuously updates one snapshot at the configured
cadence and emits it as the `fluxcore-snapshot` Tauri event after every tick; the frontend
store subscribes to that event and falls back to interval polling only if event subscription
fails. `get_snapshot` clones current state for on-demand reads. When history persistence is
enabled, SQLite rows are the single chart source and are re-read on a five-second cadence.
Polling is capped to 250 ms–60 s and disabled monitoring pauses collection.

The collector uses `sysinfo` for CPU, memory, process and mounted-volume data. CPU identity
comes from OS CPU descriptors, architecture from `GetNativeSystemInfo`, and current frequency
from the PDH `\Processor Information(_Total)\Processor Frequency` counter with the
