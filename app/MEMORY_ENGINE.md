# Memory Engine (Phase 2)

The Intelligent Memory Engine lives in `src-tauri/src/memory_engine/` and extends the Phase 1
monitor without replacing any of it. It analyzes real Windows memory behavior, detects sustained
pressure, tracks per-application memory trends, predicts short-term developments, and recommends
— never silently performs — safe actions.

## Design principles

- **Honest measurement only.** Cached and standby memory is reported as reclaimable, never as
  "used" or as wasted RAM. Expected benefits are qualitative; FluxCore never invents byte savings.
- **No replacement of the Windows Memory Manager.** The engine observes, explains and advises.
- **GPU VRAM is never presented as system RAM** and is never a target for "memory transfer."
- **Sustained evidence, not single samples.** Trends, leaks and pressure states require multiple
  observations over time (hysteresis, minimum durations, regression quality gates).
- **Every system-affecting operation is explained, confirmed, audited.** See
  [OPTIMIZATION_SAFETY.md](OPTIMIZATION_SAFETY.md).

## Module map

| Module | Responsibility |
| --- | --- |
| `models.rs` | Canonical serde types shared with the frontend (camelCase JSON) |
| `stats.rs` | OLS linear regression + trend direction with noise floor |
| `analyzer.rs` | `GlobalMemoryStatusEx`, `K32GetPerformanceInfo` and PDH counters → `MemoryAnalysis` |
| `pressure_detector.rs` | Multi-signal pressure evaluation, hysteresis and minimum-duration state machine |
| `process_analyzer.rs` | Per-process readings (`K32GetProcessMemoryInfo` EX2), bounded sample history, trend + leak indicators |
| `compression.rs` | Windows memory compression reporting (`\Memory\Compressed Bytes`, "Memory Compression" process) |
| `virtual_memory.rs` | Commit charge/limit, pagefile usage, read-only pagefile configuration from the registry |
| `prediction.rs` | Short-term forecast from the regression window, with confidence and disclaimer |
| `policies.rs` | Idle/dashboard policy evaluation (background refresh throttling, detail boost) |
| `recommendations.rs` | Recommendation generation from live measurements only |
| `optimizer.rs` | Action registry (7 actions, none destructive) + read-only guidance collectors |
| `safety.rs` | Forbidden-operation list and mode/confirmation validation |
| `history.rs` | Persistence point construction + in-memory audit ring |
| `mod.rs` | `MemoryEngine` orchestrator: `tick()`, `intelligence()`, `execute_action()`, persistence |

## Data flow

```
monitor_loop (Phase 1 worker thread)
  └─ collector.collect() → SystemSnapshot (CPU/RAM/GPU/disk/processes)
       └─ MemoryEngine::tick(&snapshot)          ← integration point
            ├─ PDH counters + GlobalMemoryStatusEx + K32GetPerformanceInfo → MemoryAnalysis
            ├─ sample window → available-memory trend (bytes/min, OLS)
            ├─ PressureDetector::update (hysteresis + min durations) → PressureReport
            ├─ pressure event tracking (open/close rows in memory_pressure_events)
            ├─ policy evaluation (idle / dashboard-active / detail boost / paused)
            ├─ process scan (interval-throttled) → ProcessTracker → trends + leak indicators
            ├─ compression + virtual-memory analysis
            ├─ prediction (every 15 s) → Prediction
            ├─ recommendation generation → persisted new ones → MemoryIntelligenceSnapshot
            └─ interval persistence (memory_snapshots, process_memory_history, prune)
```

The frontend polls `get_memory_intelligence` and `get_memory_history_series` over the typed IPC
boundary; no page triggers extra hardware queries.

## Pressure model

Four states: `normal`, `elevated`, `high`, `critical`. Inputs (all real measurements):

- available physical memory percent,
- commit charge percent (commit limit),
- pagefile usage percent,
- hard faults (page reads/sec) — four-times the configured threshold counts as a signal,
- declining available-memory trend — escalates one level, with a 1.5 %/min floor to avoid
  reacting to noise.

State changes are debounced: a candidate level must persist for `upgrade_min_seconds`
(default 30 s) before upgrading and `downgrade_min_seconds` (default 120 s) before downgrading.
Sustained `elevated`+ periods are recorded as events with minimum available bytes and peak
commit percent.

