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
|---|---|---|---|---|---|---|---|---|
| M1 | `types/gpu.ts` contract mirrors | type-safe IPC | MISSING | `src/types/gpu.ts` | — | camelCase mirrors, nullable numbers | `tsc -b` | ✅ |
| M2 | `services/gpu.ts` 17 wrappers + 2 event listeners | wire every command | MISSING | `src/services/gpu.ts` | M1 | 1:1 command mapping | build + tests | ✅ |
| M3 | `stores/gpu.ts` + localStorage prefs | state + cadence | MISSING | `src/stores/gpu.ts` | M2 | refresh/setMode/saveConfig/submit/cancel/benchmark | vitest | ✅ |
| M4 | `pages/GpuAcceleration.tsx` 5 tabs | all five required views | MISSING | `src/pages/GpuAcceleration.tsx` | M3 | Overview/Workloads/Center/Tasks/Comparison, no dead controls | vitest + build | ✅ |
| M5 | `components/gpu/*` (7 components) + `gpu-*` CSS | reuse existing design language | MISSING | `src/components/gpu/*`, `src/App.css` | M1 | cards/rows/table/chart/budget/badge/unsupported | `gpu.test.tsx` | ✅ |
| M6 | Remove non-functional controls & mocks from production | spec §5.8 | n/a | page | M4 | every control hits a real command | code review + tests | ✅ |
| M7 | Fix pre-existing `App.tsx` render-purity lint error | `npm run lint` must pass | BROKEN (pre-existing) | `src/App.tsx:55` | — | lint green, chart window preserved | `npm run lint` | ✅ |

## Finalization — testing, bug fixing, documentation, release verification

| # | Task | Why | Initial status | Files | Depends on | Expected result | Verification | Status |
|---|---|---|---|---|---|---|---|---|
| F1 | Full Rust suite green (lib + 4 integration targets) | acceptance | MISSING | tests | H1 | 0 failures | WSL `cargo test` | ✅ (121 passed, 0 failed) |
| F2 | Frontend suite + lint + production vite build green | acceptance | PARTIAL | app | M1–M7 | 48/48 vitest, lint 0 errors, vite build ok | native npm | ✅ (48/48, lint clean, build ok) |
| F3 | Real CPU-vs-GPU benchmark report | acceptance §9 | MISSING | `PHASE3_BENCHMARK_REPORT.md` | H4 | measured medians, honest verdicts | ctypes probe on RTX 2050 | ✅ (written from measured probe runs; numpy CPU baseline labelled) |
| F4 | Production Tauri build / documented constraint | acceptance §11 | MISSING | build config | F1 | installer or honest environment limitation | `npm run tauri build` attempt | 🚫 documented: MSVC absent; winget VS BuildTools exit 1602 (UAC declined, not forced) |
| F5 | Documentation set (architecture/implementation/compat/guide/safety/test/benchmark/completion) | acceptance §10 | PARTIAL | `app/*.md` | F1–F4 | evidence-based reports | review | ✅ (all 10 docs written/updated) |
| F6 | Update this plan + final audit statuses | keep reports truthful | — | this file, audit | all | statuses match evidence | review | ✅ |

## Environment installs performed (authorized by the user)

- WSL Ubuntu: rustup stable (1.100.0), `gcc-mingw-w64-x86-64-posix`, `gcc`, `pkg-config`,
  `libgtk-3-dev`, `libwebkit2gtk-4.1-dev`, `libsoup-3.0-dev`, `libjavascriptcoregtk-4.1-dev`
  (the same dependency set as `tools/verify/Dockerfile`).
- Windows: `rustup target add x86_64-pc-windows-gnu` (target std only; the GNU *host* toolchain
  binaries are SAC-blocked and are not used).
- **Not installed:** CUDA toolkit (unnecessary — driver API + embedded PTX), MSVC Build Tools
  (see F4: native MSVC linking is unavailable and SAC policy applies to downloaded toolchains).

## Known deviations from the frozen contract (reported, not silent)

1. `resources.admit` is called when a worker **dequeues** a GPU task, not at submit (freeze §9 said
   submit). Admitting at submit would reserve VRAM for queued tasks and let the concurrency gate
   reject queueing; dequeue-time admission keeps the queue usable while still enforcing budget,
   gates and concurrency before any GPU execution. Release happens on every exit path.
2. `ResourceManager::set_queued_gpu_tasks` added (additive) so the UI can show queued vs running
   truthfully; the frozen surface had no setter.
3. `ResourceBudgetStatus.allocated_bytes` mirrors `reserved_bytes` (reservations are released exactly
   when the matching buffers free); subtracting both would double-count the same bytes (R9).
4. `ResourceManager::set_device_vram_total` added (additive): provider-reported dedicated VRAM is used
   for the budget only when the Phase 1 monitor snapshot has no GPU (NVML absent / Linux verification
   host). Without it, every GPU admission was denied on such hosts (found by the scheduler tests).

## Status update — 2026-11-17 (final)

- C1–C6 ✅. H1/H2 ✅ — 121 Rust tests, 0 failures (91 lib + 8 provider + 3 regression + 9 safety +
  10 scheduler), executed in WSL Ubuntu.
- H3 ✅ cross-built release `gpu_benchmark.exe` (mingw-w64). Native execution 🚫: organization
  Device Guard/WDAC policy blocks unsigned cross-built executables; policy **not bypassed**.
- H4 ✅ via signed-Python `ctypes` probe against `nvcuda.dll` using the exact shipped PTX and the
  same deterministic inputs: 4/4 kernels validated (max_abs_error ≤ 5.72e-06), VRAM fully released
  (3302 MiB free before and after). Details: test report §5.
- H5 ✅ at scheduler level (mock GPU failure → CPU fallback, reason recorded, `fell_back=true`;
  `require_gpu` never falls back). Real-driver failure-path execution UNVERIFIED (WDAC).
- M1–M7 ✅. F1 ✅. F2 ✅ (vite build, vitest 48/48, eslint 0 errors).
- F3 ✅ — `PHASE3_BENCHMARK_REPORT.md` written from measured probe runs: axpy 1M 1.25x, scale 1M
  1.44x, matmul 256³ 64.11x, row_sum 1024² 7.91x (CPU baseline = numpy reference, labelled).
- F4 🚫 documented — native MSI/NSIS bundling needs MSVC Build Tools; winget install ended exit
  1602 (UAC/user-exit) and was not forced. Install instructions in `GPU_ACCELERATION_GUIDE.md` /
  completion report. Frontend production build + Rust release cross-build verified.
- F5/F6 ✅ — all 10 documents present; `PHASE3_COMPLETION_REPORT.md` written with the 14 required
  items; audit matrix final statuses appended there.
- Additional bug fixed during verification: scheduler self-deadlock (`submit` held the shared mutex
  while `ensure_workers` re-locked it) — found by a hanging test, fixed by passing the CPU cap in.