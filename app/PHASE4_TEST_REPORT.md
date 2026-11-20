# Phase 4 test and verification report

Date: 2026-11-20. Phase 4 is developed on `feature/phase4-resource-intelligence`, based on the
existing `feature/phase3-gpu-acceleration` commit `49c902b`, and merged to `main` via pull request #3
(merge commit `b20f71c`). Verification now runs on two paths: the WSL **Linux logic path**, and
natively on **Windows/MSVC** through the `Native verification (Windows)` GitHub Actions workflow
(`.github/workflows/native-verification.yml`). The workflow exists because Smart App Control blocks
executing unsigned binaries on the developer's own machine (os error 4551); a hosted runner carries no
such code-integrity policy, so the same binaries build, run and launch there.

## Executed tests and builds

| Test / build | Result |
| --- | --- |
| Final full Rust suite (`cargo test --locked`, WSL Ubuntu) | **137 passed, 0 failed**: 106 library + 8 GPU provider + 3 Phase 1/2 regression + 9 GPU safety + 10 scheduler + 1 Phase 4 integration. Includes forecasting, robust baselines, sustained anomaly/hysteresis, confounder-aware outcomes, policy bounds, Smart Assist confirmation, adaptive opt-in, stale-sample rejection, monitor/history-off clearing, restart interruption, manual apply/rollback, SQLite persistence and the monitor-loop integration guard. |
| Native Rust suite (`cargo test --locked`, Windows/MSVC, CI) | **PASS**. The library target executes natively and all 106 library tests pass, together with the integration, GPU provider/regression/safety and scheduler targets. See "Native Windows" below. |
| Rust all-target compile | **PASS** for Linux and `x86_64-pc-windows-gnu`, both `cargo check --locked --all-targets`. Includes Windows-gated collector code. |
| Phase 4 overhead benchmark (`examples/intelligence_benchmark.rs`, release) | **Executed** on WSL (twice) and **natively on Windows** (CI). Both result sets below. |
| Frontend (native Windows, Vitest) | **50 passed, 0 failed across 7 files**, including 2 Phase 4 tests. Existing chart/jsdom size and `act(...)` warnings appear on stderr; tests pass. |
| Frontend production build (`npm --prefix app run build`) | **PASS**, `tsc -b` and Vite production build, 2167 modules. Vite reports event-import chunking and >500 kB bundle warnings; output generated. |
| Frontend lint (`npm --prefix app run lint`) | **PASS**, ESLint exits 0. Generated Rust build output is excluded. |
| Windows bundles (`npm run tauri build`, Windows/MSVC, CI) | **PASS** — MSI and NSIS installers built and uploaded as `fluxcore-windows-bundles`. |
| Native app smoke launch (Windows/MSVC, CI) | **PASS** — the release `fluxcore.exe` is launched, stays alive for 20 s and is terminated cleanly; the step fails on an early exit (reporting the exit code in hex) or a startup panic. |
| `git diff --check` | **PASS**. |

### Test-run reconciliation

Earlier runs reported 131 tests (before the final policy and integration tests were added) and
104–106 library-only tests. The table above is the single final full-suite run after all Phase 4 tests
were added; it supersedes those intermediate counts. An accidentally duplicated WSL cargo invocation
was stopped before its test execution; its exit 15 was not a test failure and is not counted. The
Windows GNU `cargo check --all-targets` passes on the final source.

## Phase 4 overhead (measured)

### Linux logic path

Harness: `app/src-tauri/examples/intelligence_benchmark.rs`, release build, WSL Ubuntu, two independent runs.
Metric values come from the live OS collector; only the snapshot timestamp is shifted by the 15-second
cadence so every iteration performs a full sample instead of the early return. Each pass ran 20
in-memory samples and 20 persisted samples.

| Stage | Run A | Run B |
| --- | --- | --- |
| Phase 1 collector pass | 6,563 μs | 5,951 μs |
| Phase 2 memory engine tick | 50 μs | 48 μs |
| Phase 4 analysis only (forecast, anomaly, findings) | 164 μs | 97 μs |
| Phase 4 full sample including SQLite | 939 μs | 782 μs |
| Persistence delta (SQLite transaction) | 775 μs (82.5%) | 685 μs (87.6%) |
| Phase 4 gated tick inside the 15 s window | < 1 μs | < 1 μs |
| Phase 4 share of the Phase 1+2 pipeline per sample | 14.2% | 13.0% |
| SQLite file after 40 observations | 4.0 KiB | 4.0 KiB |
| Benchmark process resident footprint | 38.4 MiB | 38.4 MiB |

