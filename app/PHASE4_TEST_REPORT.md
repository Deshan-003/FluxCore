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
| SQLite file after 40 observations | 4.0 KiB | 4.0 KiB |
| Benchmark process resident footprint | 38.4 MiB | 38.4 MiB |

Interpretation: a full Phase 4 sample costs **0.78–0.94 ms** and runs every 15 seconds, i.e. roughly
**0.05–0.06 ms per second of monitoring (about 0.006% of one core)**. Monitoring ticks inside the
sampling window pay less than one microsecond. SQLite persistence dominates the sample; the analysis
itself is 0.10–0.16 ms. These figures are from the Linux logic path, where GPU and storage-I/O
collectors are stubbed; native Windows collector cost and the installed-app footprint are not
measured by this harness.

## Native Windows build and runtime (2026-11-20)

The user authorized installing the native toolchain, so Visual Studio 2022 Build Tools 17.14.45
(MSVC 14.44.35207, Windows SDK 10.0.26100) was installed via winget. WebView2 runtime was already
present.

| Native step | Result |
| --- | --- |
| MSVC compile/link, debug (`cargo build`, `cargo test --no-run`) | **PASS** — library, all six test targets and the app binary (29.3 MB) were produced |
| `intelligence_integration_tests` (monitor loop, real Windows collectors) | **PASS** — executed natively, exit 0 |
| `fluxcore_lib` test harness | **FAILED TO EXECUTE** — `STATUS_ENTRYPOINT_NOT_FOUND` (0xC0000139); an earlier attempt was blocked with os error 4551 |
| App launch (`fluxcore.exe`) | **BLOCKED** — "An Application Control policy has blocked this file" (os error 4551) |
| Rebuilding new build scripts / cleaned packages | **BLOCKED** — os error 4551 |
| MSI/NSIS installer | **NOT BUILT** — blocked by the same policy |
| Native Phase 4 benchmark run | **NOT RUN** — execution blocked |

Policy evidence: `HKLM\SYSTEM\CurrentControlSet\Control\CI\Policy\VerifiedAndReputablePolicyState = 1`
(Smart App Control enabled). Per the user's explicit decision on 2026-11-20, Smart App Control was
**left enabled**; no exclusion or other security change was made. Disabling SAC is irreversible
without resetting Windows and remains the machine owner's decision. Consequence: native
runtime/UI verification, native overhead measurement and installer creation remain
environment-blocked even though the compiler is now installed and native compilation succeeds.

A separate build-environment issue was observed while rebuilding: intermittent
`E0463 can't find crate for <proc-macro>` for `document_features`, `tauri_macros` and
`zerofrom_derive`. rustc's metadata log reported "Rejecting via proc macro: expected false got
true", i.e. a stale/ambiguous artifact after partial rebuilds. Cleaning the affected crate
sometimes restored a loadable artifact. This is an artifact-resolution condition, not a source
defect: the same sources compile cleanly and pass 137 tests under WSL, and the native binaries
that were produced ran or failed according to the SAC policy above.

## Coverage

- Forecast walk-forward model selection, unavailable conditions, uncertainty confidence and percentage bounds.
- Anomaly duration, recovery hysteresis, interrupted/missing samples and outlier-resistant baselines.
- Before/after scan-cost evaluation with stable workload and changed workload cases.
- SQLite migration 5 additive/repeatable behavior, existing GPU config preservation, forecast matching to actual values only near the target.
- Adaptive policy validation, Smart Assist confirmation, explicit adaptive enablement, stale telemetry rejection, monitor/history-off clearing, manual apply, expiry/rollback logic and persistence.
- Phase 1/2/3 regression coverage, GPU provider/scheduler/resource safety paths.
- Restart behaviour: an unfinished temporary override is marked interrupted and the new process starts with no active override.
- Monitor-loop integration: the existing Phase 1 loop delivers a real Phase 4 observation without any extra collector or thread.
- Resource Intelligence UI fixture test for current metric display, user confirmation, and action disabling without persistent history.

