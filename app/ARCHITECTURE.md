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
OS-registered base clock as fallback (the base clock is separately exposed as the maximum).
Pagefile usage comes from `GlobalMemoryStatusEx`. Per-volume drive type and read/write rates
come from `IOCTL_STORAGE_QUERY_PROPERTY` and `IOCTL_DISK_PERFORMANCE` deltas between ticks.
Windows CIM (`Win32_VideoController`) supplies GPU identity and driver version. NVML is loaded
once per collector session (`nvmlInit_v2` at open, `nvmlShutdown` on drop) and all supported
NVIDIA sensors are refreshed every tick.
The memory engine adds `GlobalMemoryStatusEx`, `K32GetPerformanceInfo`,
`K32GetProcessMemoryInfo` (EX2) and a PDH counter query — full list and fallback behavior in
[WINDOWS_COMPATIBILITY.md](WINDOWS_COMPATIBILITY.md). Providers fail independently and
unsupported values serialize as `null`.

## Data integrity

Physical RAM used is `total - available`. OS committed memory is a distinct counter, never
added to physical RAM. Standby, modified and cache bytes are reported as **reclaimable** — they
are never counted as unavailable, and free-RAM reporting never implies wasted RAM. Dedicated GPU
memory, shared GPU memory and system RAM are separate nullable fields. Disk capacity usage is
distinct from disk I/O activity. Percentages are nullable end to end: when a total is zero or a
counter is missing, the derived percent serializes as `null` (and persists as SQL NULL since
migration 0003) instead of a fabricated 0. Missing provider readings remain null and render as
"Unavailable", with engine limitations surfaced explicitly in the UI.

## Health thresholds and pressure

Phase 1 UI thresholds: CPU or physical RAM at 75% is elevated; at 90% it is critical. Phase 2
adds the memory pressure state machine (normal/elevated/high/critical) driven by available
memory, commit charge, pagefile usage, hard-fault rate and trend, with hysteresis
(upgrade ≥ 30 s, downgrade ≥ 120 s by default) and persisted pressure events. Thresholds and
hysteresis are user-configurable through the policy editor and validated in the backend.

## Optimization and safety

Three modes (AnalysisOnly / SmartAssist / Manual) gate a seven-action registry that only
touches FluxCore's own resources or provides read-only guidance. Destructive operations
(process termination, working-set trimming, standby purging, pagefile changes, compression
toggling) are rejected by `safety.rs` regardless of mode or confirmation, and every outcome is
audited. See [OPTIMIZATION_SAFETY.md](OPTIMIZATION_SAFETY.md).

## Persistence and privacy

No data leaves the device. The database stores settings, hardware identification, sampled
aggregate resource values and (Phase 2) memory snapshots, pressure events, top-N process memory
points (name/PID/private/working-set/trend), generated recommendations, predictions and the
optimization audit trail. Memory-intelligence retention defaults to 30 days with a per-table row
floor; the action audit is kept for 90 days. Phase 1 seven-day history retention is unchanged.

## Current limitations

See [HARDWARE_COMPATIBILITY.md](HARDWARE_COMPATIBILITY.md) and
[WINDOWS_COMPATIBILITY.md](WINDOWS_COMPATIBILITY.md). Tray/startup integration, generic
AMD/Intel GPU telemetry, shared GPU memory accounting, disk temperature/SMART health, GPU
process attribution and per-process virtual size are not in scope; the UI does not claim those
are active.
