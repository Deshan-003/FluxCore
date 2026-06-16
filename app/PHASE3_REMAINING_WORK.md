# FluxCore — Phase 3 Remaining Work Plan & Live Status

Derived from `PHASE3_PROGRESS_AUDIT.md` (initial state). Sequence follows the actual repository
state: make the crate compile → complete the four placeholder modules → persistence → frontend →
tests → real-hardware benchmarks → production build → docs. Status column is updated as work lands.

Legend: ✅ done & verified · 🔶 done, verification pending · ⬜ not started · 🚫 blocked by environment

## Critical — required for basic GPU acceleration to function

| # | Task | Why | Initial status | Files | Depends on | Expected result | Verification | Status |
|---|---|---|---|---|---|---|---|---|
| C1 | Fix `wgpu::InstanceDescriptor` construction | single E0599 blocked the whole crate | BROKEN | `providers/wgpu.rs:403` | — | crate compiles | WSL `cargo check --all-targets` | ✅ (check green) |
| C2 | Implement `resources.rs` (budget, gates, admission, accounting, config validation) | VRAM limits + safety gates | MISSING | `gpu_engine/resources.rs` | models, monitoring | percent-semantics budget, idempotent reserve/release | `gpu_safety_tests` | ✅ |
| C3 | Implement `scheduler.rs` (queue, priority, concurrency, cancel, timeout, backpressure, workers) | task execution engine | MISSING | `gpu_engine/scheduler.rs` | C2, execution | bounded, cancellable, fallback-capable execution | `gpu_scheduler_tests` | ✅ |
| C4 | Implement `persistence.rs` (task/benchmark/config/provider DML over migration 4) | history + settings | MISSING | `gpu_engine/persistence.rs` | database | round-trips, additive migrations | `gpu_safety_tests` | ✅ |
| C5 | Implement `benchmark.rs` (warm-up, N runs, medians, honest speedup) | real CPU-vs-GPU measurement | MISSING | `gpu_engine/benchmark.rs` | providers | no invented GPU timings | example + `PHASE3_BENCHMARK_REPORT.md` | ✅ (engine done; hardware measurements via ctypes probe, see F3) |
| C6 | Enforce provider size caps in `cpu.rs::can_execute` | contract §7.1 | PARTIAL | `providers/cpu.rs` | — | oversized admission rejected | `gpu_provider_tests::oversized_workload_is_rejected_by_caps` | ✅ |

## High Priority — reliable execution, scheduling, resource management

| # | Task | Why | Initial status | Files | Depends on | Expected result | Verification | Status |
|---|---|---|---|---|---|---|---|---|
| H1 | Phase 3 integration tests (provider/scheduler/safety/regression) | no Phase 3 test had ever run | MISSING | `src-tauri/tests/gpu_*.rs` | C1–C6 | green suite incl. cancel/timeout/fallback/concurrency | WSL `cargo test` | ✅ (121/121) |
| H2 | Cold-start-safe regression tests | engines initialize asynchronously | MISSING | `tests/gpu_regression_tests.rs` | H1 | Phase 1/2 proven intact with GPU engine present | same | ✅ |
| H3 | `examples/gpu_benchmark.rs` standalone harness | benchmarks without Tauri/DB | MISSING | `src-tauri/examples/gpu_benchmark.rs` | C5 | real report on any host | cross-build + native run | ✅ cross-built; 🚫 native exe WDAC-blocked (probe used instead) |
| H4 | Real-GPU verification on RTX 2050 (PTX JIT, launch, correctness, VRAM) | acceptance requires a working GPU backend | UNVERIFIED | cuda.rs + ptx | C1, H3 | kernels load & validate on hardware | signed-Python ctypes probe of shipped PTX | ✅ (4/4 kernels validated, VRAM released; test report §5) |
| H5 | CPU-fallback proof on real driver failure paths | never report CPU as GPU | UNVERIFIED | execution.rs, scheduler | C3 | fell_back=true with recorded reason | scheduler tests (mock) + native run | ✅ scheduler-level (mock failure → CPU, reason recorded); real-driver failure path UNVERIFIED (WDAC) |

## Medium Priority — frontend integration & controls

| # | Task | Why | Initial status | Files | Depends on | Expected result | Verification | Status |
