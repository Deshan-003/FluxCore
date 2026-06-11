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
        Ok(true) => {}
        Ok(false) => {
            return Err(GpuError::ExecutionFailed {
                provider: "wgpu".to_string(),
                code: None,
                detail: "readback map_async failed (device lost or validation error)".to_string(),
            });
        }
        Err(_) => {
            return Err(GpuError::Timeout {
                provider: "wgpu".to_string(),
            });
        }
    }
    run.compute_ns = t_compute.elapsed().as_nanos() as u64;

    // transfer-out: read the mapped bytes back into f32 values.
    let t_out = Instant::now();
    let view = readback_buf.slice(..).get_mapped_range().map_err(|_| {
        GpuError::ExecutionFailed {
            provider: "wgpu".to_string(),
            code: None,
            detail: "get_mapped_range failed on readback buffer".to_string(),
        }
    })?;
    let bytes: &[u8] = &view;
    let out_f32 = from_bytes(bytes);
    drop(view);
    readback_buf.unmap();
    run.transfer_out_ns = t_out.elapsed().as_nanos() as u64;

    let y_bytes: u64 = if y.is_empty() { 4 } else { (y.len() * 4) as u64 };
    run.peak_bytes = (x.len() * 4) as u64 + y_bytes + out_bytes * 2 + 16;
    run.out = out_f32;
    Ok(run)
}

fn accumulate(agg: &mut WgpuRun, d: &WgpuRun) {
    agg.setup_ns += d.setup_ns;
    agg.transfer_in_ns += d.transfer_in_ns;
    agg.compute_ns += d.compute_ns;
    agg.transfer_out_ns += d.transfer_out_ns;
    agg.peak_bytes = agg.peak_bytes.max(d.peak_bytes);
}

// --- raw compute -----------------------------------------------------------

fn run_wgpu(
    inner: &WgpuInner,
    params: &WorkloadParams,
    inputs: &WorkloadInputs,
    cancel: &CancelToken,
    timeout: Duration,
    start: Instant,
) -> GpuResult<WgpuRun> {
    let pipeline = inner.pipelines.get(params.workload_id()).ok_or_else(|| {
        GpuError::Unsupported {
            provider: "wgpu".to_string(),
            detail: format!(
                "workload '{}' has no compiled WGSL pipeline on this adapter",
                params.workload_id()
            ),
        }
    })?;

    let mut agg = WgpuRun::default();
    let mut out = vec![0.0f32; params.output_len()];

    // Per-buffer float cap: keep every storage binding within the adapter's
    // max_storage_buffer_binding_size and keep allocations modest.
    let cap = (inner.max_storage_buffer_binding_size / 4).min(4_194_240) as usize;

    match *params {
        WorkloadParams::VectorAxpy { n, a } | WorkloadParams::VectorScale { n, a } => {
            let is_axpy = matches!(*params, WorkloadParams::VectorAxpy { .. });
            let step = n.min(cap).max(1);
            let mut s = 0usize;
            let mut chunks = 0u32;
            while s < n {
                check_interrupt("wgpu", cancel, start, timeout)?;
                let e = (s + step).min(n);
                let y: &[f32] = if is_axpy { &inputs.y[s..e] } else { &[] };
                let pb = params_bytes((e - s) as u32, 0, 0, a);
                let d = dispatch(inner, pipeline, &inputs.x[s..e], y, e - s, pb, timeout, start)?;
                out[s..e].copy_from_slice(&d.out);
                accumulate(&mut agg, &d);
                s = e;
                chunks += 1;
            }
            agg.notes.push(format!(
                "WGSL vector kernel, workgroup_size({WORKGROUP}), {chunks} chunk(s), <= {wg} workgroups/dispatch",
                wg = inner.max_workgroups
            ));
        }
        WorkloadParams::Matmul { m, k, n } => {
            check_interrupt("wgpu", cancel, start, timeout)?;
            let pb = params_bytes(m as u32, k as u32, n as u32, 0.0);
            let d = dispatch(inner, pipeline, &inputs.x, &inputs.y, m * n, pb, timeout, start)?;
            copy_into(&mut out, &d.out);
            accumulate(&mut agg, &d);
            agg.notes.push(format!(
                "WGSL naive matmul {m}x{k}x{n}, workgroup_size({WORKGROUP})"
            ));
        }
        WorkloadParams::RowSum { rows, cols } => {
            check_interrupt("wgpu", cancel, start, timeout)?;
            let pb = params_bytes(rows as u32, cols as u32, 0, 0.0);
            let d = dispatch(inner, pipeline, &inputs.x, &[], rows, pb, timeout, start)?;
            copy_into(&mut out, &d.out);
            accumulate(&mut agg, &d);
            agg.notes.push(format!(
                "WGSL row_sum {rows}x{cols}, one invocation per row, workgroup_size({WORKGROUP})"
            ));
        }
    }

    check_interrupt("wgpu", cancel, start, timeout)?;
    agg.out = out;
    Ok(agg)
}

