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
