use crate::database::Database;
use crate::memory_engine::MemoryEngine;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::{
    sync::{mpsc, Arc, Mutex, RwLock},
    thread,
    time::{Duration, Instant},
};
use sysinfo::{Disks, ProcessesToUpdate, System};

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MonitoringSettings {
    pub refresh_interval_ms: u64,
    pub history_duration_minutes: u32,
    pub persist_history: bool,
    pub monitoring_enabled: bool,
    pub start_on_login: bool,
    pub minimize_to_tray: bool,
    pub theme: String,
}
impl Default for MonitoringSettings {
    fn default() -> Self {
        Self {
            refresh_interval_ms: 1000,
            history_duration_minutes: 10,
            persist_history: true,
            monitoring_enabled: true,
            start_on_login: false,
            minimize_to_tray: false,
            theme: "dark".into(),
        }
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CpuCore {
    pub id: usize,
    pub usage_percent: f32,
    pub frequency_mhz: Option<u64>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CpuMetrics {
    pub name: String,
    pub vendor: String,
    pub architecture: Option<String>,
    pub physical_cores: Option<usize>,
    pub logical_processors: usize,
    pub usage_percent: Option<f32>,
    pub frequency_mhz: Option<u64>,
    pub max_frequency_mhz: Option<u64>,
    pub temperature_c: Option<f32>,
    pub cores: Vec<CpuCore>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoryMetrics {
    pub total_bytes: u64,
    pub used_bytes: u64,
    pub available_bytes: u64,
    pub free_bytes: Option<u64>,
    pub usage_percent: Option<f32>,
    pub cached_bytes: Option<u64>,
    pub committed_bytes: Option<u64>,
    pub commit_limit_bytes: Option<u64>,
    pub pagefile_used_bytes: Option<u64>,
    pub pagefile_total_bytes: Option<u64>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GpuMetrics {
    pub name: String,
    pub vendor: String,
    pub provider: String,
    pub utilization_percent: Option<f32>,
    pub dedicated_used_bytes: Option<u64>,
    pub dedicated_total_bytes: Option<u64>,
    pub dedicated_available_bytes: Option<u64>,
    pub shared_used_bytes: Option<u64>,
    /// Share of dedicated VRAM in use, derived from used/total bytes.
    pub vram_usage_percent: Option<f32>,
    /// NVML memory-controller busy time; not VRAM occupancy.
    pub memory_utilization_percent: Option<f32>,
    pub temperature_c: Option<f32>,
    pub core_clock_mhz: Option<u32>,
    pub memory_clock_mhz: Option<u32>,
    pub power_watts: Option<f32>,
    pub fan_percent: Option<u32>,
    pub driver_version: Option<String>,
    /// "Dedicated"/"Integrated" where the vendor allows a safe inference.
    pub gpu_class: Option<String>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StorageVolume {
    pub name: String,
    pub file_system: String,
    pub kind: String,
    pub total_bytes: u64,
    pub available_bytes: u64,
    pub used_bytes: u64,
    pub usage_percent: Option<f32>,
    pub read_bytes_per_second: Option<u64>,
    pub write_bytes_per_second: Option<u64>,
    pub temperature_c: Option<f32>,
    pub health: Option<String>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProcessMetrics {
    pub pid: u32,
    pub name: String,
    pub cpu_percent: Option<f32>,
    pub memory_bytes: u64,
    pub memory_percent: Option<f32>,
    pub gpu_percent: Option<f32>,
    pub gpu_memory_bytes: Option<u64>,
    pub started_at: Option<String>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HardwareInfo {
    pub os_name: String,
    pub os_version: String,
    pub host_name: String,
    pub gpus: Vec<GpuMetrics>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SystemSnapshot {
    pub collected_at: String,
    pub monitoring: bool,
    pub cpu: CpuMetrics,
    pub memory: MemoryMetrics,
    pub gpus: Vec<GpuMetrics>,
    pub storage: Vec<StorageVolume>,
    pub processes: Vec<ProcessMetrics>,
    pub hardware: HardwareInfo,
    pub errors: Vec<String>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HardwareCheckItem {
    pub component: String,
    pub status: String,
    pub provider: String,
    pub detail: String,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HardwareCheckReport {
    pub checked_at: String,
    pub checks: Vec<HardwareCheckItem>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryPoint {
    pub timestamp: String,
    pub cpu_percent: Option<f32>,
    pub memory_percent: Option<f32>,
    pub gpu_percent: Option<f32>,
    pub vram_used_bytes: Option<u64>,
    pub disk_read_bytes_per_second: Option<u64>,
    pub disk_write_bytes_per_second: Option<u64>,
}

/// Percentage of `part` over `total`, or `None` when the total is unknown so a
/// failed collection is never presented as a real zero.
fn percent(part: u64, total: u64) -> Option<f32> {
    if total == 0 {
        None
    } else {
        Some(part as f32 * 100.0 / total as f32)
    }
}

/// Bytes per second between two cumulative counter readings; `None` when the
/// two readings share a timestamp. Counter resets saturate to zero.
fn rate_bytes_per_second(previous: u64, current: u64, elapsed: Duration) -> Option<u64> {
    let seconds = elapsed.as_secs_f64();
    if seconds <= 0.0 {
        return None;
    }
    Some((current.saturating_sub(previous) as f64 / seconds).round() as u64)
}

/// Drive-type label from documented storage queries: NVMe bus, else seek
/// penalty (rotational media), else an honest "unavailable" label.
fn classify_drive(bus_type: Option<u32>, seek_penalty: Option<bool>, removable: bool) -> String {
    if removable {
        return "Removable volume".into();
    }
    match (bus_type, seek_penalty) {
        (Some(17), _) => "NVMe SSD".into(),
        (_, Some(true)) => "HDD".into(),
        (_, Some(false)) => "SSD".into(),
        _ => "Mounted volume (device type unavailable)".into(),
    }
}

/// Vendor-based GPU class inference; `None` where the vendor alone cannot
/// decide (AMD ships both integrated and dedicated GPUs).
fn gpu_class_for(vendor: &str, nvml_backed: bool) -> Option<String> {
    let vendor = vendor.to_lowercase();
    if nvml_backed || vendor.contains("nvidia") {
        Some("Dedicated".into())
    } else if vendor.contains("intel") {
        Some("Integrated".into())
    } else {
        None
    }
}
impl SystemSnapshot {
    pub fn hardware_check(&self) -> HardwareCheckReport {
        let mut checks = vec![
            HardwareCheckItem {
                component: "CPU".into(), status: if self.cpu.logical_processors > 0 { "available" } else { "unavailable" }.into(),
                provider: "Windows CPU counters / sysinfo".into(),
                detail: if self.cpu.logical_processors > 0 { format!("{} · {} logical processors · live usage available", self.cpu.name, self.cpu.logical_processors) } else { "CPU counters were not returned by Windows.".into() },
            },
            HardwareCheckItem {
                component: "Physical memory".into(), status: if self.memory.total_bytes > 0 { "available" } else { "unavailable" }.into(),
                provider: "Windows memory status API".into(),
                detail: if self.memory.total_bytes > 0 { format!("{} total · {} available", self.memory.total_bytes, self.memory.available_bytes) } else { "Physical memory counters were unavailable.".into() },
            },
            HardwareCheckItem {
                component: "Storage capacity".into(), status: if self.storage.is_empty() { "unavailable" } else { "available" }.into(),
                provider: "Mounted filesystem API".into(),
                detail: if self.storage.is_empty() { "No mounted filesystem volumes were returned.".into() } else { format!("{} mounted volume(s); I/O rates from storage performance counters, device health (SMART) unavailable.", self.storage.len()) },
            },
            HardwareCheckItem {
                component: "Processes".into(), status: if self.processes.is_empty() { "unavailable" } else { "available" }.into(),
                provider: "Windows process counters / sysinfo".into(),
                detail: if self.processes.is_empty() { "No process data was returned.".into() } else { format!("{} processes enumerated; per-process GPU attribution unavailable.", self.processes.len()) },
            },
        ];
        if self.gpus.is_empty() {
            checks.push(HardwareCheckItem { component: "GPU".into(), status: "unavailable".into(), provider: "Windows device enumeration / NVIDIA NVML".into(), detail: "No GPU adapter was enumerated.".into() });
        } else {
            for gpu in &self.gpus {
                let telemetry = gpu.utilization_percent.is_some() || gpu.dedicated_total_bytes.is_some();
                checks.push(HardwareCheckItem {
                    component: gpu.name.clone(), status: if telemetry { "available" } else { "partial" }.into(), provider: gpu.provider.clone(),
                    detail: if telemetry { format!("GPU telemetry active; utilization {}, dedicated VRAM {}.", gpu.utilization_percent.map(|value| format!("{value:.0}%")).unwrap_or_else(|| "unavailable".into()), gpu.dedicated_total_bytes.map(|value| format!("{value} bytes")).unwrap_or_else(|| "unavailable".into())) } else { "Adapter identified by Windows; GPU telemetry provider unavailable. Sensor values are not estimated.".into() },
                });
            }
        }
        HardwareCheckReport { checked_at: self.collected_at.clone(), checks }
    }

    pub fn history_point(&self) -> HistoryPoint {
        HistoryPoint {
            timestamp: self.collected_at.clone(),
            cpu_percent: self.cpu.usage_percent,
            memory_percent: self.memory.usage_percent,
            gpu_percent: self.gpus.iter().find_map(|gpu| gpu.utilization_percent),
            vram_used_bytes: self
                .gpus
                .iter()
                .filter_map(|gpu| gpu.dedicated_used_bytes)
                .reduce(|a, b| a.saturating_add(b)),
            disk_read_bytes_per_second: self
                .storage
                .iter()
                .filter_map(|disk| disk.read_bytes_per_second)
                .reduce(|a, b| a.saturating_add(b)),
            disk_write_bytes_per_second: self
                .storage
                .iter()
                .filter_map(|disk| disk.write_bytes_per_second)
                .reduce(|a, b| a.saturating_add(b)),
        }
    }
}

struct Shared {
    snapshot: RwLock<Option<SystemSnapshot>>,
    settings: RwLock<MonitoringSettings>,
    app: RwLock<Option<tauri::AppHandle>>,
    database: Arc<Database>,
    memory: Arc<MemoryEngine>,
    shutdown: Arc<std::sync::atomic::AtomicBool>,
}
pub struct MonitorEngine {
    shared: Arc<Shared>,
    stop: mpsc::Sender<()>,
    worker: Mutex<Option<thread::JoinHandle<()>>>,
}
/// Validates user-adjustable monitoring settings against their sane ranges.
fn validate_settings(settings: &MonitoringSettings) -> Result<(), String> {
    if settings.theme != "dark" && settings.theme != "light" && settings.theme != "system" {
        return Err("theme must be dark, light, or system".into());
    }
    if !(250..=60000).contains(&settings.refresh_interval_ms) {
        return Err("refresh interval must be between 250 and 60000 milliseconds".into());
    }
    if !(1..=1440).contains(&settings.history_duration_minutes) {
        return Err("history duration must be between 1 and 1440 minutes".into());
    }
    Ok(())
}

impl MonitorEngine {
    pub fn new(database: Arc<Database>, memory: Arc<MemoryEngine>) -> Self {
        let settings = database.load_settings().ok().flatten().unwrap_or_default();
        let shutdown = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let shared = Arc::new(Shared {
            snapshot: RwLock::new(None),
            settings: RwLock::new(settings),
            app: RwLock::new(None),
            database,
            memory,
            shutdown: Arc::clone(&shutdown),
        });
        let (stop, receiver) = mpsc::channel();
        let worker_shared = Arc::clone(&shared);
        let worker = thread::Builder::new()
            .name("fluxcore-monitor".into())
            .spawn(move || monitor_loop(worker_shared, receiver))
            .ok();
        Self {
            shared,
            stop,
            worker: Mutex::new(worker),
        }
    }
    pub fn snapshot(&self) -> Result<SystemSnapshot, String> {
        self.shared
            .snapshot
            .read()
            .map_err(|_| "monitor snapshot lock poisoned".to_string())?
            .clone()
            .ok_or_else(|| "monitor is initializing; retry shortly".to_string())
    }
    pub fn settings(&self) -> Result<MonitoringSettings, String> {
        self.shared
            .settings
            .read()
            .map_err(|_| "settings lock poisoned".to_string())
            .map(|s| s.clone())
    }
    pub fn save_settings(&self, settings: MonitoringSettings) -> Result<(), String> {
        validate_settings(&settings)?;
        self.shared.database.save_settings(&settings)?;
        *self
            .shared
            .settings
            .write()
            .map_err(|_| "settings lock poisoned".to_string())? = settings;
        Ok(())
    }
    pub fn history(&self) -> Result<Vec<HistoryPoint>, String> {
        self.shared.database.history()
    }
    pub fn check_hardware(&self) -> Result<HardwareCheckReport, String> {
        let snapshot = self.snapshot()?;
        Ok(snapshot.hardware_check())
    }
    /// Enables push delivery: every collected snapshot is emitted as the
    /// `fluxcore-snapshot` event in addition to the invoke-based polling API.
    pub fn set_app_handle(&self, handle: tauri::AppHandle) {
        if let Ok(mut app) = self.shared.app.write() {
            *app = Some(handle);
        }
    }
}
impl Drop for MonitorEngine {
    fn drop(&mut self) {
        self.shared
            .shutdown
            .store(true, std::sync::atomic::Ordering::Release);
        let _ = self.stop.send(());
        if let Ok(mut worker) = self.worker.lock() {
            if let Some(handle) = worker.take() {
                let _ = handle.join();
            }
        }
    }
}

fn monitor_loop(shared: Arc<Shared>, stop: mpsc::Receiver<()>) {
    let mut collector = Collector::new();
    let mut last_persist = Instant::now() - Duration::from_secs(10);
    loop {
        if shared.shutdown.load(std::sync::atomic::Ordering::Acquire) {
            break;
        }
        let settings = shared
            .settings
            .read()
            .map(|s| s.clone())
            .unwrap_or_default();
        if settings.monitoring_enabled {
            let snapshot = collector.collect();
            if let Ok(mut current) = shared.snapshot.write() {
                *current = Some(snapshot.clone());
            }
            if let Ok(app) = shared.app.read() {
                if let Some(app) = app.as_ref() {
                    use tauri::Emitter;
                    if let Err(error) = app.emit("fluxcore-snapshot", &snapshot) {
                        eprintln!("FluxCore snapshot event error: {error}");
                    }
                }
            }
            shared.memory.tick(&snapshot);
            if settings.persist_history && last_persist.elapsed() >= Duration::from_secs(5) {
                if let Err(error) = shared.database.record_snapshot(&snapshot) {
                    eprintln!("FluxCore history persistence error: {error}");
                }
                last_persist = Instant::now();
            }
        } else if let Ok(mut current) = shared.snapshot.write() {
            if let Some(snapshot) = current.as_mut() {
                snapshot.monitoring = false;
            }
        }
        let interval = Duration::from_millis(settings.refresh_interval_ms.clamp(250, 60000));
        if stop.recv_timeout(interval).is_ok() {
            break;
        }
    }
}

struct Collector {
    system: System,
    disks: Disks,
    gpu_cache: Vec<GpuMetrics>,
    os_name: String,
    os_version: String,
    host_name: String,
    #[cfg(windows)]
    nvml: Option<NvmlSession>,
    #[cfg(windows)]
    disk_io: DiskIoState,
    #[cfg(windows)]
    cpu_frequency: Option<CpuFrequencyCounter>,
}
impl Collector {
    fn new() -> Self {
        let mut system = System::new_all();
        system.refresh_all();
        let disks = Disks::new_with_refreshed_list();
        let mut gpu_errors = Vec::new();
        #[cfg(windows)]
        let nvml = NvmlSession::open(&mut gpu_errors);
        #[cfg(windows)]
        let (gpu_cache, detect_errors) = detect_gpus(nvml.as_ref());
        #[cfg(not(windows))]
        let (gpu_cache, detect_errors) = detect_gpus();
        gpu_errors.extend(detect_errors);
        if !gpu_errors.is_empty() {
            eprintln!("FluxCore GPU detection: {}", gpu_errors.join("; "));
        }
        Self {
            system,
            disks,
            gpu_cache,
            os_name: System::long_os_version()
                .unwrap_or_else(|| "Windows (version unavailable)".into()),
            os_version: System::kernel_version().unwrap_or_else(|| "Unavailable".into()),
            host_name: System::host_name().unwrap_or_else(|| "Unknown PC".into()),
            #[cfg(windows)]
            nvml,
            #[cfg(windows)]
            disk_io: DiskIoState::default(),
            #[cfg(windows)]
            cpu_frequency: CpuFrequencyCounter::open(),
        }
    }
    #[cfg(windows)]
    fn cpu_frequency_mhz(&mut self) -> Option<u64> {
        self.cpu_frequency.as_mut().and_then(|counter| counter.read())
    }
    #[cfg(not(windows))]
    fn cpu_frequency_mhz(&mut self) -> Option<u64> {
        None
    }
    #[cfg(windows)]
    fn disk_rates(&mut self, name: &str) -> (Option<u64>, Option<u64>) {
        let Some((read_bytes, write_bytes)) = windows_disk_io(name) else {
            return (None, None);
        };
        let now = Instant::now();
        let rates = match self.disk_io.last.get(name) {
            Some((previous_at, previous_read, previous_write)) => {
                let elapsed = now.duration_since(*previous_at);
                (
                    rate_bytes_per_second(*previous_read, read_bytes, elapsed),
                    rate_bytes_per_second(*previous_write, write_bytes, elapsed),
                )
            }
            None => (None, None),
        };
        self.disk_io
            .last
            .insert(name.to_string(), (now, read_bytes, write_bytes));
        rates
    }
    #[cfg(not(windows))]
    fn disk_rates(&mut self, _name: &str) -> (Option<u64>, Option<u64>) {
        (None, None)
    }
    fn collect(&mut self) -> SystemSnapshot {
        self.system.refresh_cpu_usage();
        self.system.refresh_memory();
        self.system.refresh_processes(ProcessesToUpdate::All, true);
        self.disks.refresh(true);
        // Sample the live clock before `cpus` holds an immutable borrow of the
        // system state; the PDH counter needs &mut self.
        let live_frequency = self.cpu_frequency_mhz();
        let cpus = self.system.cpus();
        let usage = if cpus.is_empty() {
            None
        } else {
            Some(
                (cpus.iter().map(|cpu| cpu.cpu_usage()).sum::<f32>() / cpus.len() as f32)
                    .clamp(0.0, 100.0),
            )
        };
        // sysinfo reports the OS-registered base clock on Windows; the live
        // clock comes from the PDH processor-frequency counter when present.
        let base_clock = if cpus.is_empty() {
            None
        } else {
            (cpus.iter().map(|cpu| cpu.frequency()).sum::<u64>() / cpus.len() as u64)
                .try_into()
                .ok()
                .filter(|value: &u64| *value > 0)
        };
        let first = cpus.first();
        let cpu = CpuMetrics {
            name: first
                .map(|cpu| cpu.brand().trim().to_string())
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| "Processor name unavailable".into()),
            vendor: first
                .map(|cpu| cpu.vendor_id().trim().to_string())
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| "Unavailable".into()),
            architecture: cpu_architecture(),
            physical_cores: self.system.physical_core_count(),
            logical_processors: cpus.len(),
            usage_percent: usage,
            frequency_mhz: live_frequency.or(base_clock),
            max_frequency_mhz: base_clock,
            temperature_c: None,
            cores: cpus
                .iter()
                .enumerate()
                .map(|(id, cpu)| CpuCore {
                    id,
                    usage_percent: cpu.cpu_usage().clamp(0.0, 100.0),
                    frequency_mhz: (cpu.frequency() > 0).then_some(cpu.frequency()),
                })
                .collect(),
        };
        let total = self.system.total_memory();
        let available = self.system.available_memory();
        let used = total.saturating_sub(available);
        let (
            cached_bytes,
            committed_bytes,
            commit_limit_bytes,
        ) = windows_memory_accounting();
        let (pagefile_used_bytes, pagefile_total_bytes) = windows_pagefile();
        let memory = MemoryMetrics {
            total_bytes: total,
            used_bytes: used,
            available_bytes: available,
            free_bytes: None,
            usage_percent: percent(used, total),
            cached_bytes,
            committed_bytes,
            commit_limit_bytes,
            pagefile_used_bytes,
            pagefile_total_bytes,
        };
        let mut errors = Vec::new();
        if cpus.is_empty() {
            errors.push("CPU counters were not returned by the operating system.".into());
        }
        if total == 0 {
            errors.push("Physical memory counters were unavailable.".into());
        }
        #[cfg(windows)]
        let gpus = refresh_gpu_metrics(self.nvml.as_ref(), &self.gpu_cache, &mut errors);
        #[cfg(not(windows))]
        let gpus = refresh_gpu_metrics(&self.gpu_cache, &mut errors);
        // Two passes: snapshot the immutable disk descriptors first, then take
        // &mut self for the I/O rate deltas without overlapping borrows.
        let disk_descriptors = self
            .disks
            .list()
            .iter()
            .map(|disk| {
                (
                    disk.mount_point().to_string_lossy().to_string(),
                    disk.file_system().to_string_lossy().to_string(),
                    drive_kind(disk),
                    disk.total_space(),
                    disk.available_space(),
                )
            })
            .collect::<Vec<_>>();
        let disk_metrics = disk_descriptors
            .into_iter()
            .map(|(name, file_system, kind, total, available)| {
                let used = total.saturating_sub(available);
                let (read_bytes_per_second, write_bytes_per_second) = self.disk_rates(&name);
                StorageVolume {
                    file_system,
                    kind,
                    total_bytes: total,
                    available_bytes: available,
                    used_bytes: used,
                    usage_percent: percent(used, total),
                    read_bytes_per_second,
                    write_bytes_per_second,
                    temperature_c: None,
                    health: None,
                    name,
                }
            })
            .collect::<Vec<_>>();
        let process_metrics = self
            .system
            .processes()
            .iter()
            .map(|(pid, process)| ProcessMetrics {
                pid: pid.as_u32(),
                name: process.name().to_string_lossy().to_string(),
                cpu_percent: (process.cpu_usage() <= 100.0).then_some(process.cpu_usage()),
                memory_bytes: process.memory(),
                memory_percent: percent(process.memory(), total),
                gpu_percent: None,
                gpu_memory_bytes: None,
                started_at: chrono::DateTime::from_timestamp(process.start_time() as i64, 0)
                    .map(|started| started.to_rfc3339()),
            })
            .collect::<Vec<_>>();
        if self.system.processes().is_empty() {
            errors.push("Process information is unavailable.".into());
        }
        SystemSnapshot {
            collected_at: Utc::now().to_rfc3339(),
            monitoring: true,
            cpu,
            memory,
            gpus,
            storage: disk_metrics,
            processes: process_metrics,
            hardware: HardwareInfo {
                os_name: self.os_name.clone(),
                os_version: self.os_version.clone(),
                host_name: self.host_name.clone(),
                gpus: self.gpu_cache.clone(),
            },
            errors,
        }
    }
}

#[cfg(windows)]
fn windows_memory_accounting() -> (Option<u64>, Option<u64>, Option<u64>) {
    use std::mem::size_of;
    use windows::Win32::System::ProcessStatus::{K32GetPerformanceInfo, PERFORMANCE_INFORMATION};
    unsafe {
        let mut info = PERFORMANCE_INFORMATION::default();
        info.cb = size_of::<PERFORMANCE_INFORMATION>() as u32;
        let info_size = info.cb;
        if K32GetPerformanceInfo(&mut info, info_size).as_bool() {
            let page = info.PageSize as u64;
            let commit = (info.CommitTotal as u64).saturating_mul(page);
            let limit = (info.CommitLimit as u64).saturating_mul(page);
            let cached = (info.SystemCache as u64).saturating_mul(page);
            (Some(cached), Some(commit), Some(limit))
        } else {
            (None, None, None)
        }
    }
}

#[cfg(not(windows))]
fn windows_memory_accounting() -> (Option<u64>, Option<u64>, Option<u64>) {
    (None, None, None)
}

/// Pagefile usage from `GlobalMemoryStatusEx`: the commit-backed page file
/// total and the part of it currently in use.
#[cfg(windows)]
fn windows_pagefile() -> (Option<u64>, Option<u64>) {
    use std::mem::size_of;
    use windows::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
    unsafe {
        let mut status = MEMORYSTATUSEX::default();
        status.dwLength = size_of::<MEMORYSTATUSEX>() as u32;
        if GlobalMemoryStatusEx(&mut status).is_ok() {
            let total = status.ullTotalPageFile;
            (Some(total.saturating_sub(status.ullAvailPageFile)), Some(total))
        } else {
            (None, None)
        }
    }
}

#[cfg(not(windows))]
fn windows_pagefile() -> (Option<u64>, Option<u64>) {
    (None, None)
}

#[cfg(windows)]
fn cpu_architecture() -> Option<String> {
    use windows::Win32::System::SystemInformation::*;
    unsafe {
        let mut info = SYSTEM_INFO::default();
        GetNativeSystemInfo(&mut info);
        Some(
            match info.Anonymous.Anonymous.wProcessorArchitecture {
                PROCESSOR_ARCHITECTURE_AMD64 => "x86-64",
                PROCESSOR_ARCHITECTURE_INTEL => "x86",
                PROCESSOR_ARCHITECTURE_ARM64 => "ARM64",
                PROCESSOR_ARCHITECTURE_ARM => "ARM",
                _ => "Unknown",
            }
            .to_string(),
        )
    }
}

#[cfg(not(windows))]
fn cpu_architecture() -> Option<String> {
    Some(std::env::consts::ARCH.to_string())
}

fn drive_kind(disk: &sysinfo::Disk) -> String {
    let removable = disk.is_removable();
    #[cfg(windows)]
    {
        let mount = disk.mount_point().to_string_lossy().to_string();
        let (bus_type, seek_penalty) = windows_drive_properties(&mount);
        classify_drive(bus_type, seek_penalty, removable)
    }
    #[cfg(not(windows))]
    {
        classify_drive(None, None, removable)
    }
}

/// Cumulative read/write byte counters for one volume via the documented
/// `IOCTL_DISK_PERFORMANCE` query (no elevated rights required).
#[cfg(windows)]
fn windows_disk_io(mount_point: &str) -> Option<(u64, u64)> {
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::Storage::FileSystem::{CreateFileW, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING};
    use windows::Win32::System::IO::DeviceIoControl;
    use windows::Win32::System::Ioctl::{DISK_PERFORMANCE, IOCTL_DISK_PERFORMANCE};
    use windows::core::PCWSTR;
    let device = volume_device_path(mount_point)?;
    let path = windows::core::HSTRING::from(device);
    unsafe {
        let handle = CreateFileW(
            PCWSTR::from_raw(path.as_ptr()),
            0,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            None,
            OPEN_EXISTING,
            Default::default(),
            None,
        )
        .ok()?;
        let mut performance = DISK_PERFORMANCE::default();
        let mut returned = 0u32;
        let result = DeviceIoControl(
            handle,
            IOCTL_DISK_PERFORMANCE,
            None,
            0,
            Some(&mut performance as *mut _ as *mut _),
            std::mem::size_of::<DISK_PERFORMANCE>() as u32,
            Some(&mut returned),
            None,
        );
        let _ = CloseHandle(handle);
        result
            .ok()
