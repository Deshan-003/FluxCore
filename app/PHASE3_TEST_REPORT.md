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
- Safety: every §5.4 config range rejected; percent-semantics default; idempotent reserve/release;
  admission denied with zero budget; task/benchmark/config/workload-settings/provider round-trips;
  migrations 1–4 additive and idempotent across reopen with legacy rows preserved.
- Regression: Phase 1 snapshot and Phase 2 intelligence still work with the GPU engine constructed.

Linux-host caveat: inside WSL there is no NVIDIA driver and no DX12/Vulkan adapter, so the `cuda` and
`wgpu` providers report unavailable there (honestly). Real-driver GPU execution on this Windows
machine is verified separately (§5).

## 3. Frontend — native Windows

| Command | Result |
| --- | --- |
| `npm --prefix app run build` (`tsc -b && vite build`) | **PASS** — 2164 modules, dist emitted |
| `npm --prefix app run test:frontend` (vitest) | **PASS — 48/48 tests, 6 files** (incl. 12 new Phase 3 component tests) |
| `npm --prefix app run lint` (eslint) | **PASS — 0 errors, 0 warnings** |

Notes:
- Two lint errors were fixed: pre-existing `App.tsx:55` `Date.now()` in render (`react-hooks/purity`),
  and a setState-in-effect in the new page (refactored to derived draft state).
- jsdom lacks `ResizeObserver`; the Phase 3 component test installs a standard stub (test-only).
- An earlier vitest run reported "no tests / 6 errors" only when a concurrent WSL cargo build exhausted
  host RAM; run alone it is fully green. Recorded here so the flake is not mistaken for a code defect.

## 4. Bugs discovered and fixed during verification

| # | Bug | Location | Fix |
| --- | --- | --- | --- |
| 1 | non-existent wgpu 30 API blocked whole-crate compile | `providers/wgpu.rs:403` | `new_without_display_handle_from_env()` |
| 2 | CPU provider advertised caps but never enforced them at admission | `providers/cpu.rs::can_execute` | enforce `max_elements` |
| 3 | **Scheduler self-deadlock**: `submit` held the shared mutex while `ensure_workers` re-locked it | `gpu_engine/scheduler.rs` | pass CPU cap into `ensure_workers` |
| 4 | VRAM budget = 0 whenever the monitor sees no GPU (Linux/NVML-absent), denying all GPU admission | `gpu_engine/resources.rs`, `mod.rs` | provider-reported VRAM fallback (`set_device_vram_total`) |
| 5 | regression tests assumed engines ready on first call | `tests/gpu_regression_tests.rs` | bounded readiness polling; monitor drives memory engine |
| 6 | test-only move error (`for id in ids` twice) | `tests/gpu_scheduler_tests.rs` | iterate by reference |
| 7 | pre-existing render-purity lint error | `src/App.tsx:55` | anchor chart window to newest sample |
| 8 | setState-in-effect lint error in new page | `src/pages/GpuAcceleration.tsx` | derived `draftOverride ?? config` |

## 5. Real-GPU native verification (Windows, RTX 2050)

Cross-build succeeded: `cargo build --release --target x86_64-pc-windows-gnu --example gpu_benchmark`
(WSL, mingw-w64 linker). **Native execution of the cross-built binary is BLOCKED by this machine's
organization Device Guard / WDAC policy** ("was blocked by your organization's Device Guard policy").
A hello-world control binary from the same toolchain was likewise blocked while a pre-existing
signed binary ran, confirming the block is policy, not a toolchain defect. The policy was **not
bypassed**.

Real-driver verification was therefore performed with the **signed Python interpreter** via `ctypes`
against `nvcuda.dll`, JIT-loading the exact shipped `app/src-tauri/ptx/*.ptx` kernels with the same
deterministic inputs and accumulation order as the Rust CPU reference. Executed results (run 2,
2026-11-17):

- Device: NVIDIA GeForce RTX 2050, cc 8.6, driver API 13010 (driver 591.59), VRAM total 4095 MiB,
  free 3302 MiB before the run.
- `vector_axpy` n=1,048,576: validated=True, max_abs_error=2.38e-07, cpu=3.622 ms,
  transfer_in=1.424 ms, kernel=0.247 ms, transfer_out=1.225 ms, gpu_total=2.896 ms, speedup=1.25x.
- `vector_scale` n=1,048,576: validated=True, max_abs_error=0, cpu=3.238 ms, gpu_total=2.249 ms,
  speedup=1.44x.
- `matmul` 256×256×256: validated=True, max_abs_error=5.72e-06, cpu=34.089 ms, gpu_total=0.532 ms,
  speedup=64.11x.
- `row_sum` 1024×1024: validated=True, max_abs_error=0, cpu=8.796 ms, gpu_total=1.112 ms, speedup=7.91x.
- `ALL_KERNELS_VALIDATED=True`; free VRAM after cleanup = 3302 MiB (all allocations released).

CPU timings are the numpy reference implementation of identical semantics on the same inputs
(labelled as such in `PHASE3_BENCHMARK_REPORT.md`); the Rust scalar CPU provider would be slower.
This proves: PTX JIT via `cuModuleLoadData` works without nvcc on this driver/GPU, all four kernels
compute correctly within R16 tolerance, launch/transfer/cleanup accounting is correct, and VRAM is
fully released. It does **not** prove the Rust `gpu_benchmark` binary itself ran on Windows.

## 6. Not verified / environment-limited

- **Rust-native GPU binary execution on Windows**: WDAC-blocked (above). Logic verified via WSL test
  suite; hardware behaviour verified via the ctypes probe using identical kernels/inputs.
- **Tauri MSI/NSIS installer bundling**: requires native Windows MSVC toolchain. A winget install of
  VS Build Tools was attempted with user authorization and ended with exit 1602 (UAC/user-exit);
  it was **not** forced. Frontend production build and Rust release cross-build are verified;
  installer bundling is documented as an environment limitation, not claimed.
- **AMD/Intel GPU paths**: no such adapter on this machine → UNTESTED, no vendor claim.
- **wgpu on Windows**: adapter discovery and WGSL execution not exercised at runtime (WDAC blocks the
  test binary; the probe covers the CUDA path only) → UNVERIFIED, code compiles clean.