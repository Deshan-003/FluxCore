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
