//! Concrete execution backends for Phase 3 (`gpu_engine/providers`).
//!
//! Owned by Subagent 2. Exact public surface: `PHASE3_CONTRACT_FREEZE.md` §4.
//! This module is deliberately a thin seam: it owns the `GpuProvider` trait,
//! the provider factory functions and the small shared helpers the three
//! backends use (CPU-reference validation and cooperative cancel/timeout
//! checks). All real compute lives in `cpu.rs`, `cuda.rs` and `wgpu.rs`.

pub mod cpu;
pub mod cuda;
pub mod wgpu;

pub use cpu::CpuProvider;
pub use cuda::CudaProvider;
pub use wgpu::WgpuProvider;

// Provider-side shared data lives in `models.rs` (leaf module). Re-export the
// subset the backends and the rest of the engine need so callers can do
// `use crate::gpu_engine::providers::{GpuProvider, ExecutionRequest, ...}`.
pub use crate::gpu_engine::models::{
    AccelerationEnvironment, BackendKind, CancelToken, CapabilityReport, ExecutionOutcome,
    ExecutionRequest, GpuDeviceInfo, GpuMemoryInfo, ProviderCapabilities, ProviderKind,
    ProviderStatus, TimingBreakdown, WorkloadInputs, WorkloadParams, WorkloadRequirement,
    WorkloadSupport,
};
pub use crate::gpu_engine::error::{GpuError, GpuResult};

use std::sync::Arc;
use std::time::{Duration, Instant};

/// A concrete GPU (or CPU) execution backend.
///
/// Implementations must be cheap to construct, safe to drop at any time, and
/// must never panic across the trait boundary: every failure is a `GpuError`.
/// All state is behind interior mutability because providers are shared as
/// `Arc<dyn GpuProvider>` across scheduler worker threads.
pub trait GpuProvider: Send + Sync {
    /// Stable id persisted in SQLite and shown in the UI ("cpu"|"cuda"|"wgpu").
    fn id(&self) -> &'static str;
    /// Human label.
    fn label(&self) -> &'static str;
    /// Coarse kind for UI grouping.
    fn kind(&self) -> ProviderKind;
    /// One-time initialisation. Must be idempotent and must not block > ~2 s.
    fn initialize(&self) -> GpuResult<Vec<GpuDeviceInfo>>;
    /// True when `initialize` succeeded and at least one device is usable.
    fn is_available(&self) -> bool;
    /// Devices from the most recent successful `initialize`.
    fn devices(&self) -> Vec<GpuDeviceInfo>;
    /// Live capability matrix.
    fn capabilities(&self) -> ProviderCapabilities;
    /// Workload ids this provider can actually execute, with per-impl limits.
    fn supported_workloads(&self) -> Vec<WorkloadSupport>;
    /// Memory snapshot as the provider sees it (may be `None` for CPU).
