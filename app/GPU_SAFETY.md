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
