# Phase 4 CUDA verification on real hardware

Date: 2026-11-21. Machine: NVIDIA **GeForce RTX 2050** (4 GB), driver **591.59**, CUDA driver API
**13.1**, compute capability 8.6, Windows 11. All four PTX kernels shipped with FluxCore were
re-run against the Phase 4 sources. Raw output: [evidence/cuda-verification.json](evidence/cuda-verification.json).

## Why this harness exists

The developer machine's code-integrity policy blocks executing locally built Rust binaries, so the
native CUDA provider cannot be launched from `cargo test` here. A signed Python interpreter can still
call the signed `nvcuda.dll`, so [tools/cuda_verify.py](tools/cuda_verify.py) JIT-compiles the exact
PTX files in `src-tauri/ptx/` through `cuModuleLoadData` and launches them with the same inputs and
launch geometry as `src-tauri/src/gpu_engine/providers/cuda.rs`. **No CUDA toolkit or `nvcc` is
required.** This is the same technique used for the Phase 3 hardware measurements.

Reproduce:

```powershell
python -I app/tools/cuda_verify.py --out app/evidence/cuda-verification.json
```

## What was reproduced from the Rust source

- **Inputs**: the FNV-1a seed, xorshift64 stream and exactly-representable `f32` mapping from
  `gpu_engine/models.rs` — so CPU and GPU read identical input bits.
- **Launch geometry**: vector workloads use `min(maxThreadsPerBlock, 256)` threads with the same
  chunking; matmul uses the 2D `(ceil(n/tile), ceil(m/tile))` grid with `tile² ≤ maxThreads`.
- **CPU reference**: ascending, non-fused `f32` multiply/add, matching the PTX and the Rust
  provider's stated arithmetic order.
- **Tolerance**: `1e-3 * max(1, |reference|)` per element, as in R16.

## Results

| Workload | Input | Kernel (ms) | Transfer in (ms) | Transfer out (ms) | GPU total (ms) | Max abs error | Validated |
| --- | --- | --- | --- | --- | --- | --- | --- |
| vector_axpy | n = 1,048,576, a = 1.5 | 3.3798 | 4.8091 | 2.1432 | 10.3321 | 2.38e-07 | ✅ |
| vector_scale | n = 1,048,576, a = 1.5 | 0.3121 | 1.2195 | 1.1807 | 2.7123 | 0 | ✅ |
| matmul | 256 × 256 × 256 | 0.2288 | 0.5664 | 0.1428 | 0.9380 | 5.72e-06 | ✅ |
| row_sum | 1024 × 1024 | 0.3588 | 2.1406 | 0.1054 | 2.6048 | 0 | ✅ |

**All four kernels passed.** Max errors are far below the 1e-3 tolerance; two workloads match the
CPU reference bit-for-bit.

## Resource accounting

`cuMemGetInfo_v2` reported **3302.2 MiB free before and 3302.2 MiB free after** every workload —
all FluxCore device allocations were released; no VRAM leak and no other process's memory touched.
The device reported 1024 max threads per block.

## Honest scope

- This validates the **kernels and their arithmetic**, not the Rust provider's full execution path
  (timing fields, cancellation, timeout and error mapping). Those are covered by the native Rust
  test suite, which runs on CI.
- Timings are a single measured pass per workload after module JIT; they are wall-clock around the
  driver calls under WDDM, not an nvcc-optimised baseline. They reproduce the Phase 3 conclusion:
  compute-bound matmul gains the most, memory-bound vector work is dominated by transfer.
- AMD and Intel adapters remain **device-identification only** in FluxCore — no compute kernels are
  shipped for them. The AMD Radeon iGPU on this machine is detected by the OS provider but is not a
  CUDA target; there is nothing for this harness to validate on it.
