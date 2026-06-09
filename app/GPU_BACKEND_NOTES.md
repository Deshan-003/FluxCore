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
current per worker thread at execute time (CUDA contexts are per-thread).

## PTX kernels

All four are embedded with `include_str!("../../../ptx/*.ptx")` (three `..`
levels from `providers/` — verified by rustc). Each is passed to
`cuModuleLoadData` as a NUL-terminated string and JIT-compiled by the driver:
- `axpy` — `out=a*x+y`, non-fused `mul.f32`/`add.f32` to match CPU.
- `scale` — `out=a*x`.
- `matmul` — 2D grid (no integer division), ascending-`t` accumulation.
- `rowsum` — one thread per row, ascending-column accumulation.

**Which ones "load":** at `initialize()` the provider attempts all four. A
workload is added to `supported_workloads()` **only if its module actually
loads**; a failure is recorded as an honest `GpuError::Unsupported` reason from
`cuGetErrorString` and never advertised. This is verified at runtime on the
target GPU, not here. Vector launches are chunked to ≤1,000,000 elements with a
cancel/timeout checkpoint between chunks (TDR safety).

## WGSL kernels

All four compiled at `initialize()` with `create_shader_module` +
`create_compute_pipeline` behind one shared bind-group layout
(binding 0 `x` read-storage, 1 `y` read-storage, 2 `out` read-write storage,
3 `Params` uniform: `p0,p1,p2:u32, a:f32`, 16 bytes). Only pipelines that
compile without an uncaptured validation error are advertised. An
`on_uncaptured_error` handler captures validation/device-loss text instead of
panicking; a real error drops the device so the next attempt re-initialises.
Readback uses callback-based `map_async` + bounded
`device.poll(PollType::Wait { submission_index, timeout })`, per wgpu 30.

## What was actually verified

- Every wgpu 30 signature used was read from the cached crate source
  (`Instance::new` by value, `enumerate_adapters`, `request_device`,
  `PollType::Wait { submission_index, timeout }`, `queue.submit → SubmissionIndex`,
  callback `map_async`, `BufferView: Deref<[u8]>`, descriptors, `Limits` fields).
- All 5 Rust files **parse** (`rustfmt --check`, syntax only).
- All 5 Rust files **type-check cleanly, exit 0, zero warnings** against a
  standalone harness that stubs the external crate APIs with those exact
  signatures and strips the serde derives from copies of `models.rs`/`error.rs`.
  This also verified every `include_str!` path resolves and that all three
  providers satisfy `GpuProvider: Send + Sync`.

## Honest limitations / what is unsupported

- `cargo check --manifest-path app/src-tauri/Cargo.toml` **could not be run to
  completion in this session**: the machine has no MSVC linker/import libs and a
  system-wide Windows "Application Control policy" (os error 4551) blocks
  executing freshly built build scripts / loading proc-macro DLLs. This is an
  environment constraint, not a code error; the standalone `rustc
  --emit=metadata` harness above was used instead. PTX/WGSL JIT compile and
  real GPU execution were therefore **not** runtime-exercised here.
- CUDA PTX is hand-authored for `sm_75`; pre-`sm_75` devices would fail JIT
  (reported honestly as `Unsupported`), newer devices JIT it forward.
- No workload is reported as supported unless its real kernel module loaded /
  compiled; execution never returns success without running the kernel.
- wgpu reports no VRAM free/total (the API does not expose it) —
  `GpuMemoryInfo`/`gpu_vram_bytes` stay `None`/`0` for that path; CUDA reports
  real `cuMemGetInfo_v2` values.
- `cuda_runtime_api_present` is inferred from toolkit presence (cudart ships
  with the toolkit); `vulkan_present`/`dx12_present` reflect adapters actually
  discovered by wgpu, never hardcoded `true`.
