# Phase 4 completion report — evidence-based status

**Overall status: PARTIAL.** The Phase 4 statistical analysis, anomaly detection, recommendations, guarded policy path, SQLite migration, Tauri IPC and Resource Intelligence interface are implemented and merged to `main`. Engine overhead is measured on both the Linux logic path and natively on Windows; the native build, tests, installers, app launch **and a 3-case native UI end-to-end suite** all pass on a GitHub Actions Windows runner; the four shipped CUDA kernels are validated on a real RTX 2050; and forecast calibration plus the anomaly false-positive rate are measured on a controlled CI workload. This report still does not label Phase 4 fully accepted: signed-app verification needs a code-signing certificate, and representative desktop-workload calibration, installed-app footprint/cold start and non-NVIDIA GPU runtime remain unmeasured.

| Acceptance area | Status | Evidence / limitation |
| --- | --- | --- |
| Phase 1–3 audit and reuse | COMPLETE | [Progress audit](PHASE4_PROGRESS_AUDIT.md); existing monitor, memory engine and GPU profiler/scheduler reused. |
| Correlated system intelligence | COMPLETE for available metrics | Same timestamp ties available CPU, RAM/pressure, GPU/VRAM, storage and process context. Missing provider counters remain unavailable. No independently measured disk latency or per-process GPU attribution. |
| Historical analysis/persistence | COMPLETE for Phase 4 schema | Additive migration 5; 15-second observations and bounded retention; legacy GPU settings tested across migration/reopen. |
| Statistical prediction | COMPLETE for implemented metrics; calibration measured on a controlled workload | Rolling mean/exponential/trend walk-forward selection and insufficient-data/uncertainty behavior are unit tested. Two 40-observation CI runs: MAE 4.264/4.285, MAPE 51.3%/27.0%, interval coverage 95.2%/100% (21 matured 1-minute forecasts each). Results vary with host conditions; representative desktop workloads and 5/15-minute horizons unmeasured. |
| Anomaly detection | COMPLETE for tested signals; false-positive rate measured but variable | Thresholds, robust baseline, hysteresis, severity and interrupted-data handling are covered by tests. Controlled idle runs raised 2 anomalies in one run and 0 in another (both over 80 samples) — the detector is **not** guaranteed false-positive-free. Not security or malware detection. |
| Recommendations | COMPLETE for implemented rules | Evidence, suggested action, qualitative possible benefit, risk, confidence and approval requirement. GPU workload guidance reads Phase 3; it does not submit work. |
| Adaptive policy safety | COMPLETE for tested supported action | Only temporary FluxCore detailed-process scan interval; analysis-only default, explicit adaptive consent, cooldown, pressure guard, expiry, audit and rollback. Native Windows UX is covered by the automated UI E2E suite. |
| Outcome evaluation | PARTIAL | Directly measures owned scan cost and compares before/after only when sample coverage and workload context permit; cannot prove system-wide causality. |
| Frontend and Tauri | COMPLETE in build/IPC/unit/E2E scope | 50 frontend tests, production build and lint pass. Three native UI E2E cases pass: collector online, sidebar navigation to Resource Intelligence, and toggling the monitoring switch both ways. |
| Regression tests | COMPLETE on WSL and native Windows test paths | **137 passed, 0 failed** under WSL (106 library + 31 integration). The same sources additionally execute natively on Windows/MSVC on CI: 106 library tests pass, plus the integration, GPU provider/regression/safety and scheduler targets. Linux and Windows GNU all-target checks pass (see [test report](PHASE4_TEST_REPORT.md)). |
| Phase 4 resource overhead | COMPLETE on Linux and native Windows | Linux logic path: 0.78–0.94 ms per 15 s sample, gated tick < 1 μs. **Native Windows: 4.64 ms per 15 s sample, of which SQLite is 4.60 ms (99.2%)**; gated tick < 1 μs; benchmark private 8.3 MiB / working set 17.3 MiB. Absolute cost ~0.03% of one core. The SQLite-dominated Windows ratio must not be reported using the Linux figure. Installed-app working set and cold start remain unmeasured. |
| Native Windows build/runtime/install | **VERIFIED on CI** | Smart App Control is still enabled on the developer machine (os error 4551 blocks unsigned binaries there, including tauri build scripts) and no security setting was changed. On a `windows-latest` runner the MSI and NSIS installers build, the benchmark runs, the release app launches, and WebdriverIO drives the real UI through an embedded WebDriver server. Signed-app verification is still outstanding (certificate required). |
| CUDA kernels on real hardware | **VERIFIED** | All four shipped PTX kernels JIT-loaded through `nvcuda.dll` on an RTX 2050 (driver 591.59, CUDA 13.1) with the Rust input generator and launch geometry reproduced: all pass 1e-3 tolerance, free VRAM identical before/after. See [PHASE4_CUDA_VERIFICATION.md](PHASE4_CUDA_VERIFICATION.md). AMD/Intel adapters ship no compute kernels (identification only). |

## Branch topology

Phase 1–4 work is committed and pushed on `main`; the native UI E2E landed via PR #5 and this
verification pass is proposed in PR #6. The original development branch
`feature/phase4-resource-intelligence` was based on `feature/phase3-gpu-acceleration` (`49c902b`),
which descends from `main`. `backup/phase3-june-200` is content-identical to the Phase 3 branch and
holds no unique work. `git for-each-ref` lists no further local/remote branches and `git stash list`
is empty, so no branch or stash holds additional Phase 4 work.

## Known limitations

- Predictions extrapolate recent samples over up to 15 minutes and carry heuristic uncertainty, not calibrated likelihoods. A changing workload invalidates the trend. Calibration was measured on a controlled idle CI VM, not a representative desktop.
- Existing history cannot be treated as correctly keyed per adapter/volume for Phase 4 forecasts, so Phase 4 starts a distinct history rather than reinterpret legacy aggregates.
- Adaptive optimization is intentionally narrow. It does not tune GPU settings, RAM/pagefile behavior, Windows processes, system settings or third-party software.
- Optional OS metrics, device temperatures and per-process hardware attribution remain subject to provider support.
- Overhead figures are measured, but the two paths differ sharply: the Linux ratio (~13% of the Phase 1+2 pipeline) is not a Windows number, where SQLite persistence is 99.2% of a 4.64 ms sample. Installed-app working set, cold start and long-run behaviour remain unmeasured.
- Native Windows execution is policy-blocked on the developer's machine: VS Build Tools 2022 and Windows SDK 10.0.26100 are installed and native compilation passes, but Smart App Control rejects the unsigned app, library harness, newly built scripts and tauri's build-script executables (os error 4551). No security setting was changed and SAC remains enabled by choice. Those steps run on a hosted Windows runner instead. Intermittent proc-macro artifact-resolution errors during partial local rebuilds are build-environment conditions, not source defects.
- AMD and Intel GPUs provide device identification only — no compute kernels are shipped, and wgpu/DX12 runtime behaviour on those adapters is unverified.
- Signed-app verification requires a code-signing certificate. The workflow is wired and inert until `WINDOWS_CERT_PFX_BASE64` and `WINDOWS_CERT_PASSWORD` exist; the signed path itself is untested.

Run status, suite details, warnings and unmeasured items are recorded in [PHASE4_TEST_REPORT.md](PHASE4_TEST_REPORT.md). No Phase 5 work was started.
