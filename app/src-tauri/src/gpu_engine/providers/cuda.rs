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
    Ok(())
}

fn launch(
    api: &CudaApi,
    function: usize,
    grid: (u32, u32, u32),
    block: (u32, u32, u32),
    params: &mut Vec<*mut c_void>,
) -> GpuResult<()> {
    let code = unsafe {
        (api.cu_launch_kernel)(
            function as *mut c_void,
            grid.0,
            grid.1,
            grid.2,
            block.0,
            block.1,
            block.2,
            0,
            std::ptr::null_mut(),
            params.as_mut_ptr(),
            std::ptr::null_mut(),
        )
    };
    if code != 0 {
        return Err(exec_err(api, code, "cuLaunchKernel"));
    }
    Ok(())
}

fn synchronize(api: &CudaApi) -> GpuResult<()> {
    let code = unsafe { (api.cu_ctx_synchronize)() };
    if code != 0 {
        return Err(exec_err(api, code, "cuCtxSynchronize (possible WDDM TDR)"));
    }
    Ok(())
}

fn ceil_div(a: usize, b: usize) -> u32 {
    ((a + b - 1) / b) as u32
}

// --- raw compute -----------------------------------------------------------

fn run_cuda(
    drv: &CudaDriver,
    params: &WorkloadParams,
    inputs: &WorkloadInputs,
    cancel: &CancelToken,
    timeout: Duration,
    start: Instant,
) -> GpuResult<CudaRun> {
    let api = &drv.api;
    let setup_start = Instant::now();

    // CUDA contexts are per-thread. The primary context was made current on
    // the initialize() thread, but execute() runs on a scheduler worker, so
    // set it current here before any allocation/module work on this thread.
    let code = unsafe { (api.cu_ctx_set_current)(drv.context as *mut c_void) };
    if code != 0 {
        return Err(exec_err(api, code, "cuCtxSetCurrent (execute thread)"));
    }

    // Find the loaded function for this workload (honest: never runs a kernel
    // that was not JIT-loaded at initialize()).
    let function = drv
        .modules
        .iter()
        .find(|m| m.workload_id == params.workload_id())
        .map(|m| m.function)
        .ok_or_else(|| {
            let reason = drv
                .ptx_errors
                .iter()
                .find(|(id, _)| id.as_str() == params.workload_id())
                .map(|(_, r)| r.clone())
                .unwrap_or_else(|| "no PTX module was loaded for this workload".to_string());
            GpuError::Unsupported {
                provider: "cuda".to_string(),
                detail: format!("{}: {reason}", params.workload_id()),
            }
        })?;

    let block = drv.max_threads_per_block.min(256).max(1);
    let mut notes = vec![
        format!("CUDA driver API (nvcuda.dll), sm_75 PTX JIT, blockDim={block}"),
        "compute capability read via cuDeviceGetAttribute".to_string(),
    ];

    // Everything up to here is setup (function lookup + preparation).
    let setup_ns = setup_start.elapsed().as_nanos() as u64;

    let mut out = vec![0.0f32; params.output_len()];
    // Assigned in every match arm (verified by definite-assignment analysis).
    let transfer_in_ns: u64;
    let transfer_out_ns: u64;
    let peak_bytes: u64;
    let mut compute_ns = 0u64;

    match *params {
        WorkloadParams::VectorAxpy { n, a } | WorkloadParams::VectorScale { n, a } => {
            let is_axpy = matches!(*params, WorkloadParams::VectorAxpy { .. });
            // Allocate full device buffers once, then launch chunked slices by
            // offsetting the device pointers (<= 1M elements per launch).
            let up_start = Instant::now();
            let (x_dev, x_bytes) = dev_upload(api, &inputs.x, n)?;
            let (y_dev, y_bytes) = if is_axpy {
                dev_upload(api, &inputs.y, n)?
            } else {
                (0u64, 0u64)
            };
            let (out_dev, out_bytes) = dev_alloc(api, n * 4).map(|p| (p, (n * 4) as u64))?;
            transfer_in_ns = up_start.elapsed().as_nanos() as u64;
            peak_bytes = x_bytes + y_bytes + out_bytes;

            let mut offset = 0usize;
            while offset < n {
                check_interrupt("cuda", cancel, start, timeout)?;
                let len = (n - offset).min(VECTOR_CHUNK);
                let byte_off = (offset * 4) as u64;
                let px = x_dev + byte_off;
                let po = out_dev + byte_off;
                let n32 = len as u32;
                let grid = (ceil_div(len, block as usize), 1, 1);
                let blk = (block, 1, 1);
                let compute_start = Instant::now();
                if is_axpy {
                    let py = y_dev + byte_off;
                    let mut kp: Vec<*mut c_void> = vec![
                        &px as *const u64 as *mut c_void,
                        &py as *const u64 as *mut c_void,
                        &po as *const u64 as *mut c_void,
                        &a as *const f32 as *mut c_void,
                        &n32 as *const u32 as *mut c_void,
                    ];
                    launch(api, function, grid, blk, &mut kp)?;
                } else {
                    let mut kp: Vec<*mut c_void> = vec![
                        &px as *const u64 as *mut c_void,
                        &po as *const u64 as *mut c_void,
                        &a as *const f32 as *mut c_void,
                        &n32 as *const u32 as *mut c_void,
                    ];
                    launch(api, function, grid, blk, &mut kp)?;
                }
                synchronize(api)?;
                compute_ns += compute_start.elapsed().as_nanos() as u64;
                offset += len;
            }

            let down_start = Instant::now();
            dev_download(api, out_dev, &mut out)?;
            transfer_out_ns = down_start.elapsed().as_nanos() as u64;

            dev_free(api, x_dev);
            if y_dev != 0 {
                dev_free(api, y_dev);
            }
            dev_free(api, out_dev);
            if offset != n {
                notes.push(format!("processed {offset}/{n} elements"));
            } else {
                notes.push(format!("{n} elements in {} chunk(s)", ceil_div(n, VECTOR_CHUNK)));
            }
        }
        WorkloadParams::Matmul { m, k, n } => {
            check_interrupt("cuda", cancel, start, timeout)?;
            let up_start = Instant::now();
            let (a_dev, a_bytes) = dev_upload(api, &inputs.x, m * k)?;
            let (b_dev, b_bytes) = dev_upload(api, &inputs.y, k * n)?;
            let (c_dev, c_bytes) = dev_alloc(api, m * n * 4).map(|p| (p, (m * n * 4) as u64))?;
            transfer_in_ns = up_start.elapsed().as_nanos() as u64;
            peak_bytes = a_bytes + b_bytes + c_bytes;

            // 2D grid: x -> columns (j), y -> rows (i). tile*tile <= maxThreads.
            let mut tile = 16u32;
            while tile * tile > drv.max_threads_per_block && tile > 1 {
                tile /= 2;
            }
            let grid = (ceil_div(n, tile as usize), ceil_div(m, tile as usize), 1);
            let blk = (tile, tile, 1);
            let m32 = m as u32;
            let k32 = k as u32;
            let n32 = n as u32;
            let compute_start = Instant::now();
            let mut kp: Vec<*mut c_void> = vec![
                &a_dev as *const u64 as *mut c_void,
                &b_dev as *const u64 as *mut c_void,
                &c_dev as *const u64 as *mut c_void,
                &m32 as *const u32 as *mut c_void,
                &k32 as *const u32 as *mut c_void,
                &n32 as *const u32 as *mut c_void,
            ];
            launch(api, function, grid, blk, &mut kp)?;
            synchronize(api)?;
            compute_ns = compute_start.elapsed().as_nanos() as u64;

            let down_start = Instant::now();
            dev_download(api, c_dev, &mut out)?;
            transfer_out_ns = down_start.elapsed().as_nanos() as u64;

            dev_free(api, a_dev);
            dev_free(api, b_dev);
            dev_free(api, c_dev);
            notes.push(format!("naive matmul {m}x{k}x{n}, 2D grid tile {tile}"));
        }
        WorkloadParams::RowSum { rows, cols } => {
            check_interrupt("cuda", cancel, start, timeout)?;
            let up_start = Instant::now();
            let (a_dev, a_bytes) = dev_upload(api, &inputs.x, rows * cols)?;
            let (o_dev, o_bytes) = dev_alloc(api, rows * 4).map(|p| (p, (rows * 4) as u64))?;
            transfer_in_ns = up_start.elapsed().as_nanos() as u64;
            peak_bytes = a_bytes + o_bytes;

            let grid = (ceil_div(rows, block as usize), 1, 1);
            let blk = (block, 1, 1);
            let rows32 = rows as u32;
            let cols32 = cols as u32;
            let compute_start = Instant::now();
            let mut kp: Vec<*mut c_void> = vec![
                &a_dev as *const u64 as *mut c_void,
                &o_dev as *const u64 as *mut c_void,
                &rows32 as *const u32 as *mut c_void,
                &cols32 as *const u32 as *mut c_void,
            ];
            launch(api, function, grid, blk, &mut kp)?;
            synchronize(api)?;
            compute_ns = compute_start.elapsed().as_nanos() as u64;

            let down_start = Instant::now();
            dev_download(api, o_dev, &mut out)?;
            transfer_out_ns = down_start.elapsed().as_nanos() as u64;

            dev_free(api, a_dev);
            dev_free(api, o_dev);
            notes.push(format!("row_sum {rows}x{cols}, one thread per row"));
        }
    }

    check_interrupt("cuda", cancel, start, timeout)?;
    Ok(CudaRun {
        out,
        setup_ns,
        transfer_in_ns,
        compute_ns,
        transfer_out_ns,
        peak_bytes,
        notes,
    })
}