fn copy_into(dst: &mut [f32], src: &[f32]) {
    let n = dst.len().min(src.len());
    dst[..n].copy_from_slice(&src[..n]);
}

// --- initialization --------------------------------------------------------

impl WgpuProvider {
    fn do_initialize(&self) -> GpuResult<Vec<GpuDeviceInfo>> {
        // Instance with DX12 + Vulkan only (matching the crate features).
        let mut desc = wgpu::InstanceDescriptor::new_without_display_handle_from_env();
        desc.backends = wgpu::Backends::DX12 | wgpu::Backends::VULKAN;
        let instance = wgpu::Instance::new(desc);

        let backends = wgpu::Backends::DX12 | wgpu::Backends::VULKAN;
        let adapters: Vec<wgpu::Adapter> = pollster::block_on(instance.enumerate_adapters(backends));
        if adapters.is_empty() {
            return Err(GpuError::InitializationFailed {
                provider: "wgpu".to_string(),
                code: None,
                detail: "no DX12/Vulkan adapter was enumerated by wgpu".to_string(),
            });
        }

        // Record every discovered adapter and create a device on the first one
        // that succeeds. request_device is async in wgpu 30.
        let mut devices: Vec<GpuDeviceInfo> = Vec::new();
        let mut chosen: Option<(wgpu::Device, wgpu::Queue, wgpu::Limits, String, String)> = None;
        for (idx, adapter) in adapters.iter().enumerate() {
            let info = adapter.get_info();
            let limits = adapter.limits();
            let backend_str = match info.backend {
                wgpu::Backend::Dx12 => "wgpu-dx12",
                wgpu::Backend::Vulkan => "wgpu-vulkan",
                _ => "wgpu-other",
            };
            let device_id = format!("{backend_str}-{idx}");
            let is_discrete = matches!(info.device_type, wgpu::DeviceType::DiscreteGpu);
            devices.push(GpuDeviceInfo {
                device_id: device_id.clone(),
                name: info.name.clone(),
                vendor: format!("vendor id 0x{:x}", info.vendor),
                backend: backend_str.to_string(),
                monitor_gpu_index: None,
                dedicated_total_bytes: None, // wgpu does not expose VRAM totals
                dedicated_free_bytes: None,
                compute_capability: None,
                driver_version: if info.driver.is_empty() {
                    None
                } else {
                    Some(info.driver.clone())
                },
                max_workgroup_size: Some(limits.max_compute_workgroup_size_x),
                max_storage_buffer_bytes: Some(limits.max_storage_buffer_binding_size),
                is_discrete,
                notes: vec![format!(
                    "wgpu 30 adapter ({}); driver info: {}",
                    backend_str,
                    if info.driver_info.is_empty() {
                        "unavailable"
                    } else {
                        &info.driver_info
                    }
                )],
            });

            if chosen.is_none() {
                let dev_desc = wgpu::DeviceDescriptor {
                    label: Some("fluxcore-compute"),
                    required_features: wgpu::Features::empty(),
                    required_limits: limits.clone(),
                    ..Default::default()
                };
                if let Ok((device, queue)) = pollster::block_on(adapter.request_device(&dev_desc)) {
                    chosen = Some((
                        device,
                        queue,
                        limits,
                        backend_str.to_string(),
                        device_id,
                    ));
                }
            }
        }

        let (device, queue, limits, active_backend, _active_device_id) =
            chosen.ok_or_else(|| GpuError::InitializationFailed {
                provider: "wgpu".to_string(),
                code: None,
                detail: "wgpu found adapters but request_device failed on all of them".to_string(),
            })?;

        // Capture uncaptured wgpu errors instead of letting wgpu panic.
        let error_slot: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));
        let slot = Arc::clone(&error_slot);
        let handler: Arc<dyn wgpu::UncapturedErrorHandler> = Arc::new(move |e: wgpu::Error| {
            if let Ok(mut s) = slot.lock() {
                *s = Some(format!("{e:?}"));
            }
        });
        device.on_uncaptured_error(handler);

        // Shared bind-group layout: binding 0 x (read storage), 1 y (read
        // storage), 2 out (read_write storage), 3 params (uniform).
        let read_storage = wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Storage { read_only: true },
            has_dynamic_offset: false,
            min_binding_size: None,
        };
        let bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("fluxcore-bgl"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: read_storage.clone(),
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: read_storage,
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: false },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("fluxcore-pipeline-layout"),
            bind_group_layouts: &[Some(&bgl)],
            immediate_size: 0,
        });

        // Compile each WGSL module; advertise only pipelines that compiled
        // without an uncaptured validation error.
        let mut shaders: Vec<wgpu::ShaderModule> = Vec::new();
        let mut pipelines: HashMap<&'static str, wgpu::ComputePipeline> = HashMap::new();
        let mut supported: Vec<WorkloadSupport> = Vec::new();
        let mut wgsl_errors: Vec<(String, String)> = Vec::new();
        for (workload_id, wgsl) in KERNELS.iter() {
            if let Ok(mut s) = error_slot.lock() {
                *s = None;
            }
            let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some(*workload_id),
                source: wgpu::ShaderSource::Wgsl((*wgsl).into()),
            });
            let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: Some(*workload_id),
                layout: Some(&pipeline_layout),
                module: &shader,
                entry_point: Some("main"),
                compilation_options: Default::default(),
                cache: None,
            });
            // Flush validation errors to the handler, then inspect the slot.
            let _ = device.poll(wgpu::PollType::Poll);
            let captured = error_slot.lock().ok().and_then(|s| s.clone());
            if let Some(err) = captured {
                wgsl_errors.push((workload_id.to_string(), err));
                continue;
            }
            shaders.push(shader);
            pipelines.insert(*workload_id, pipeline);
            let max_elements = match *workload_id {
                "vector_axpy" | "vector_scale" => 64 * 1024 * 1024,
                "matmul" => 2048,
                _ => 32 * 1024 * 1024,
            };
            supported.push(WorkloadSupport {
                workload_id: workload_id.to_string(),
                max_elements,
                notes: vec!["WGSL compute pipeline compiled successfully".to_string()],
            });
        }

        let compute_ready = !pipelines.is_empty();
        let wgsl_error_note = if wgsl_errors.is_empty() {
            None
        } else {
            Some(
                wgsl_errors
                    .iter()
                    .map(|(id, e)| format!("{id}: {e}"))
                    .collect::<Vec<_>>()
                    .join("; "),
            )
        };

        let caps = ProviderCapabilities {
            compute_ready,
            unavailable_reason: if compute_ready {
                None
            } else {
                Some(
                    wgsl_error_note
                        .clone()
                        .unwrap_or_else(|| "no WGSL pipeline could be compiled".to_string()),
                )
            },
            max_threads_per_workgroup: Some(limits.max_compute_invocations_per_workgroup),
            max_workgroup_size_x: Some(limits.max_compute_workgroup_size_x),
            max_workgroup_size_y: Some(limits.max_compute_workgroup_size_y),
            max_workgroup_size_z: Some(limits.max_compute_workgroup_size_z),
            max_buffer_bytes: Some(limits.max_buffer_size),
            allocation_alignment: 4,
            backend_notes: {
                let mut n = vec![
                    "wgpu 30.0.2, WGSL compute, DX12 + Vulkan adapters".to_string(),
                    format!("{} WGSL pipeline(s) compiled", pipelines.len()),
                    format!(
                        "active backend {active_backend}; max_storage_buffer_binding_size={} bytes",
                        limits.max_storage_buffer_binding_size
                    ),
                ];
                if let Some(note) = &wgsl_error_note {
                    n.push(format!("WGSL compile failures: {note}"));
                }
                n
            },
        };

        let mem = GpuMemoryInfo::default(); // wgpu exposes no VRAM free/total

        let inner = WgpuInner {
            _instance: instance,
            device,
            queue,
            bgl,
            _pipeline_layout: pipeline_layout,
            _shaders: shaders,
            pipelines,
            error_slot,
            max_workgroups: limits.max_compute_workgroups_per_dimension,
            max_storage_buffer_binding_size: limits.max_storage_buffer_binding_size,
        };

        *self.inner.lock().map_err(|_| GpuError::Internal {
            provider: "wgpu".to_string(),
            detail: "inner state lock poisoned".to_string(),
        })? = Some(inner);
        *self.devices.write().map_err(|_| GpuError::Internal {
            provider: "wgpu".to_string(),
            detail: "devices lock poisoned".to_string(),
        })? = devices.clone();
        *self.caps.write().map_err(|_| GpuError::Internal {
            provider: "wgpu".to_string(),
            detail: "capabilities lock poisoned".to_string(),
        })? = caps;
        *self.supported.write().map_err(|_| GpuError::Internal {
            provider: "wgpu".to_string(),
            detail: "supported lock poisoned".to_string(),
        })? = supported;
        *self.mem.write().map_err(|_| GpuError::Internal {
            provider: "wgpu".to_string(),
            detail: "memory lock poisoned".to_string(),
        })? = mem;

        self.available.store(true, Ordering::SeqCst);
        self.compute_ready.store(compute_ready, Ordering::SeqCst);
        *self.last_error.write().unwrap_or_else(|e| e.into_inner()) = None;
        Ok(devices)
    }

    fn record_failure(&self, e: &GpuError) {
        self.available.store(false, Ordering::SeqCst);
        self.compute_ready.store(false, Ordering::SeqCst);
        *self
            .last_error
            .write()
            .unwrap_or_else(|p| p.into_inner()) = Some(e.to_string());
        *self
            .caps
            .write()
            .unwrap_or_else(|p| p.into_inner()) = ProviderCapabilities {
            compute_ready: false,
            unavailable_reason: Some(e.to_string()),
            ..ProviderCapabilities::default()
        };
    }
}

