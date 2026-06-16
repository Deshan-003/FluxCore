# FluxCore — Phase 3 Test Report (executed results)

All results below were **actually executed** on 2026-11-17. Nothing is claimed that did not run.
Verification hosts: WSL Ubuntu (Linux, rustc 1.100.0) for Rust; native Windows (Node v24.22.1) for
frontend; native Windows real-driver GPU checks via the signed Python interpreter + `ctypes`
(cross-built Rust binaries are blocked by the machine's Device Guard policy — see §5).

## 1. Rust — compile

| Command | Host | Result |
| --- | --- | --- |
| `cargo check --manifest-path app/src-tauri/Cargo.toml --all-targets` | WSL Ubuntu | **PASS** (0 errors; 3 pre-existing `dead_code` warnings for `cfg(windows)`-only helpers on Linux) |
| same, MSVC host native | Windows | **FAIL — environment**: no MSVC `link.exe`; PATH `link.exe` is Git's hardlink tool. Not a code defect. |
| same, GNU host toolchain native | Windows | **BLOCKED — environment**: Smart App Control denies the rustup-downloaded GNU `cargo.exe`/`rustc.exe` (os error 4551). |

The initial WSL check found exactly one code error in the pre-existing Phase 3 code:
`providers/wgpu.rs:403` `InstanceDescriptor::from_env_or_default` (E0599, not a wgpu 30.0.2 API).
Fixed to `new_without_display_handle_from_env()`; check then green.

## 2. Rust — tests (`cargo test`, WSL Ubuntu)

| Target | Passed | Failed | Time |
| --- | --- | --- | --- |
| lib unittests (Phase 1 + Phase 2 + models/profiler/capability) | 91 | 0 | 0.47 s |
| `tests/gpu_provider_tests.rs` | 8 | 0 | 0.27 s |
| `tests/gpu_regression_tests.rs` | 3 | 0 | 1.48 s |
| `tests/gpu_safety_tests.rs` | 9 | 0 | 0.07 s |
| `tests/gpu_scheduler_tests.rs` | 10 | 0 | 3.79 s |
| **Total** | **121** | **0** | |

Coverage highlights (all executed):
- CPU reference correctness for all four workloads via output digest equality; deterministic inputs;
  validation tolerance accept/reject; cooperative cancel; expired timeout; unknown-workload and
  oversized-cap admission rejection.
- Scheduler: CPU-only selection (no GPU timing reported), pinned-CUDA execution with GPU timing and
  validation, GPU-failure → CPU fallback with recorded reason and `fell_back=true`, `require_gpu`
  never falls back, cooperative cancel of a running task, wall-clock timeout → `TimedOut` with
  reservation released, queue backpressure, GPU concurrency ≤ 1 observed, unknown/finished cancel
  errors, observer sees Queued→Running→Succeeded.
