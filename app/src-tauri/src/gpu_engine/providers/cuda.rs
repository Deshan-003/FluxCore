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
        let r = unsafe { (self.cu_get_error_string)(code, &mut p) };
        if r == 0 && !p.is_null() {
            // SAFETY: CUDA returns a pointer to a static, NUL-terminated string.
            unsafe { CStr::from_ptr(p) }.to_string_lossy().to_string()
        } else {
            format!("CUDA error code {code}")
        }
    }
}

// --- error mappers ---------------------------------------------------------

fn init_err(api: &CudaApi, code: i32, ctx: &str) -> GpuError {
    GpuError::InitializationFailed {
        provider: "cuda".to_string(),
        code: Some(code),
        detail: format!("{ctx}: {}", api.error_string(code)),
    }
}
fn exec_err(api: &CudaApi, code: i32, ctx: &str) -> GpuError {
    GpuError::ExecutionFailed {
        provider: "cuda".to_string(),
        code: Some(code),
        detail: format!("{ctx}: {}", api.error_string(code)),
    }
}
fn resource_err(api: &CudaApi, code: i32, ctx: &str) -> GpuError {
    GpuError::ResourceExhausted {
        provider: "cuda".to_string(),
        detail: format!("{ctx}: {}", api.error_string(code)),
    }
}

/// Format a `cuDriverGetVersion` integer (e.g. 13010 -> "13.1").
fn format_driver_version(v: i32) -> String {
    format!("{}.{}", v / 1000, (v % 1000) / 10)
}

/// Lightweight, independent driver-version probe used by `capability.rs`.
/// Loads nvcuda.dll, initializes, reads the version via `cuDriverGetVersion`
/// and drops the library. Does not create a context or touch NVML.
pub fn probe_driver_version() -> Option<String> {
    let api = CudaApi::load().ok()?;
    let code = unsafe { (api.cu_init)(0) };
    if code != 0 {
        return None;
    }
    let mut v: i32 = 0;
    let code = unsafe { (api.cu_driver_get_version)(&mut v) };
    (code == 0 && v > 0).then(|| format_driver_version(v))
}

// --- driver state ----------------------------------------------------------

struct CudaModule {
    workload_id: &'static str,
    _module: usize,  // CUmodule handle (owned; released with the context)
    function: usize, // CUfunction handle
}

/// All state that must stay alive between calls. Handles are stored as `usize`
/// so this struct is `Send` (raw pointers are not), allowing it to live inside
/// a `Mutex` behind `Arc<dyn GpuProvider>`.
struct CudaDriver {
    api: CudaApi,
    context: usize, // CUcontext (primary context of device 0)
    device_ordinal: i32,
    modules: Vec<CudaModule>,
    ptx_errors: Vec<(String, String)>, // (workload_id, driver error text)
    max_threads_per_block: u32,
    free_bytes: u64,
    total_bytes: u64,
}

/// Result of the raw CUDA compute (before validation/outcome assembly).
struct CudaRun {
    out: Vec<f32>,
    setup_ns: u64,
    transfer_in_ns: u64,
    compute_ns: u64,
    transfer_out_ns: u64,
    peak_bytes: u64,
    notes: Vec<String>,
}

pub struct CudaProvider {
    driver: Mutex<Option<CudaDriver>>,
    available: AtomicBool,
    compute_ready: AtomicBool,
    devices: RwLock<Vec<GpuDeviceInfo>>,
    caps: RwLock<ProviderCapabilities>,
    supported: RwLock<Vec<WorkloadSupport>>,
    mem: RwLock<GpuMemoryInfo>,
    last_error: RwLock<Option<String>>,
}

impl CudaProvider {
    pub fn new() -> Self {
        CudaProvider {
            driver: Mutex::new(None),
            available: AtomicBool::new(false),
            compute_ready: AtomicBool::new(false),
            devices: RwLock::new(Vec::new()),
            caps: RwLock::new(ProviderCapabilities::default()),
            supported: RwLock::new(Vec::new()),
            mem: RwLock::new(GpuMemoryInfo::default()),
            last_error: RwLock::new(None),
        }
    }
}

impl Default for CudaProvider {
    fn default() -> Self {
        Self::new()
    }
}

// --- device-buffer helpers -------------------------------------------------

fn dev_alloc(api: &CudaApi, bytes: usize) -> GpuResult<u64> {
    let mut p: u64 = 0;
    let code = unsafe { (api.cu_mem_alloc)(&mut p, bytes) };
    if code != 0 {
        return Err(resource_err(api, code, "cuMemAlloc_v2"));
    }
    Ok(p)
}

/// Allocate `floats` f32 values and upload `data` (which must be at least that
/// long). Returns the device pointer plus the byte count.
fn dev_upload(api: &CudaApi, data: &[f32], floats: usize) -> GpuResult<(u64, u64)> {
    let bytes = (floats * 4) as u64;
    let p = dev_alloc(api, bytes as usize)?;
    let code = unsafe {
        (api.cu_memcpy_hto_d)(p, data.as_ptr() as *const c_void, bytes as usize)
    };
    if code != 0 {
        unsafe { (api.cu_mem_free)(p) };
        return Err(exec_err(api, code, "cuMemcpyHtoD_v2"));
    }
    Ok((p, bytes))
}

fn dev_free(api: &CudaApi, p: u64) {
    if p != 0 {
        unsafe {
            (api.cu_mem_free)(p);
        }
    }
}

fn dev_download(api: &CudaApi, p: u64, out: &mut [f32]) -> GpuResult<()> {
    let bytes = out.len() * 4;
    let code = unsafe { (api.cu_memcpy_dto_h)(out.as_mut_ptr() as *mut c_void, p, bytes) };
    if code != 0 {
        return Err(exec_err(api, code, "cuMemcpyDtoH_v2"));
    }
