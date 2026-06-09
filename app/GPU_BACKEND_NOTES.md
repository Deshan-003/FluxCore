# Phase 3 — GPU backend implementation notes (Subagent 2)

Backend owner: Subagent 2 (GPU Acceleration Backend Engineer).
Contracts: `PHASE3_CONTRACT_FREEZE.md` §4/§5, `PHASE3_ARCHITECTURE.md` §8.
Target: Windows / NVIDIA RTX 2050 (sm_86, driver 591.59), `nvcuda.dll` + `nvml.dll`
present, **no CUDA toolkit / no nvcc / no CUDA_PATH**.

## Files created / owned

| File | Purpose |
| --- | --- |
| `src-tauri/src/gpu_engine/providers/mod.rs` | `GpuProvider` trait, `all_providers`, `initialize_all`, `backend_for_provider`, `provider_for_backend`, shared `validate_gpu` / `check_interrupt` / `build_outcome` helpers, re-exports |
| `src-tauri/src/gpu_engine/providers/cpu.rs` | CPU reference + fallback; all 4 workloads via chunked `WorkloadParams::run_cpu` sub-problems with cancel/timeout checkpoints |
| `src-tauri/src/gpu_engine/providers/cuda.rs` | `nvcuda.dll` driver-API FFI, primary-context, PTX JIT, chunked launches, `cuGetErrorString` mapping |
| `src-tauri/src/gpu_engine/providers/wgpu.rs` | wgpu 30 DX12/Vulkan/WGSL provider, pipeline compile + readback + device-loss recovery |
| `src-tauri/src/gpu_engine/capability.rs` | `detect_providers`, `environment`, `status_of` |
| `src-tauri/src/gpu_engine/providers/wgsl/{axpy,scale,matmul,rowsum}.wgsl` | 4 WGSL compute kernels (`@workgroup_size(64)`, shared 4-binding layout, grid-stride loops) |
| `src-tauri/ptx/{axpy,scale,matmul,rowsum}.ptx` | 4 hand-written `sm_75`, `.address_size 64`, PTX ISA 7.0 kernels |

## CUDA entry points resolved (`libloading::Library::get`, `extern "C"`, CUresult→i32)

`cuInit`, `cuDriverGetVersion`, `cuDeviceGetCount`, `cuDeviceGet`,
`cuDeviceGetName`, `cuDeviceGetAttribute`, `cuDevicePrimaryCtxRetain`,
`cuCtxSetCurrent` (fallback `cuCtxSetCurrent_v2` → `cuCtxPushCurrent_v2`),
`cuMemAlloc_v2`, `cuMemFree_v2`, `cuMemcpyHtoD_v2`, `cuMemcpyDtoH_v2`,
`cuMemGetInfo_v2`, `cuModuleLoadData`, `cuModuleGetFunction`, `cuLaunchKernel`,
`cuCtxSynchronize`, `cuGetErrorString`, `cuDevicePrimaryCtxRelease` (optional,
`_v2` fallback). Device attributes: CC major=75, minor=76, max threads per
block=1. Handles are stored as `usize` so the driver state is `Send` behind a
`Mutex` (required by `Arc<dyn GpuProvider>`). **No NVML calls** (Phase 1 owns
telemetry). A single primary context on device 0 is retained; it is re-set
