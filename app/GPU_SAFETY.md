# FluxCore — GPU Safety Model (Phase 3)

Phase 3 accelerates **explicit, user-started computations** inside FluxCore. It never touches other
applications, never converts VRAM into system RAM, and never modifies hardware state.

## 1. Hard prohibitions (enforced by design, not by UI hiding)

| Prohibited | How it is prevented |
| --- | --- |
| Converting VRAM into Windows RAM / claiming combined memory | no such code path; UI copy states VRAM is GPU memory (`mod.rs` safety_notes, `ResourceBudgetPanel`) |
| Redirecting/accelerating other processes | workloads are a fixed registry executed in-process; no injection, no hooks |
| Allocating all VRAM | budget = `min(dedicated_total * vram_budget_percent/100, cap_mb MiB)`; defaults 50 % + 1024 MiB cap → 1 GiB on a 4 GiB card |
| Overclocking, voltage/firmware changes, disabling safety mechanisms | no such APIs are called; only documented CUDA driver / wgpu entry points |
| Kernel drivers / admin requirement | user-mode `nvcuda.dll` + wgpu; no elevation anywhere |
| Undocumented GPU APIs | every resolved symbol is a documented driver-API entry point (`GPU_BACKEND_NOTES.md` list) |
| Freeing other processes' VRAM | FluxCore releases only its own reservations/buffers |

## 2. Resource limits and gates (`resources.rs`)

- **Budget**: percent semantics (1.0–90.0, default 50.0) with an optional MiB cap (≤ 4096).
- **Remaining** = budget − reserved; reservations are id-keyed and idempotent (double reserve/release
  cannot corrupt accounting).
- **Safety gates** read from exactly one Phase 1 monitor snapshot per decision:
  GPU utilisation > `max_gpu_utilization_percent` (default 85), temperature > `max_temperature_c`
  (default 85 °C), system RAM available < `min_system_ram_available_percent` (default 15).
  Power is recorded as a note, not a blocker (board-power limit not reliably readable via NVML).
- **Concurrency**: `max_concurrent_gpu_tasks` 1–2 (default 1), `max_concurrent_cpu_tasks` 1–4 (default 2).
- **Backpressure**: queue length capped (`max_queue_length`, default 32); overflow submissions error.
- Utilization is a **safety gate only** — it is never a reason to route work to the GPU
  (`profiler.rs` decision order step 7; tested by `safety_gate_blockers_are_cpu_not_gpu_routing`).

## 3. Time bounds and WDDM TDR safety

- Vector launches chunked to ≤ 1,000,000 elements with a cancel/timeout checkpoint between chunks.
- Per-attempt timeout `task_timeout_ms` (500–60000, default 10000) enforced cooperatively inside
  providers and by the worker; wgpu readback uses bounded `PollType::Wait`.
- Matmul capped at m,k,n ≤ 2048 (catalogue default max 1024³) so a single launch stays far below the
  ~2 s TDR window; row_sum capped at rows*cols ≤ 32 M.
- A task stuck inside a driver call stays `Running` until the call returns (cooperative cancellation);
  the timeout then marks it `TimedOut` and releases accounting.

## 4. Cancellation and cleanup

- `CancelToken` (shared `AtomicBool`) checked between chunks by every provider; user cancellation
  outranks timeout.
- Scheduler releases the VRAM reservation on **every** exit path (success, failure, cancel, timeout,
  rejection) under the same lock that decrements the running counter.
- `Scheduler::Drop` cancels all tokens and joins workers; `GpuEngine::Drop` shuts every provider down.
- CUDA primary context retained once per session and released on shutdown; wgpu device dropped on
  unrecoverable error so the next attempt re-initializes.

## 5. Correctness and honesty

- Every GPU result is validated against the CPU reference (`validate_gpu`, tolerance
  `1e-3 * max(1, |ref|)`); mismatch → `CorrectnessFailed` with the real max absolute error.
- CPU fallback never reports GPU timings: `fell_back=true`, `gpu_time_ms=None`, reason recorded in
  `warnings`; `selected_backend` stays the backend that was *attempted*.
- Benchmarks discard warm-ups, report medians, and state slowdowns with equal prominence; when no GPU
  path ran, `gpu=None` plus a reason — a GPU time is never invented.
- Nullable telemetry renders as "Unavailable", never a fabricated 0.

## 6. Phase 1 / Phase 2 non-interference

- No second monitor thread: the engine reads `MonitorEngine::snapshot()` at most once per decision and
  `MemoryEngine::intelligence()` for RAM context.
- `gpu_task_history` is written on task completion only, never per monitor tick.
- Migration 4 is additive (`IF NOT EXISTS`, `INSERT OR IGNORE`, single transaction); Phase 1/2 tables
  and rows are untouched (tested by `migrations_are_additive_and_idempotent_across_reopen` and
  `migration_three_preserves_legacy_rows`).