Interpretation: a full Phase 4 sample costs **0.78–0.94 ms** and runs every 15 seconds, i.e. roughly
**0.05–0.06 ms per second of monitoring (about 0.006% of one core)**. Monitoring ticks inside the
sampling window pay less than one microsecond. SQLite persistence dominates the sample; the analysis
itself is 0.10–0.16 ms. These figures come from the Linux logic path, where GPU and storage-I/O
collectors are stubbed.

### Native Windows (CI, 2026-11-20)

Same harness, release build, executed on a `windows-latest` runner against the live Windows collectors.
20 sampled observations per pass.

| Stage | Native Windows |
| --- | --- |
| Phase 1 collector pass | 1,872 μs |
| Phase 2 memory engine tick | 162 μs |
| Phase 4 analysis only (forecast, anomaly, findings) | 39 μs |
| Phase 4 full sample including SQLite | **4,640 μs** |
| Persistence delta (SQLite transaction) | 4,601 μs (**99.2%**) |
| Phase 4 gated tick inside the 15 s window | < 1 μs |
| Phase 4 share of the Phase 1+2 pipeline per sample | **228.1%** |
| SQLite file after 40 observations | 4.0 KiB |
| Benchmark process footprint | private 8.3 MiB · working set 17.3 MiB · peak WS 20.4 MiB |

Interpretation: a full native sample costs **4.64 ms** every 15 seconds, i.e. about **0.31 ms per
second of monitoring (~0.03% of one core)**. The absolute cost is still negligible, but the *shape*
differs sharply from Linux: SQLite persistence is **99.2%** of the sample on Windows, so the "~13% of
the Phase 1+2 pipeline" figure from the Linux run **must not be presented as a Windows number**. (The
working-set figure is the benchmark process, not the installed app; installed-app working set and
cold-start cost remain unmeasured.)

## Native Windows build and runtime (2026-11-20)

The user authorized installing the native toolchain, so Visual Studio 2022 Build Tools 17.14.45
(MSVC 14.44.35207, Windows SDK 10.0.26100) was installed via winget. WebView2 runtime was already
present.

On the developer machine, Smart App Control
(`HKLM\SYSTEM\CurrentControlSet\Control\CI\Policy\VerifiedAndReputablePolicyState = 1`) blocks
executing the unsigned binaries with os error 4551, and — since the policy tightened — it also blocks
tauri's *build-script* executables, so the local MSVC test/exe path can no longer be executed at all.
Per the user's explicit decision on 2026-11-20, Smart App Control was **left enabled**; no exclusion,
code-signing or other security change was made. Verification therefore moved to a hosted runner.

| Native step | Where | Result |
| --- | --- | --- |
| MSVC compile/link, debug (`cargo build`, `cargo test --no-run`) | local | **PASS** — library, all six test targets and the app binary (29.3 MB) were produced |
| `intelligence_integration_tests` (monitor loop, real Windows collectors) | local, before enforcement | **PASS** — executed natively, exit 0 |
| `fluxcore_lib` test harness | local | **FAILED TO EXECUTE** — `STATUS_ENTRYPOINT_NOT_FOUND` (0xC0000139); blocked with os error 4551 after enforcement |
| App launch (`fluxcore.exe`) | local | **BLOCKED** — "An Application Control policy has blocked this file" (os error 4551) |
| MSI/NSIS installer | local | **BLOCKED** — os error 4551 |
| Native Phase 4 benchmark run | local | **BLOCKED** — os error 4551 |
| `fluxcore_lib` test harness (106 library tests) | CI `windows-latest` | **PASS** — executed natively, all 106 pass |
| Integration / GPU provider / safety / scheduler targets | CI `windows-latest` | **PASS** |
| App launch (`fluxcore.exe`) | CI `windows-latest` | **PASS** — launched, alive after 20 s (`Smoke launch OK`) |
| MSI + NSIS installer | CI `windows-latest` | **PASS** — both bundles built and uploaded |
| Native Phase 4 benchmark | CI `windows-latest` | **PASS** — see the native table above |

### The `fluxcore_lib` load failure (0xC0000139) — resolved