// --- initialization --------------------------------------------------------

impl CudaProvider {
    fn do_initialize(&self) -> GpuResult<Vec<GpuDeviceInfo>> {
        let api = CudaApi::load().map_err(|detail| GpuError::DriverUnavailable {
            provider: "cuda".to_string(),
            detail,
        })?;

        let code = unsafe { (api.cu_init)(0) };
        if code != 0 {
            return Err(init_err(&api, code, "cuInit(0)"));
        }

        let mut driver_version: Option<String> = None;
        let mut v: i32 = 0;
        if unsafe { (api.cu_driver_get_version)(&mut v) } == 0 && v > 0 {
            driver_version = Some(format_driver_version(v));
        }

        let mut count: i32 = 0;
        let code = unsafe { (api.cu_device_get_count)(&mut count) };
        if code != 0 {
            return Err(init_err(&api, code, "cuDeviceGetCount"));
        }
        if count <= 0 {
            return Err(GpuError::InitializationFailed {
                provider: "cuda".to_string(),
                code: None,
                detail: "cuInit succeeded but no CUDA device was found (cuDeviceGetCount == 0)"
                    .to_string(),
            });
        }

        // Enumerate every device (names + compute capability) for the UI.
        let mut devices: Vec<GpuDeviceInfo> = Vec::new();
        let mut max_threads_per_block: u32 = 256;
        for ordinal in 0..count {
            let mut dev: i32 = -1;
            if unsafe { (api.cu_device_get)(&mut dev, ordinal) } != 0 {
                continue;
            }
            let mut name_buf = [0i8; 256];
            let name = if unsafe {
                (api.cu_device_get_name)(name_buf.as_mut_ptr(), name_buf.len() as i32, dev)
            } == 0
            {
                // SAFETY: CUDA writes a NUL-terminated name into name_buf.
                unsafe { CStr::from_ptr(name_buf.as_ptr()) }
                    .to_string_lossy()
                    .to_string()
            } else {
                format!("NVIDIA CUDA device {ordinal}")
            };

            let mut major: i32 = 0;
            let mut minor: i32 = 0;
            unsafe {
                (api.cu_device_get_attribute)(
                    &mut major,
                    CU_DEVICE_ATTRIBUTE_COMPUTE_CAPABILITY_MAJOR,
                    dev,
                );
