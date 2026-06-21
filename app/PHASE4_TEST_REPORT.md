# Phase 4 test and verification report

Date: 2026-11-20. Phase 4 is developed on `feature/phase4-resource-intelligence`, based on the existing `feature/phase3-gpu-acceleration` commit `49c902b`. The Phase 3 work is not present on `main`, so Phase 4 was kept on a separate branch.

## Executed tests and builds

| Test / build | Result |
| --- | --- |
| Final full Rust suite (`cargo test --locked`, WSL Ubuntu) | **137 passed, 0 failed**: 106 library + 8 GPU provider + 3 Phase 1/2 regression + 9 GPU safety + 10 scheduler + 1 Phase 4 integration. Includes forecasting, robust baselines, sustained anomaly/hysteresis, confounder-aware outcomes, policy bounds, Smart Assist confirmation, adaptive opt-in, stale-sample rejection, monitor/history-off clearing, restart interruption, manual apply/rollback, SQLite persistence and the monitor-loop integration guard. |
| Rust all-target compile | **PASS** for Linux and `x86_64-pc-windows-gnu`, both `cargo check --locked --all-targets`. Includes Windows-gated collector code. |
| Phase 4 overhead benchmark (`examples/intelligence_benchmark.rs`, release) | **Executed twice, 20 sampled observations per pass, on live collector data.** Results below. |
| Frontend (native Windows, Vitest) | **50 passed, 0 failed across 7 files**, including 2 Phase 4 tests. Existing chart/jsdom size and `act(...)` warnings appear on stderr; tests pass. |
| Frontend production build (`npm --prefix app run build`) | **PASS**, `tsc -b` and Vite production build, 2167 modules. Vite reports event-import chunking and >500 kB bundle warnings; output generated. |
| Frontend lint (`npm --prefix app run lint`) | **PASS**, ESLint exits 0. Generated Rust build output is excluded. |
| `git diff --check` | **PASS**. |

### Test-run reconciliation

Earlier runs reported 131 tests (before the final policy and integration tests were added) and 104–106 library-only tests. The table above is the single final full-suite run after all Phase 4 tests were added; it supersedes those intermediate counts. An accidentally duplicated WSL cargo invocation was stopped before its test execution; its exit 15 was not a test failure and is not counted. The Windows GNU `cargo check --all-targets` passes on the final source.

## Phase 4 overhead (measured)

Harness: `app/src-tauri/examples/intelligence_benchmark.rs`, release build, WSL Ubuntu, two independent runs.
Metric values come from the live OS collector; only the snapshot timestamp is shifted by the 15-second
cadence so every iteration performs a full sample instead of the early return. Each pass ran 20
in-memory samples and 20 persisted samples.

| Stage | Run A | Run B |
| --- | --- | --- |
| Phase 1 collector pass | 6,563 µs | 5,951 µs |
| Phase 2 memory engine tick | 50 µs | 48 µs |
| Phase 4 analysis only (forecast, anomaly, findings) | 164 µs | 97 µs |
| Phase 4 full sample including SQLite | 939 µs | 782 µs |
| Persistence delta (SQLite transaction) | 775 µs (82.5%) | 685 µs (87.6%) |
| Phase 4 gated tick inside the 15 s window | < 1 µs | < 1 µs |
| Phase 4 share of the Phase 1+2 pipeline per sample | 14.2% | 13.0% |
