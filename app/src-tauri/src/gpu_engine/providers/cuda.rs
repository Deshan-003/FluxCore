//! CUDA provider — NVIDIA driver API via `nvcuda.dll`, embedded PTX, no toolkit.
//!
//! Owned by Subagent 2. Resolves documented driver-API entry points with
//! `libloading`, JIT-compiles the hand-written PTX modules at `initialize()`
//! with `cuModuleLoadData`, and only advertises a workload in
//! `supported_workloads()` when its module actually loaded. Every nonzero
//! CUDA status is converted to a `GpuError` using `cuGetErrorString`'s real
//! text. No NVML calls (Phase 1 owns telemetry); no `unwrap()` on driver calls.

use super::{
    build_outcome, check_interrupt, pick_device_id, validate_gpu, CancelToken, ExecutionOutcome,
    ExecutionRequest, GpuDeviceInfo, GpuError, GpuMemoryInfo, GpuProvider, GpuResult,
    ProviderCapabilities, ProviderKind, TimingBreakdown, WorkloadInputs, WorkloadParams,
    WorkloadRequirement, WorkloadSupport,
};
use std::ffi::{c_void, CStr, CString};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, RwLock};
use std::time::{Duration, Instant};

// PTX is embedded from `app/src-tauri/ptx/*.ptx`. From this file
// (`src/gpu_engine/providers/cuda.rs`) that is three directories up.
const PTX_AXPY: &str = include_str!("../../../ptx/axpy.ptx");
const PTX_SCALE: &str = include_str!("../../../ptx/scale.ptx");
const PTX_MATMUL: &str = include_str!("../../../ptx/matmul.ptx");
const PTX_ROWSUM: &str = include_str!("../../../ptx/rowsum.ptx");

/// (workload_id, PTX entry-point name, embedded PTX text).
const KERNELS: [(&str, &str, &str); 4] = [
    ("vector_axpy", "axpy", PTX_AXPY),
    ("vector_scale", "scale", PTX_SCALE),
    ("matmul", "matmul", PTX_MATMUL),
    ("row_sum", "rowsum", PTX_ROWSUM),
];

// CUDA driver API attributes (cuda.h CUdevice_attribute enum).
const CU_DEVICE_ATTRIBUTE_MAX_THREADS_PER_BLOCK: i32 = 1;
const CU_DEVICE_ATTRIBUTE_COMPUTE_CAPABILITY_MAJOR: i32 = 75;
const CU_DEVICE_ATTRIBUTE_COMPUTE_CAPABILITY_MINOR: i32 = 76;

/// Max elements per vector launch chunk (keeps each launch short, under TDR).
const VECTOR_CHUNK: usize = 1_000_000;

// --- FFI types -------------------------------------------------------------
// CUresult is a 32-bit status; CUdevice is an int ordinal; CUdeviceptr is a
// 64-bit device pointer on this x64 target; contexts/modules/functions/streams
// are opaque pointers. On Windows x64 `extern "C"` is the platform default ABI.

type CuInit = unsafe extern "C" fn(u32) -> i32;
type CuDriverGetVersion = unsafe extern "C" fn(*mut i32) -> i32;
type CuDeviceGetCount = unsafe extern "C" fn(*mut i32) -> i32;
type CuDeviceGet = unsafe extern "C" fn(*mut i32, i32) -> i32;
type CuDeviceGetName = unsafe extern "C" fn(*mut i8, i32, i32) -> i32;
type CuDeviceGetAttribute = unsafe extern "C" fn(*mut i32, i32, i32) -> i32;
type CuDevicePrimaryCtxRetain = unsafe extern "C" fn(*mut *mut c_void, i32) -> i32;
type CuCtxSetCurrent = unsafe extern "C" fn(*mut c_void) -> i32;
type CuMemAlloc = unsafe extern "C" fn(*mut u64, usize) -> i32;
type CuMemFree = unsafe extern "C" fn(u64) -> i32;
type CuMemcpyHtoD = unsafe extern "C" fn(u64, *const c_void, usize) -> i32;
type CuMemcpyDtoH = unsafe extern "C" fn(*mut c_void, u64, usize) -> i32;
type CuMemGetInfo = unsafe extern "C" fn(*mut usize, *mut usize) -> i32;
type CuModuleLoadData = unsafe extern "C" fn(*mut *mut c_void, *const c_void) -> i32;
type CuModuleGetFunction = unsafe extern "C" fn(*mut *mut c_void, *mut c_void, *const i8) -> i32;
type CuLaunchKernel = unsafe extern "C" fn(
    *mut c_void,             // CUfunction
    u32,
    u32,
    u32, // gridDimX/Y/Z
    u32,
    u32,
    u32, // blockDimX/Y/Z
    u32,                     // sharedMemBytes
    *mut c_void,             // CUstream
    *mut *mut c_void,        // void** kernelParams
    *mut *mut c_void,        // void** extra
) -> i32;
type CuCtxSynchronize = unsafe extern "C" fn() -> i32;
type CuGetErrorString = unsafe extern "C" fn(i32, *mut *const i8) -> i32;
type CuDevicePrimaryCtxRelease = unsafe extern "C" fn(i32) -> i32;