The local `STATUS_ENTRYPOINT_NOT_FOUND` was **not** a SAC artifact. It is a known Tauri issue
([tauri-apps/tauri#13419](https://github.com/tauri-apps/tauri/issues/13419),
[#13948](https://github.com/tauri-apps/tauri/issues/13948),
[#14580](https://github.com/tauri-apps/tauri/issues/14580)): `tauri-build` embeds the Common Controls
v6 application manifest into the **app binary only**, so test and example link targets get no manifest,
Windows loads Common Controls **v5**, a required import is missing and the process dies at load time
(not a test failure). Fixed by embedding the manifest into every Windows/MSVC link target: `build.rs`
calls `tauri_build::WindowsAttributes::new_without_app_manifest()` and emits
`cargo:rustc-link-arg=/MANIFEST:EMBED` plus `/MANIFESTINPUT:<windows-app-manifest.xml>` (the same
manifest content tauri-build normally uses, so the app binary is unchanged). The logic is gated on
`CARGO_CFG_TARGET_OS=windows` + `CARGO_CFG_TARGET_ENV=msvc`, leaving Linux and the windows-gnu check
path untouched. Commit `067c85f`.

A second, genuine issue surfaced once the suite could run: `monitor_engine_starts_publishes_and_stops`
timed out because the monitor loop's first collection performs one-off OS probing — a sysinfo refresh,
an NVML probe and a `powershell.exe Get-CimInstance Win32_VideoController` GPU enumeration — which
exceeded the test's fixed 2 s budget on a cold runner. The test now waits on a 30 s wall-clock
deadline (`a6bb95d`). The probe itself was then bounded too, because an unbounded WMI call could stall
first startup indefinitely: `windows_gpu_names()` now polls the child with an 8 s deadline, kills it on
timeout and falls back to NVML-only GPU data (`8c35d64`). With that bound the suite runs in ~8 s
instead of a 42.7 s outlier, and a hung WMI query can no longer freeze the app's first sample.

A separate build-environment issue was observed during partial local rebuilds: intermittent
`E0463 can't find crate for <proc-macro>` for `document_features`, `tauri_macros` and
`zerofrom_derive` ("Rejecting via proc macro: expected false got true", i.e. a stale/ambiguous
artifact). This is an artifact-resolution condition, not a source defect — the same sources build and
pass on both WSL and CI.

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
- Windows-gated collector helpers (CIM entry parsing, drive classification, GPU class inference, process CPU normalisation, rate derivation) execute natively on CI.

Synthetic series are used only in unit tests, never in production data or the shipped dashboard.

## Not verified / measured

- An interactive native Windows Tauri UI session and signed-app verification remain unexecuted. The
  app binary is verified to *start* and stay alive on CI, but no one drives the UI end to end.
- Installed-app working set, cold-start cost and long-run behaviour are unmeasured. (Native per-sample
  overhead itself is now measured — see above.)
- Phase 3 reported CUDA driver-level results apply to its earlier code and were not re-run against this
  Phase 4 branch. AMD/Intel, wgpu/DX12 runtime, and new Phase 4 GPU telemetry remain unverified here.
- Real-world prediction accuracy, calibration, and anomaly false-positive rate remain unmeasured.

## Evidence-based status

- Analysis, statistical forecasting, anomaly rules and recommendations: **COMPLETE for tested implementation; real-world calibration UNVERIFIED**.
- Safe adaptive optimization path: **COMPLETE for tested backend path; native Windows UX unverified** (no interactive UI session).
- UI, IPC and SQLite persistence: **COMPLETE for type/build/unit, Linux SQLite and native Windows test scope; interactive installed-app E2E UNVERIFIED**.
- Outcome evaluation: **PARTIAL** (measures FluxCore scan cost; reports inconclusive when conditions change; never claims overall system speedup).
- Phase 4 resource-overhead measurement: **COMPLETE** on both paths — Linux 0.78–0.94 ms per 15 s sample; native Windows 4.64 ms per 15 s sample (99.2% SQLite); gated tick < 1 μs on both.
- Native Windows build, test, bundle and launch: **VERIFIED on CI** (`windows-latest`). Executing them on the developer's own machine remains blocked by Smart App Control, which is still enabled by choice; no security setting was changed.
- Overall acceptance: **PARTIAL** — the native Windows build/runtime/installer/benchmark blockers are closed; what remains is real-workload calibration and false-positive measurement, an interactive UI session, signed-app verification, and the GPU-lab re-runs.
