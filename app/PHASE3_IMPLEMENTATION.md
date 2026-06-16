# FluxCore — Phase 3 Implementation Record

What was implemented during the continuation wave, on top of the pre-existing Phase 3 code
(see `PHASE3_PROGRESS_AUDIT.md` for the initial state). Existing working modules were preserved.

## 1. Backend modules completed (were 1-line placeholders)

### `gpu_engine/resources.rs` (new, ~260 ln)
- `ResourceManager { monitor: Arc<MonitorEngine>, reserved: Mutex<HashMap<u64,u64>>, queued_gpu: AtomicUsize }`.
- `budget_from_total`: `min(total * percent/100, cap_mb * MiB)` — percent semantics per freeze §0bis C3.
- `status()` reads **one** snapshot; gates (utilisation / temperature / system-RAM-available) computed
  from that same snapshot; `safety_level` Ok/Caution/Blocked.
- `admit()` enforces concurrency, gates, remaining budget and real free VRAM, then reserves.
- `reserve`/`release` idempotent per task id; `validate_config` enforces every §5.4 range with exact messages.
- Additive `set_queued_gpu_tasks` so queue depth is truthful in the UI (documented deviation).

### `gpu_engine/scheduler.rs` (new, ~430 ln)
- Priority `BinaryHeap` (priority desc, queued_at asc, id asc), task map, per-task `CancelToken` and
  `require_gpu` side maps.
- One serialised GPU worker + CPU worker pool bounded by `max_concurrent_cpu_tasks`; workers created
  lazily on first submit, joined in `Drop`.
- Admission at dequeue (documented deviation from freeze §9), CPU re-queue when admission denies and
  `require_gpu == false`, `Rejected` when `require_gpu == true`.
- Cooperative cancel of queued tasks (heap rebuild) and running tasks (token); wall-clock outcome
  mapping Succeeded/Failed/Cancelled/TimedOut; reservation released on every exit path.
- Backpressure at `max_queue_length`; observer invoked on every state transition.

### `gpu_engine/persistence.rs` (new, ~260 ln)
- All freeze §7 functions over `Database::with_connection`; JSON `payload` authoritative, narrow
  columns for indexing; `warnings` stored as JSON.
- `record_task`/`recent_tasks`, `record_benchmark`/`recent_benchmarks`/`clear_benchmarks`,
  `save_config`/`load_config` (single-row upsert), `save_workload_settings`/`load_workload_settings`
  (transactional replace), `upsert_provider` (+ refreshes `gpu_provider_cache` so the table is a real cache).
- `is_terminal` helper shared with the observer/scheduler.

### `gpu_engine/benchmark.rs` (new, ~260 ln)
- `run_once` (single sample, never substitutes backends) and `run_benchmark` (warm-ups discarded,
  N measured runs, min/median/mean/max/stddev, transfer median, peak VRAM, max abs error).
- `speedup = cpu_median / gpu_end_to_end_median`; slowdowns reported with equal prominence;
  `gpu: None` + reason when no GPU path ran; cancel + total-time budget honoured.

## 2. Fixes to pre-existing code (minimal, contract-aligned)

| File | Change | Reason |
| --- | --- | --- |
| `providers/wgpu.rs:403` | `InstanceDescriptor::new_without_display_handle_from_env()` | previous call does not exist in wgpu 30.0.2; blocked whole-crate compile |
| `providers/cpu.rs::can_execute` | enforce advertised `max_elements` caps | contract §7.1 provider-enforced caps; cuda.rs already did |
| `src/App.tsx:55` | chart window anchored to newest sample timestamp instead of `Date.now()` in render | pre-existing `react-hooks/purity` lint error; behaviour preserved for live data |
| `src/pages/GpuAcceleration.tsx` | draft config derived as `draftOverride ?? config` (no setState-in-effect) | `react-hooks/set-state-in-effect` lint error |

## 3. Frontend added (was entirely absent)

- `src/types/gpu.ts` — full camelCase mirrors incl. discriminated `WorkloadParams` union.
- `src/services/gpu.ts` — 17 invoke wrappers + `onGpuTaskEvent` / `onGpuCapabilitiesEvent`.
- `src/stores/gpu.ts` — zustand store, localStorage `fluxcore.gpu.preferences.v1`, event-driven task updates.
- `src/pages/GpuAcceleration.tsx` — five tabs (Overview / Workload Intelligence / Acceleration Center /
  Task Monitor / Performance Comparison); every control bound to a real command; no production mocks.
- `src/components/gpu/{GpuStatusCard,BackendBadge,WorkloadRecommendationRow,TaskMonitorTable,
  BenchmarkComparisonChart,ResourceBudgetPanel,UnsupportedState}.tsx` + `gpu.test.tsx`.
- `src/App.css` — appended `gpu-*` section only; reuses existing panels/bars/notes/toggles.

## 4. Tests and harness added

- `src-tauri/tests/gpu_provider_tests.rs` (8), `gpu_scheduler_tests.rs` (10, mock GPU provider),
  `gpu_safety_tests.rs` (9), `gpu_regression_tests.rs` (3).
- `src-tauri/examples/gpu_benchmark.rs` — standalone, no Tauri/DB, real measurements only.

## 5. Verification environment

WSL Ubuntu (rustup 1.100.0 + mingw cross + Tauri Linux deps) for `cargo check`/`cargo test`;
native Windows for `tsc -b`, `vite build`, `eslint`, `vitest`; mingw cross-build to
`x86_64-pc-windows-gnu` for native Windows GPU execution. See `GPU_COMPATIBILITY.md` §5 and
`PHASE3_TEST_REPORT.md` for executed results.