/// Resolve one typed symbol; returns the raw (Copy) function pointer.
fn resolve<T: Copy>(lib: &libloading::Library, name: &[u8]) -> Result<T, String> {
    // SAFETY: `Library::get` is unsafe (arbitrary symbol name); we only pass
    // documented CUDA entry-point names and a matching fn-pointer type.
    unsafe { lib.get::<T>(name) }
        .map(|sym| *sym)
        .map_err(|e| format!("{}: {e}", String::from_utf8_lossy(name)))
}

/// Loaded `nvcuda.dll` plus every resolved entry point. The `Library` is kept
/// alive for the lifetime of this struct so the fn pointers stay valid.
struct CudaApi {
    _lib: libloading::Library,
    cu_init: CuInit,
    cu_driver_get_version: CuDriverGetVersion,
    cu_device_get_count: CuDeviceGetCount,
    cu_device_get: CuDeviceGet,
    cu_device_get_name: CuDeviceGetName,
    cu_device_get_attribute: CuDeviceGetAttribute,
    cu_device_primary_ctx_retain: CuDevicePrimaryCtxRetain,
    cu_ctx_set_current: CuCtxSetCurrent,
    cu_mem_alloc: CuMemAlloc,
    cu_mem_free: CuMemFree,
    cu_memcpy_hto_d: CuMemcpyHtoD,
    cu_memcpy_dto_h: CuMemcpyDtoH,
    cu_mem_get_info: CuMemGetInfo,
    cu_module_load_data: CuModuleLoadData,
    cu_module_get_function: CuModuleGetFunction,
    cu_launch_kernel: CuLaunchKernel,
    cu_ctx_synchronize: CuCtxSynchronize,
    cu_get_error_string: CuGetErrorString,
    cu_device_primary_ctx_release: Option<CuDevicePrimaryCtxRelease>,
}

impl CudaApi {
    fn load() -> Result<Self, String> {
        // SAFETY: loading a system DLL by its documented name.
        let lib = unsafe { libloading::Library::new("nvcuda.dll") }
            .map_err(|e| format!("nvcuda.dll could not be loaded: {e}"))?;

        let cu_ctx_set_current: CuCtxSetCurrent = resolve(&lib, b"cuCtxSetCurrent")
            .or_else(|_| resolve(&lib, b"cuCtxSetCurrent_v2"))
            .or_else(|_| resolve(&lib, b"cuCtxPushCurrent_v2"))
            .map_err(|_| "cuCtxSetCurrent / cuCtxPushCurrent_v2 not found".to_string())?;

        // Release symbol name varies by driver build; optional, with fallback.
        let cu_device_primary_ctx_release: Option<CuDevicePrimaryCtxRelease> =
            resolve(&lib, b"cuDevicePrimaryCtxRelease_v2")
                .ok()
                .or_else(|| resolve(&lib, b"cuDevicePrimaryCtxRelease").ok());

        Ok(CudaApi {
            cu_init: resolve(&lib, b"cuInit")?,
            cu_driver_get_version: resolve(&lib, b"cuDriverGetVersion")?,
            cu_device_get_count: resolve(&lib, b"cuDeviceGetCount")?,
            cu_device_get: resolve(&lib, b"cuDeviceGet")?,
            cu_device_get_name: resolve(&lib, b"cuDeviceGetName")?,
            cu_device_get_attribute: resolve(&lib, b"cuDeviceGetAttribute")?,
            cu_device_primary_ctx_retain: resolve(&lib, b"cuDevicePrimaryCtxRetain")?,
            cu_ctx_set_current,
            cu_mem_alloc: resolve(&lib, b"cuMemAlloc_v2")?,
            cu_mem_free: resolve(&lib, b"cuMemFree_v2")?,
            cu_memcpy_hto_d: resolve(&lib, b"cuMemcpyHtoD_v2")?,
            cu_memcpy_dto_h: resolve(&lib, b"cuMemcpyDtoH_v2")?,
            cu_mem_get_info: resolve(&lib, b"cuMemGetInfo_v2")?,
            cu_module_load_data: resolve(&lib, b"cuModuleLoadData")?,
            cu_module_get_function: resolve(&lib, b"cuModuleGetFunction")?,
            cu_launch_kernel: resolve(&lib, b"cuLaunchKernel")?,
            cu_ctx_synchronize: resolve(&lib, b"cuCtxSynchronize")?,
            cu_get_error_string: resolve(&lib, b"cuGetErrorString")?,
            cu_device_primary_ctx_release,
            _lib: lib,
        })
    }

    /// Real driver error text via `cuGetErrorString`.
    fn error_string(&self, code: i32) -> String {
        let mut p: *const i8 = std::ptr::null();