## Process memory analysis

Per process: private bytes (commit charge), working set, private working set (when exposed),
peak working set, CPU percent, share of physical RAM, growth trend (bytes/min) and observation
window. Per-process *virtual size* is intentionally not reported — it requires
`NtQueryInformationProcess`, which is not a documented, crate-exposed API (see
[WINDOWS_COMPATIBILITY.md](WINDOWS_COMPATIBILITY.md)).

**Leak/growth indicator** — labeled only when *all* gates pass:

- ≥ 8 samples spanning ≥ 240 seconds,
- positive private-bytes slope greater than max(1 MiB/min, 0.5 %/min of process size),
- ≥ 80 % of consecutive deltas positive,
- regression r² ≥ 0.6.

Confidence: High (r² ≥ 0.9 and ≥ 20 samples), Medium (r² ≥ 0.75), otherwise Low. The label is
"suspected growth pattern", never an accusation; the reason string embeds the raw numbers.

## Prediction

`prediction.rs` runs OLS over a bounded window (`prediction_window_minutes`, default 30) of
system samples and per-process series. It reports available-memory and commit slopes, projected
values at the horizon, and estimated minutes until the high-pressure threshold — or
"not projected" when the trend does not support that claim. Confidence = `0.6·r² + 0.4·window
coverage`, and every prediction carries a disclaimer that forecasts are estimates from recent
behavior, not guarantees.

## Policies

Evaluated each tick (`policies.rs`): dashboard-active vs idle detection, background pause (set by
the pause action), detail boost under pressure. When the Memory Intelligence / Optimization Center
pages are hidden or the window is backgrounded, the frontend reports inactivity
(`set_dashboard_active`) and the engine throttles process scanning and persistence — the engine
costs less when nobody is looking.

## Optimization modes

| Mode | Behavior |
| --- | --- |
| `analysisOnly` | Observe, detect, analyze, recommend. No action can execute. |
| `smartAssist` | Actions from the registry may run; anything requiring confirmation still requires explicit approval in the UI. |
| `manual` | Same registry, user explicitly invokes each action with per-action confirmation. |

The action registry contains exactly seven entries (four FluxCore-internal, three read-only
guidance collectors); none changes system state. Everything else is rejected by `safety.rs`
before execution. Details: [OPTIMIZATION_SAFETY.md](OPTIMIZATION_SAFETY.md).

## Persistence (migration 0002)

All tables are created idempotently alongside the Phase 1 schema — Phase 1 tables are untouched.

| Table | Contents |
| --- | --- |
| `memory_snapshots` | Periodic analysis points (used/available/committed/compressed/pagefile/pressure level) |
| `memory_pressure_events` | Sustained elevated/high/critical periods with min available + peak commit |
| `process_memory_history` | Top-N process points (name, pid, private, working set, trend) |
| `optimization_recommendations` | Generated recommendations as first surfaced |
| `optimization_actions` | Full audit: every executed, rejected, failed or cancelled action |
| `optimization_policies` | Persisted policy configuration |
| `prediction_history` | Forecasts with confidence for later honesty checks |

Retention: `prune_memory_data(retention_days)` (default 30 days) runs after each persistence
interval, with a 20 000-row floor per table and a separate 90-day audit prune so the action
audit trail outlives telemetry.

## IPC surface

`get_memory_intelligence`, `get_memory_history_series`, `get_optimization_mode`,
`set_optimization_mode`, `get_optimization_actions`, `execute_optimization_action`,
`get_memory_policy_config`, `update_memory_policy_config`, `set_dashboard_active` — all typed in
`src/services/memory.ts`, mirrored 1:1 from the Rust models in `src/types/memory.ts`.

## Tests

Every module carries unit tests over synthetic fixtures (no live hardware dependency): regression
math, pressure escalation/hysteresis, leak gating, compression fallbacks, pagefile parsing,
prediction confidence, policy selection, recommendation triggers, safety rejection of all
forbidden operations, registry invariants, and engine tick/persistence against a temporary
SQLite database. Production code paths use real system data exclusively.