// --- GpuProvider impl ------------------------------------------------------

impl GpuProvider for WgpuProvider {
    fn id(&self) -> &'static str {
        "wgpu"
    }

    fn label(&self) -> &'static str {
        "wgpu (DX12 / Vulkan / WGSL)"
    }

    fn kind(&self) -> ProviderKind {
        ProviderKind::Wgpu
    }

    fn initialize(&self) -> GpuResult<Vec<GpuDeviceInfo>> {
        // Idempotent fast path.
        if self.available.load(Ordering::SeqCst) {
            if let Ok(guard) = self.inner.lock() {
                if guard.is_some() {
                    return Ok(self.devices.read().map(|d| (*d).clone()).unwrap_or_default());
                }
            }
        }
        // Never let a wgpu panic escape the trait boundary.
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| self.do_initialize()));
        match result {
            Ok(Ok(devices)) => Ok(devices),
            Ok(Err(e)) => {
                self.record_failure(&e);
                Err(e)
            }
            Err(_) => {
                let e = GpuError::InitializationFailed {
                    provider: "wgpu".to_string(),
                    code: None,
                    detail: "wgpu initialization panicked (caught)".to_string(),
                };
                self.record_failure(&e);
                Err(e)
            }
        }
    }

    fn is_available(&self) -> bool {
        self.available.load(Ordering::SeqCst)
    }

    fn devices(&self) -> Vec<GpuDeviceInfo> {
        self.devices.read().map(|d| (*d).clone()).unwrap_or_default()
    }

    fn capabilities(&self) -> ProviderCapabilities {
        self.caps.read().map(|c| (*c).clone()).unwrap_or_default()
    }

    fn supported_workloads(&self) -> Vec<WorkloadSupport> {
        self.supported.read().map(|s| (*s).clone()).unwrap_or_default()
    }

    fn memory_info(&self) -> Option<GpuMemoryInfo> {
        if !self.available.load(Ordering::SeqCst) {
            return None;
        }
        Some(GpuMemoryInfo::default())
    }

    fn can_execute(&self, requirement: &WorkloadRequirement) -> GpuResult<()> {
        if !self.available.load(Ordering::SeqCst) || !self.compute_ready.load(Ordering::SeqCst) {
            return Err(GpuError::Unsupported {
                provider: "wgpu".to_string(),
                detail: format!(
                    "wgpu compute not available: {}",
                    self.last_error()
                        .unwrap_or_else(|| "not compute-ready".to_string())
                ),
            });
        }
        let supported = self.supported_workloads();
        let sup = supported
            .iter()
            .find(|s| s.workload_id == requirement.workload_id)
            .ok_or_else(|| GpuError::Unsupported {
                provider: "wgpu".to_string(),
                detail: format!(
                    "workload '{}' has no compiled WGSL pipeline",
                    requirement.workload_id
                ),
            })?;
        if requirement.input_size as u64 > sup.max_elements {
            return Err(GpuError::ResourceExhausted {
                provider: "wgpu".to_string(),
                detail: format!(
                    "{} needs {} elements, above the {} element wgpu cap",
                    requirement.workload_id, requirement.input_size, sup.max_elements
                ),
            });
        }
        let caps = self.capabilities();
        if let Some(max_buf) = caps.max_buffer_bytes {
            if requirement.device_bytes > max_buf {
                return Err(GpuError::ResourceExhausted {
                    provider: "wgpu".to_string(),
                    detail: format!(
                        "workload needs {} device bytes, above adapter max_buffer_size {}",
                        requirement.device_bytes, max_buf
                    ),
                });
            }
        }
        Ok(())
    }

    fn execute(&self, request: &ExecutionRequest) -> GpuResult<ExecutionOutcome> {
        let start = Instant::now();
        if !self.available.load(Ordering::SeqCst) || !self.compute_ready.load(Ordering::SeqCst) {
            return Err(GpuError::DriverUnavailable {
                provider: "wgpu".to_string(),
                detail: self
                    .last_error()
                    .unwrap_or_else(|| "wgpu provider is not initialized".to_string()),
