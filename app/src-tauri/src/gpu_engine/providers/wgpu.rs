//! wgpu provider — vendor-neutral DX12/Vulkan compute via WGSL (wgpu 30).
//!
//! Owned by Subagent 2. Uses `pollster::block_on` for the few async wgpu 30
//! calls (`enumerate_adapters`, `request_device`) and a callback + bounded
//! `device.poll(PollType::Wait { .. })` loop for the readback map. Every WGSL
//! module is compiled at `initialize()`; only successfully compiled pipelines
//! are advertised in `supported_workloads()`. An uncaptured-error handler
//! captures validation/device-loss text instead of panicking, and the device
//! is dropped so the next attempt can re-initialise.

use super::{
    build_outcome, check_interrupt, pick_device_id, validate_gpu, CancelToken, ExecutionOutcome,
    ExecutionRequest, GpuDeviceInfo, GpuError, GpuMemoryInfo, GpuProvider, GpuResult,
    ProviderCapabilities, ProviderKind, TimingBreakdown, WorkloadInputs, WorkloadParams,
    WorkloadRequirement, WorkloadSupport,
};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant};
use wgpu::util::DeviceExt;

const WGSL_AXPY: &str = include_str!("wgsl/axpy.wgsl");
const WGSL_SCALE: &str = include_str!("wgsl/scale.wgsl");
const WGSL_MATMUL: &str = include_str!("wgsl/matmul.wgsl");
const WGSL_ROWSUM: &str = include_str!("wgsl/rowsum.wgsl");

/// (workload_id, WGSL source). All shaders share one 4-binding layout.
const KERNELS: [(&'static str, &'static str); 4] = [
    ("vector_axpy", WGSL_AXPY),
    ("vector_scale", WGSL_SCALE),
    ("matmul", WGSL_MATMUL),
    ("row_sum", WGSL_ROWSUM),
];

const WORKGROUP: u32 = 64;

/// Pack the small uniform `Params` struct (4 scalars, 16 bytes, WGSL-uniform
/// layout compliant).
fn params_bytes(p0: u32, p1: u32, p2: u32, a: f32) -> [u8; 16] {
    let mut b = [0u8; 16];
    b[0..4].copy_from_slice(&p0.to_le_bytes());
    b[4..8].copy_from_slice(&p1.to_le_bytes());
    b[8..12].copy_from_slice(&p2.to_le_bytes());
    b[12..16].copy_from_slice(&a.to_bits().to_le_bytes());
    b
}

/// Reinterpret an `&[f32]` as bytes (f32 has no padding; alignment >= u8).
fn as_bytes(slice: &[f32]) -> &[u8] {
    // SAFETY: f32 is a plain 4-byte value with no padding or invalid bit
    // patterns, and its alignment is at least that of u8.
    unsafe { std::slice::from_raw_parts(slice.as_ptr() as *const u8, std::mem::size_of_val(slice)) }
}

/// Reinterpret little-endian bytes as `f32` values.
fn from_bytes(bytes: &[u8]) -> Vec<f32> {
    bytes
        .chunks_exact(4)
        .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
        .collect()
}

/// Live wgpu state. All wgpu resources are `Send + Sync` on native, so this
/// struct is `Send` and can live behind `Mutex<Option<...>>`.
struct WgpuInner {
    _instance: wgpu::Instance,
    device: wgpu::Device,
    queue: wgpu::Queue,
    bgl: wgpu::BindGroupLayout,
    _pipeline_layout: wgpu::PipelineLayout,
    _shaders: Vec<wgpu::ShaderModule>,
    pipelines: HashMap<&'static str, wgpu::ComputePipeline>,
    error_slot: Arc<Mutex<Option<String>>>,
    max_workgroups: u32,
    max_storage_buffer_binding_size: u64,
}

/// Measured result of the raw wgpu compute (before validation/outcome).
#[derive(Default)]
struct WgpuRun {
    out: Vec<f32>,
    setup_ns: u64,
    transfer_in_ns: u64,
    compute_ns: u64,
    transfer_out_ns: u64,
    peak_bytes: u64,
    notes: Vec<String>,
}

pub struct WgpuProvider {
    inner: Mutex<Option<WgpuInner>>,
    available: AtomicBool,
    compute_ready: AtomicBool,
    devices: RwLock<Vec<GpuDeviceInfo>>,
    caps: RwLock<ProviderCapabilities>,
    supported: RwLock<Vec<WorkloadSupport>>,
    mem: RwLock<GpuMemoryInfo>,
    last_error: RwLock<Option<String>>,
}

impl WgpuProvider {
    pub fn new() -> Self {
        WgpuProvider {
            inner: Mutex::new(None),
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

impl Default for WgpuProvider {
    fn default() -> Self {
        Self::new()
    }
}

// --- one dispatch (buffers + compute pass + readback) ----------------------

fn dispatch(
    inner: &WgpuInner,
    pipeline: &wgpu::ComputePipeline,
    x: &[f32],
    y: &[f32],
    out_len: usize,
    params: [u8; 16],
    timeout: Duration,
    start: Instant,
) -> GpuResult<WgpuRun> {
    let device = &inner.device;
    let queue = &inner.queue;
