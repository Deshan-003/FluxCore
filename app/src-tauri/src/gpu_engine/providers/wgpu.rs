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
    let out_bytes = (out_len * 4) as u64;

    // Honest limit check on the largest single storage buffer binding.
    let largest = (x.len().max(y.len()).max(out_len).saturating_mul(4)) as u64;
    if largest > inner.max_storage_buffer_binding_size {
        return Err(GpuError::ResourceExhausted {
            provider: "wgpu".to_string(),
            detail: format!(
                "largest storage buffer ({largest} bytes) exceeds adapter max_storage_buffer_binding_size ({})",
                inner.max_storage_buffer_binding_size
            ),
        });
    }

    let mut run = WgpuRun::default();

    // transfer-in: upload x, y (or a 4-byte dummy) and the params uniform.
    let t_in = Instant::now();
    let x_buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("fluxcore-x"),
        contents: as_bytes(x),
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
    });
    let dummy = [0u8; 4];
    let y_contents: &[u8] = if y.is_empty() { &dummy } else { as_bytes(y) };
    let y_buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("fluxcore-y"),
        contents: y_contents,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
    });
    let params_buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("fluxcore-params"),
        contents: &params,
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
    });
    run.transfer_in_ns = t_in.elapsed().as_nanos() as u64;

    // setup: output + readback buffers + bind group.
    let t_setup = Instant::now();
    let out_buf = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("fluxcore-out"),
        size: out_bytes,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
        mapped_at_creation: false,
    });
    let readback_buf = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("fluxcore-readback"),
        size: out_bytes,
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("fluxcore-bg"),
        layout: &inner.bgl,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                    buffer: &x_buf,
                    offset: 0,
                    size: None,
                }),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                    buffer: &y_buf,
                    offset: 0,
                    size: None,
                }),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                    buffer: &out_buf,
                    offset: 0,
                    size: None,
                }),
            },
            wgpu::BindGroupEntry {
                binding: 3,
                resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                    buffer: &params_buf,
                    offset: 0,
                    size: None,
                }),
            },
        ],
    });
    run.setup_ns = t_setup.elapsed().as_nanos() as u64;

    // compute: record pass, copy to readback, submit, bounded wait.
    let t_compute = Instant::now();
    let wg = ((out_len as u64 + WORKGROUP as u64 - 1) / WORKGROUP as u64)
        .min(inner.max_workgroups as u64)
        .max(1) as u32;
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
    {
        let mut cpass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor::default());
        cpass.set_pipeline(pipeline);
        cpass.set_bind_group(0, &bind_group, &[]);
        cpass.dispatch_workgroups(wg, 1, 1);
    }
    encoder.copy_buffer_to_buffer(&out_buf, 0, &readback_buf, 0, out_bytes);
    let cmd = encoder.finish();
    let index = queue.submit(std::iter::once(cmd));

    // Register the map callback, then drive it to completion with a bounded
    // wait so a hung GPU can never block the worker forever (R1 / R7).
    let (tx, rx) = std::sync::mpsc::channel::<bool>();
    readback_buf
        .slice(..)
        .map_async(wgpu::MapMode::Read, move |res| {
            let _ = tx.send(res.is_ok());
        });
    let remaining = timeout.saturating_sub(start.elapsed()).max(Duration::from_millis(1));
    match device.poll(wgpu::PollType::Wait {
        submission_index: Some(index),
        timeout: Some(remaining),
    }) {
        Ok(_) => {}
        Err(wgpu::PollError::Timeout) => {
            return Err(GpuError::Timeout {
                provider: "wgpu".to_string(),
            });
        }
        Err(e) => {
            return Err(GpuError::ExecutionFailed {
                provider: "wgpu".to_string(),
                code: None,
                detail: format!("device.poll(Wait): {e}"),
            });
        }
    }
    match rx.recv_timeout(Duration::from_millis(500)) {
