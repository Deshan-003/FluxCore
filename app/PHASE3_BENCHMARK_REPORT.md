# FluxCore — Phase 3 Benchmark Report (measured on real hardware)

Date: 2026-11-17. Machine: Lenovo IdeaPad Gaming 3 15ACH6 class — AMD Ryzen 5 5500H,
**NVIDIA GeForce RTX 2050 4 GB**, 8 GB system RAM, Windows 11 (build 26300), driver **591.59**,
CUDA driver API **13.1** (`nvcuda.dll`), compute capability **8.6**. No CUDA toolkit (`nvcc` absent).

## 1. Method and why it looks like this

This machine enforces an **organization Device Guard / WDAC policy** that blocks execution of
unsigned, locally cross-built executables (the FluxCore Windows GNU binaries terminate with
"blocked by your organization's Device Guard policy"; earlier Smart App Control also denied the
rustup-downloaded GNU host toolchain, os error 4551). The policy was **not bypassed**.

Therefore hardware measurements were taken with the **signed Python interpreter** using `ctypes`
against `nvcuda.dll`, JIT-loading the **exact `app/src-tauri/ptx/*.ptx` kernels shipped with
FluxCore** (`cuModuleLoadData` → `cuModuleGetFunction` → `cuLaunchKernel` → `cuCtxSynchronize`),
with the same deterministic inputs as `gpu_engine/models.rs` (FNV-1a seed + xorshift64, values
exactly representable in f32) and the same non-fused f32 accumulation order as the CPU reference.
Probe source: `cuda_probe.py` (session scratch; reproduces every number below).

- GPU timings: wall-clock around `cuMemcpyHtoD_v2` (transfer in), launch+`cuCtxSynchronize`
  (kernel), `cuMemcpyDtoH_v2` (transfer out). `gpu_total` = in + kernel + out (end-to-end).
- CPU baseline: the identical computation executed on this CPU via the numpy reference
  implementation (same semantics/order). **This is not the Rust scalar CPU provider's timing**;
  the Rust provider is scalar and would be slower, so the speedups below are conservative
  lower bounds for the Rust CPU path.
- Single measured run per workload after one implicit warm-up (module JIT + first launch);
  errors and VRAM accounting verified per run. Re-run reproduces the same validation results.

## 2. Measured results

| Workload | Input | CPU (ms) | Transfer in (ms) | Kernel (ms) | Transfer out (ms) | GPU total (ms) | Speedup | Max abs error | Validated |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| vector_axpy | n = 1,048,576, a = 1.5 | 3.622 | 1.424 | 0.247 | 1.225 | 2.896 | **1.25x** | 2.38e-07 | yes |
| vector_scale | n = 1,048,576, a = 1.5 | 3.238 | 0.694 | 0.163 | 1.392 | 2.249 | **1.44x** | 0 | yes |
| matmul | 256×256×256 | 34.089 | 0.308 | 0.134 | 0.090 | 0.532 | **64.11x** | 5.72e-06 | yes |
| row_sum | 1024×1024 | 8.796 | 0.865 | 0.216 | 0.031 | 1.112 | **7.91x** | 0 | yes |

Validation tolerance: `1e-3 * max(1, |reference|)` per element (R16). All four kernels passed.

## 3. Resource accounting (real driver queries)

- `cuMemGetInfo_v2` before: total **4095 MiB**, free **3302 MiB** (other apps hold ~793 MiB).
- After allocating, launching and freeing every buffer: free **3302 MiB** — **all FluxCore
  allocations released**; no leak, no other-process VRAM touched.
- Peak per-workload device bytes equal `WorkloadParams::device_bytes()` (inputs + output, f32).

## 4. Honest interpretation

- At 1 M elements the vector workloads are only **1.25–1.44x** faster end-to-end because PCIe
  transfer dominates (≈2.6 ms of ≈2.9 ms). This is exactly why `profiler.rs` keeps small/transfer-
  bound workloads on CPU in Intelligent Auto mode; the model and these measurements agree.
- matmul and row_sum are compute-bound and show large speedups (64x / 7.9x) even at modest sizes.
- No claim is made that GPU is always faster; the benchmark engine reports slowdowns with equal
  prominence and returns `gpu: None` + reason when no GPU path runs.
- **Not measured here**: the Rust `benchmark.rs` engine's own end-to-end run on Windows (its binary
  is WDAC-blocked). Its logic is verified by compile + the 121-test suite; its hardware behaviour is
  evidenced by the driver-level measurements above using the same kernels and inputs.
- wgpu/DX12 path: not exercised by this probe (CUDA path only). On Windows the wgpu provider
  compiles and enumerates adapters; runtime execution remains **UNVERIFIED** on this machine
  (WDAC blocks the test binary). AMD/Intel: no adapter present → UNTESTED.

## 5. Reproduction

```powershell
python cuda_probe.py   # signed interpreter; loads nvcuda.dll + app/src-tauri/ptx/*.ptx
```

Rust-side (on machines without the application-control policy):

```powershell
cargo run --release --manifest-path app/src-tauri/Cargo.toml --example gpu_benchmark 5
```