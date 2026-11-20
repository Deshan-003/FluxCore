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
fn normalize_process_cpu(raw_percent: f32, logical_processors: usize) -> Option<f32> {
    (logical_processors > 0 && raw_percent.is_finite())
        .then(|| (raw_percent / logical_processors as f32).clamp(0.0, 100.0))
}

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
                component: "CPU".into(),
                status: if self.cpu.logical_processors > 0 {
                    "available"
                } else {
                    "unavailable"
                }
                .into(),
                provider: "Windows CPU counters / sysinfo".into(),
                detail: if self.cpu.logical_processors > 0 {
                    format!(
                        "{} · {} logical processors · live usage available",
                        self.cpu.name, self.cpu.logical_processors
                    )
                } else {
                    "CPU counters were not returned by Windows.".into()
                },
            },
            HardwareCheckItem {
                component: "Physical memory".into(),
                status: if self.memory.total_bytes > 0 {
                    "available"
                } else {
                    "unavailable"
                }
                .into(),
                provider: "Windows memory status API".into(),
                detail: if self.memory.total_bytes > 0 {
                    format!(
                        "{} total · {} available",
                        self.memory.total_bytes, self.memory.available_bytes
                    )
                } else {
                    "Physical memory counters were unavailable.".into()
                },
            },
            HardwareCheckItem {
                component: "Storage capacity".into(),
                status: if self.storage.is_empty() {
                    "unavailable"
                } else {
                    "available"
                }
                .into(),
                provider: "Mounted filesystem API".into(),
                detail: if self.storage.is_empty() {
                    "No mounted filesystem volumes were returned.".into()
                } else {
                    format!("{} mounted volume(s); I/O rates from storage performance counters, device health (SMART) unavailable.", self.storage.len())
                },
            },
            HardwareCheckItem {
                component: "Processes".into(),
                status: if self.processes.is_empty() {
                    "unavailable"
                } else {
                    "available"
                }
                .into(),
                provider: "Windows process counters / sysinfo".into(),
                detail: if self.processes.is_empty() {
                    "No process data was returned.".into()
                } else {
                    format!(
                        "{} processes enumerated; per-process GPU attribution unavailable.",
                        self.processes.len()
                    )
                },
            },
        ];
        if self.gpus.is_empty() {
            checks.push(HardwareCheckItem {
                component: "GPU".into(),
                status: "unavailable".into(),
                provider: "Windows device enumeration / NVIDIA NVML".into(),
                detail: "No GPU adapter was enumerated.".into(),
            });
        } else {
            for gpu in &self.gpus {
                let telemetry =
                    gpu.utilization_percent.is_some() || gpu.dedicated_total_bytes.is_some();
                checks.push(HardwareCheckItem {
                    component: gpu.name.clone(), status: if telemetry { "available" } else { "partial" }.into(), provider: gpu.provider.clone(),
                    detail: if telemetry { format!("GPU telemetry active; utilization {}, dedicated VRAM {}.", gpu.utilization_percent.map(|value| format!("{value:.0}%")).unwrap_or_else(|| "unavailable".into()), gpu.dedicated_total_bytes.map(|value| format!("{value} bytes")).unwrap_or_else(|| "unavailable".into())) } else { "Adapter identified by Windows; GPU telemetry provider unavailable. Sensor values are not estimated.".into() },
                });
            }
        }
        HardwareCheckReport {
            checked_at: self.collected_at.clone(),
            checks,
        }
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
    intelligence: Option<Arc<crate::intelligence::IntelligenceEngine>>,
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
        Self::with_intelligence(database, memory, None)
    }

    pub fn with_intelligence(
        database: Arc<Database>,
        memory: Arc<MemoryEngine>,
        intelligence: Option<Arc<crate::intelligence::IntelligenceEngine>>,
    ) -> Self {
        let settings = database.load_settings().ok().flatten().unwrap_or_default();
        let shutdown = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let shared = Arc::new(Shared {
            snapshot: RwLock::new(None),
            settings: RwLock::new(settings),
            app: RwLock::new(None),
            database,
            memory,
            intelligence,
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
        if let Some(intelligence) = &shared.intelligence {
            intelligence.collection_status(settings.monitoring_enabled, settings.persist_history);
        }
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
            if let Some(intelligence) = &shared.intelligence {
                intelligence.tick(&snapshot);
            }
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
        self.cpu_frequency
            .as_mut()
            .and_then(|counter| counter.read())
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
        let (cached_bytes, committed_bytes, commit_limit_bytes) = windows_memory_accounting();
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
                cpu_percent: normalize_process_cpu(process.cpu_usage(), cpu.logical_processors),
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
            (
                Some(total.saturating_sub(status.ullAvailPageFile)),
                Some(total),
            )
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
    use windows::core::PCWSTR;
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::Storage::FileSystem::{
        CreateFileW, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
    };
    use windows::Win32::System::Ioctl::{DISK_PERFORMANCE, IOCTL_DISK_PERFORMANCE};
    use windows::Win32::System::IO::DeviceIoControl;
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
        result.ok().map(|_| {
            (
                performance.BytesRead as u64,
                performance.BytesWritten as u64,
            )
        })
    }
}

/// Bus type and rotational-media flag from documented storage queries.
#[cfg(windows)]
fn windows_drive_properties(mount_point: &str) -> (Option<u32>, Option<bool>) {
    use windows::core::PCWSTR;
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::Storage::FileSystem::{
        CreateFileW, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
    };
    use windows::Win32::System::Ioctl::{
        PropertyStandardQuery, StorageDeviceProperty, StorageDeviceSeekPenaltyProperty,
        DEVICE_SEEK_PENALTY_DESCRIPTOR, IOCTL_STORAGE_QUERY_PROPERTY, STORAGE_DEVICE_DESCRIPTOR,
        STORAGE_PROPERTY_QUERY,
    };
    use windows::Win32::System::IO::DeviceIoControl;
    let Some(device) = volume_device_path(mount_point) else {
        return (None, None);
    };
    let path = windows::core::HSTRING::from(device);
    unsafe {
        let Ok(handle) = CreateFileW(
            PCWSTR::from_raw(path.as_ptr()),
            0,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            None,
            OPEN_EXISTING,
            Default::default(),
            None,
        ) else {
            return (None, None);
        };
        let mut query = STORAGE_PROPERTY_QUERY::default();
        query.PropertyId = StorageDeviceProperty;
        query.QueryType = PropertyStandardQuery;
        let mut descriptor: STORAGE_DEVICE_DESCRIPTOR = std::mem::zeroed();
        let mut returned = 0u32;
        let bus = DeviceIoControl(
            handle,
            IOCTL_STORAGE_QUERY_PROPERTY,
            Some(&query as *const _ as *const _),
            std::mem::size_of::<STORAGE_PROPERTY_QUERY>() as u32,
            Some(&mut descriptor as *mut _ as *mut _),
            std::mem::size_of::<STORAGE_DEVICE_DESCRIPTOR>() as u32,
            Some(&mut returned),
            None,
        )
        .is_ok()
        .then(|| descriptor.BusType.0 as u32);
        let mut penalty_query = STORAGE_PROPERTY_QUERY::default();
        penalty_query.PropertyId = StorageDeviceSeekPenaltyProperty;
        penalty_query.QueryType = PropertyStandardQuery;
        let mut penalty = DEVICE_SEEK_PENALTY_DESCRIPTOR::default();
        let seek_penalty = DeviceIoControl(
            handle,
            IOCTL_STORAGE_QUERY_PROPERTY,
            Some(&penalty_query as *const _ as *const _),
            std::mem::size_of::<STORAGE_PROPERTY_QUERY>() as u32,
            Some(&mut penalty as *mut _ as *mut _),
            std::mem::size_of::<DEVICE_SEEK_PENALTY_DESCRIPTOR>() as u32,
            Some(&mut returned),
            None,
        )
        .is_ok()
        .then(|| penalty.IncursSeekPenalty.as_bool());
        let _ = CloseHandle(handle);
        (bus, seek_penalty)
    }
}

#[cfg(windows)]
fn volume_device_path(mount_point: &str) -> Option<String> {
    let drive = mount_point.chars().next()?;
    (drive.is_ascii_alphabetic()).then(|| format!("\\\\.\\{drive}:"))
}

#[cfg(windows)]
type NvmlDevice = *mut std::ffi::c_void;

#[cfg(windows)]
#[repr(C)]
struct NvmlUtilization {
    gpu: u32,
    memory: u32,
}
#[cfg(windows)]
#[repr(C)]
struct NvmlMemoryInfo {
    total: u64,
    free: u64,
    used: u64,
}

/// One NVML initialization shared by every refresh tick. Function pointers are
/// resolved once, device handles are enumerated once, and the init refcount is
/// released by `nvmlShutdown` when the collector drops the session.
#[cfg(windows)]
struct NvmlSession {
    _library: libloading::Library,
    shutdown: Option<unsafe extern "C" fn() -> i32>,
    name: unsafe extern "C" fn(NvmlDevice, *mut i8, u32) -> i32,
    memory: unsafe extern "C" fn(NvmlDevice, *mut NvmlMemoryInfo) -> i32,
    utilization: Option<unsafe extern "C" fn(NvmlDevice, *mut NvmlUtilization) -> i32>,
    temperature: Option<unsafe extern "C" fn(NvmlDevice, u32, *mut u32) -> i32>,
    clock: Option<unsafe extern "C" fn(NvmlDevice, u32, *mut u32) -> i32>,
    power: Option<unsafe extern "C" fn(NvmlDevice, *mut u32) -> i32>,
    fan: Option<unsafe extern "C" fn(NvmlDevice, u32, *mut u32) -> i32>,
    driver_version: Option<unsafe extern "C" fn(*mut i8, u32) -> i32>,
    devices: Vec<NvmlDevice>,
}
#[cfg(windows)]
impl NvmlSession {
    fn open(errors: &mut Vec<String>) -> Option<Self> {
        use libloading::Library;
        let library = unsafe { Library::new("nvml.dll").or_else(|_| Library::new("nvml64.dll")) };
        let Ok(library) = library else {
            errors.push(
                "NVIDIA NVML provider is unavailable; using OS GPU identification only.".into(),
            );
            return None;
        };
        unsafe {
            let Ok(init) = library.get::<unsafe extern "C" fn() -> i32>(b"nvmlInit_v2") else {
                errors.push("NVML is missing required monitoring functions.".into());
                return None;
            };
            if init() != 0 {
                errors.push("NVIDIA driver was found but NVML initialization failed.".into());
                return None;
            }
            let (Ok(count_fn), Ok(by_index), Ok(name), Ok(memory)) = (
                library.get::<unsafe extern "C" fn(*mut u32) -> i32>(b"nvmlDeviceGetCount_v2"),
                library.get::<unsafe extern "C" fn(u32, *mut NvmlDevice) -> i32>(
                    b"nvmlDeviceGetHandleByIndex_v2",
                ),
                library.get::<unsafe extern "C" fn(NvmlDevice, *mut i8, u32) -> i32>(
                    b"nvmlDeviceGetName",
                ),
                library.get::<unsafe extern "C" fn(NvmlDevice, *mut NvmlMemoryInfo) -> i32>(
                    b"nvmlDeviceGetMemoryInfo",
                ),
            ) else {
                errors.push("NVML is missing required monitoring functions.".into());
                return None;
            };
            let mut count = 0u32;
            if count_fn(&mut count) != 0 {
                errors.push("NVML loaded, but GPU enumeration failed.".into());
                return None;
            }
            let mut devices = Vec::new();
            for index in 0..count {
                let mut device: NvmlDevice = std::ptr::null_mut();
                if by_index(index, &mut device) == 0 {
                    devices.push(device);
                }
            }
            Some(Self {
                shutdown: library.get(b"nvmlShutdown").ok().map(|f| *f),
                name: *name,
                memory: *memory,
                utilization: library
                    .get(b"nvmlDeviceGetUtilizationRates")
                    .ok()
                    .map(|f| *f),
                temperature: library.get(b"nvmlDeviceGetTemperature").ok().map(|f| *f),
                clock: library.get(b"nvmlDeviceGetClockInfo").ok().map(|f| *f),
                power: library.get(b"nvmlDeviceGetPowerUsage").ok().map(|f| *f),
                fan: library.get(b"nvmlDeviceGetFanSpeed_v2").ok().map(|f| *f),
                driver_version: library.get(b"nvmlSystemGetDriverVersion").ok().map(|f| *f),
                _library: library,
                devices,
            })
        }
    }

    /// Reads every live sensor for one device; unavailable sensors stay None.
    fn metrics(&self, device: NvmlDevice) -> GpuMetrics {
        unsafe {
            let mut name = [0i8; 96];
            let display_name = if (self.name)(device, name.as_mut_ptr(), name.len() as u32) == 0 {
                std::ffi::CStr::from_ptr(name.as_ptr())
                    .to_string_lossy()
                    .to_string()
            } else {
                "NVIDIA GPU".to_string()
            };
            let mut memory_slot = std::mem::MaybeUninit::<NvmlMemoryInfo>::uninit();
            let memory = ((self.memory)(device, memory_slot.as_mut_ptr()) == 0)
                .then(|| memory_slot.assume_init());
            let utilization = self.utilization.and_then(|f| {
                let mut u = NvmlUtilization { gpu: 0, memory: 0 };
                (f(device, &mut u) == 0).then_some((u.gpu as f32, u.memory as f32))
            });
            let temperature = self.temperature.and_then(|f| {
                let mut v = 0u32;
                (f(device, 0, &mut v) == 0).then_some(v as f32)
            });
            let core_clock = self.clock.and_then(|f| {
                let mut v = 0u32;
                (f(device, 0, &mut v) == 0).then_some(v)
            });
            let memory_clock = self.clock.and_then(|f| {
                let mut v = 0u32;
                (f(device, 2, &mut v) == 0).then_some(v)
            });
            let power = self.power.and_then(|f| {
                let mut v = 0u32;
                (f(device, &mut v) == 0).then_some(v as f32 / 1000.0)
            });
            let fan = self.fan.and_then(|f| {
                let mut v = 0u32;
                (f(device, 0, &mut v) == 0).then_some(v)
            });
            let driver_version = self.driver_version.and_then(|f| {
                let mut buffer = [0i8; 96];
                (f(buffer.as_mut_ptr(), buffer.len() as u32) == 0).then(|| {
                    std::ffi::CStr::from_ptr(buffer.as_ptr())
                        .to_string_lossy()
                        .to_string()
                })
            });
            let dedicated_used = memory.as_ref().map(|m| m.used);
            let dedicated_total = memory.as_ref().map(|m| m.total);
            GpuMetrics {
                name: display_name,
                vendor: "NVIDIA".into(),
                provider: "NVIDIA NVML".into(),
                utilization_percent: utilization.map(|u| u.0),
                dedicated_used_bytes: dedicated_used,
                dedicated_total_bytes: dedicated_total,
                dedicated_available_bytes: memory.as_ref().map(|m| m.free),
                shared_used_bytes: None,
                vram_usage_percent: dedicated_used
                    .zip(dedicated_total)
                    .and_then(|(used, total)| percent(used, total)),
                memory_utilization_percent: utilization.map(|u| u.1),
                temperature_c: temperature,
                core_clock_mhz: core_clock,
                memory_clock_mhz: memory_clock,
                power_watts: power,
                fan_percent: fan,
                driver_version,
                gpu_class: Some("Dedicated".into()),
            }
        }
    }
}
#[cfg(windows)]
impl Drop for NvmlSession {
    fn drop(&mut self) {
        if let Some(shutdown) = self.shutdown {
            unsafe {
                let _ = shutdown();
            }
        }
    }
}

/// Cumulative per-volume I/O counters from the previous tick.
#[cfg(windows)]
#[derive(Default)]
struct DiskIoState {
    last: std::collections::HashMap<String, (Instant, u64, u64)>,
}

/// Live PDH read of the system processor frequency (MHz), when the counter
/// exists on this Windows build.
#[cfg(windows)]
struct CpuFrequencyCounter {
    query: isize,
    counter: isize,
}
#[cfg(windows)]
impl CpuFrequencyCounter {
    fn open() -> Option<Self> {
        use windows::core::w;
        use windows::Win32::System::Performance::*;
        // PDH functions in windows 0.58 return raw u32 status codes.
        const ERROR_SUCCESS: u32 = 0;
        unsafe {
            let mut query: isize = 0;
            if PdhOpenQueryW(None, 0, &mut query) != ERROR_SUCCESS {
                return None;
            }
            let mut counter: isize = 0;
            let added = PdhAddEnglishCounterW(
                query,
                w!(r"\Processor Information(_Total)\Processor Frequency"),
                0,
                &mut counter,
            ) == ERROR_SUCCESS;
            if !added {
                let _ = PdhCloseQuery(query);
                return None;
            }
            Some(Self { query, counter })
        }
    }
    fn read(&mut self) -> Option<u64> {
        use windows::Win32::System::Performance::*;
        const ERROR_SUCCESS: u32 = 0;
        unsafe {
            if PdhCollectQueryData(self.query) != ERROR_SUCCESS {
                return None;
            }
            let mut value = PDH_FMT_COUNTERVALUE::default();
            if PdhGetFormattedCounterValue(self.counter, PDH_FMT_DOUBLE, None, &mut value)
                != ERROR_SUCCESS
                || value.CStatus != 0
            {
                return None;
            }
            let mhz = value.Anonymous.doubleValue;
            (mhz > 0.0).then_some(mhz as u64)
        }
    }
}
#[cfg(windows)]
impl Drop for CpuFrequencyCounter {
    fn drop(&mut self) {
        unsafe {
            let _ = windows::Win32::System::Performance::PdhCloseQuery(self.query);
        }
    }
}

#[cfg(windows)]
fn detect_gpus(nvml: Option<&NvmlSession>) -> (Vec<GpuMetrics>, Vec<String>) {
    let mut out = Vec::new();
    let mut errors = Vec::new();
    if let Some(session) = nvml {
        for device in &session.devices {
            out.push(session.metrics(*device));
        }
    }
    let system_gpus = windows_gpu_names();
    for (name, vendor, driver) in system_gpus {
        let nvml_present = out.iter().any(|gpu| gpu.provider == "NVIDIA NVML");
        if vendor.to_lowercase().contains("nvidia") && nvml_present {
            continue;
        }
        let known = out.iter().any(|gpu| gpu.name.eq_ignore_ascii_case(&name));
        if !known {
            out.push(GpuMetrics {
                driver_version: driver,
                gpu_class: gpu_class_for(&vendor, false),
                vram_usage_percent: None,
                name,
                vendor: vendor.clone(),
                provider: "Windows device identification".into(),
                utilization_percent: None,
                dedicated_used_bytes: None,
                dedicated_total_bytes: None,
                dedicated_available_bytes: None,
                shared_used_bytes: None,
                memory_utilization_percent: None,
                temperature_c: None,
                core_clock_mhz: None,
                memory_clock_mhz: None,
                power_watts: None,
                fan_percent: None,
            });
        }
    }
    if out.is_empty() {
        errors
            .push("No GPU monitoring provider was available; GPU metrics are unavailable.".into());
    }
    (out, errors)
}

#[cfg(windows)]
fn windows_gpu_names() -> Vec<(String, String, Option<String>)> {
    use std::process::Command;
    let output = Command::new("powershell.exe").args(["-NoProfile","-NonInteractive","-Command","Get-CimInstance Win32_VideoController | Select-Object -Property Name,AdapterCompatibility,DriverVersion | ConvertTo-Json -Compress"]).output();
    let Ok(output) = output else {
        return Vec::new();
    };
    if !output.status.success() {
        return Vec::new();
    }
    let parsed: Result<serde_json::Value, _> = serde_json::from_slice(&output.stdout);
    match parsed {
        Ok(serde_json::Value::Array(items)) => items.iter().filter_map(parse_gpu_entry).collect(),
        Ok(item) => parse_gpu_entry(&item).into_iter().collect(),
        Err(_) => Vec::new(),
    }
}
/// Parses one Win32_VideoController CIM object; platform-independent so the
/// mapping is covered by the native test suite.
fn parse_gpu_entry(item: &serde_json::Value) -> Option<(String, String, Option<String>)> {
    let name = item.get("Name")?.as_str()?.trim().to_string();
    let vendor = item
        .get("AdapterCompatibility")
        .and_then(|v| v.as_str())
        .unwrap_or("Unknown vendor")
        .to_string();
    let driver = item
        .get("DriverVersion")
        .and_then(|v| v.as_str())
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
    (!name.is_empty()).then_some((name, vendor, driver))
}
#[cfg(not(windows))]
fn detect_gpus() -> (Vec<GpuMetrics>, Vec<String>) {
    (
        Vec::new(),
        vec!["GPU providers are implemented for Windows only.".into()],
    )
}

fn refresh_gpu_metrics(
    #[cfg(windows)] nvml: Option<&NvmlSession>,
    cache: &[GpuMetrics],
    errors: &mut Vec<String>,
) -> Vec<GpuMetrics> {
    #[cfg(windows)]
    {
        let _ = errors;
        let Some(session) = nvml else {
            return cache.to_vec();
        };
        let mut refreshed: Vec<GpuMetrics> = session
            .devices
            .iter()
            .map(|device| session.metrics(*device))
            .collect();
        for gpu in cache.iter().filter(|gpu| gpu.provider != "NVIDIA NVML") {
            refreshed.push(gpu.clone());
        }
        refreshed
    }
    #[cfg(not(windows))]
    {
        let _ = errors;
        cache.to_vec()
    }
}

/// Benchmarks the Phase 1 collector: average microseconds per full collect.
/// Used by the Phase 2 benchmark example to quantify engine overhead.
pub fn benchmark_collect(iterations: u32) -> u128 {
    let mut collector = Collector::new();
    let iterations = iterations.max(1);
    let start = Instant::now();
    for _ in 0..iterations {
        let _ = collector.collect();
    }
    start.elapsed().as_micros() / u128::from(iterations)
}

/// Benchmarks the combined Phase 1 + Phase 2 pipeline: one full collect
/// followed by one memory engine tick per iteration.
/// Returns (average collect microseconds, average tick microseconds).
pub fn benchmark_collect_and_tick(iterations: u32, memory: &MemoryEngine) -> (u128, u128) {
    let mut collector = Collector::new();
    let iterations = iterations.max(1);
    let mut collect_micros = 0u128;
    let mut tick_micros = 0u128;
    for _ in 0..iterations {
        let start = Instant::now();
        let snapshot = collector.collect();
        collect_micros += start.elapsed().as_micros();
        let start = Instant::now();
        memory.tick(&snapshot);
        tick_micros += start.elapsed().as_micros();
    }
    (
        collect_micros / u128::from(iterations),
        tick_micros / u128::from(iterations),
    )
}

/// Aggregate timings from [`benchmark_intelligence`], in microseconds per
/// iteration.
#[derive(Clone, Copy, Debug)]
pub struct IntelligenceBenchmark {
    pub iterations: u32,
    /// Phase 1 collector pass.
    pub collect_us: u128,
    /// Phase 2 memory engine tick.
    pub memory_tick_us: u128,
    /// Phase 4 sample including forecast, anomaly and recommendation analysis,
    /// with SQLite persistence disabled.
    pub analysis_us: u128,
    /// Phase 4 sample including the SQLite observation/write transaction.
    pub sample_us: u128,
    /// Difference between `sample_us` and `analysis_us`; the storage-write cost.
    pub persistence_us: u128,
    /// Phase 4 tick inside the 15-second sampling window (early return).
    pub gate_us: u128,
}

/// Measures the Phase 4 intelligence engine on live collected snapshots.
///
/// Metric values are real OS readings; only the snapshot timestamp is shifted
/// by the 15-second cadence so each iteration performs a full analysis and
/// persistence pass instead of the sampler's early return. The benchmark only
/// reads system state and writes to its throwaway database.
pub fn benchmark_intelligence(
    iterations: u32,
    memory: &MemoryEngine,
    intelligence: &crate::intelligence::IntelligenceEngine,
) -> IntelligenceBenchmark {
    let iterations = iterations.clamp(2, 40);
    let mut collector = Collector::new();
    let base = Utc::now();
    let mut collect_micros = 0u128;
    let mut memory_micros = 0u128;
    let mut analysis_micros = 0u128;
    let mut sample_micros = 0u128;

    // In-memory pass: analysis, forecasting and anomaly work only.
    intelligence.collection_status(true, false);
    let mut last: Option<SystemSnapshot> = None;
    for index in 0..iterations {
        let start = Instant::now();
        let mut snapshot = collector.collect();
        collect_micros += start.elapsed().as_micros();
        snapshot.collected_at =
            (base + chrono::Duration::seconds(15 * i64::from(index))).to_rfc3339();
        let start = Instant::now();
        memory.tick(&snapshot);
        memory_micros += start.elapsed().as_micros();
        let start = Instant::now();
        intelligence.tick(&snapshot);
        analysis_micros += start.elapsed().as_micros();
        last = Some(snapshot);
    }

    // Full pass: the same analysis plus the local persistence transaction.
    intelligence.collection_status(true, true);
    for index in 0..iterations {
        let start = Instant::now();
        let mut snapshot = collector.collect();
        collect_micros += start.elapsed().as_micros();
        snapshot.collected_at = (base
            + chrono::Duration::seconds(15 * (i64::from(iterations) + i64::from(index))))
        .to_rfc3339();
        let start = Instant::now();
        memory.tick(&snapshot);
        memory_micros += start.elapsed().as_micros();
        let start = Instant::now();
        intelligence.tick(&snapshot);
        sample_micros += start.elapsed().as_micros();
        last = Some(snapshot);
    }

    // Early-return cost paid by every monitoring tick inside the 15 s window.
    let mut gate_micros = 0u128;
    if let Some(snapshot) = last.as_ref() {
        let runs = 200u32;
        for _ in 0..runs {
            let start = Instant::now();
            intelligence.tick(snapshot);
            gate_micros += start.elapsed().as_micros();
        }
        gate_micros /= u128::from(runs);
    }

    let divisor = u128::from(2 * iterations);
    let collect = collect_micros / divisor;
    let memory_tick = memory_micros / divisor;
    let analysis = analysis_micros / u128::from(iterations);
    let sample = sample_micros / u128::from(iterations);
    IntelligenceBenchmark {
        iterations,
        collect_us: collect,
        memory_tick_us: memory_tick,
        analysis_us: analysis,
        sample_us: sample,
        persistence_us: sample.saturating_sub(analysis),
        gate_us: gate_micros,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn memory_usage_is_physical_used_over_total() {
        let total = 16 * 1024u64.pow(3);
        let available = 4 * 1024u64.pow(3);
        let used = total - available;
        assert_eq!(percent(used, total), Some(75.0));
        assert_eq!(
            percent(used, 0),
            None,
            "unknown totals must not become zeros"
        );
    }
    #[test]
    fn process_cpu_is_normalized_to_total_machine_capacity() {
        assert_eq!(normalize_process_cpu(240.0, 12), Some(20.0));
        assert_eq!(normalize_process_cpu(1200.0, 12), Some(100.0));
        assert_eq!(normalize_process_cpu(10.0, 0), None);
        assert_eq!(normalize_process_cpu(f32::NAN, 12), None);
    }

    #[test]
    fn rate_helper_derives_bytes_per_second() {
        let elapsed = Duration::from_secs(2);
        assert_eq!(rate_bytes_per_second(1000, 3000, elapsed), Some(1000));
        assert_eq!(rate_bytes_per_second(5000, 5000, elapsed), Some(0));
        assert_eq!(
            rate_bytes_per_second(9000, 1000, elapsed),
            Some(0),
            "counter resets saturate instead of going negative"
        );
        assert_eq!(rate_bytes_per_second(1, 2, Duration::ZERO), None);
    }
    #[test]
    fn drive_classification_uses_documented_queries() {
        assert_eq!(classify_drive(Some(17), None, false), "NVMe SSD");
        assert_eq!(classify_drive(Some(3), Some(true), false), "HDD");
        assert_eq!(classify_drive(Some(3), Some(false), false), "SSD");
        assert_eq!(
            classify_drive(None, None, false),
            "Mounted volume (device type unavailable)"
        );
        assert_eq!(
            classify_drive(Some(17), Some(false), true),
            "Removable volume"
        );
    }
    #[test]
    fn gpu_class_inference_is_conservative() {
        assert_eq!(gpu_class_for("NVIDIA", false), Some("Dedicated".into()));
        assert_eq!(
            gpu_class_for("Intel Corporation", false),
            Some("Integrated".into())
        );
        assert_eq!(gpu_class_for("Advanced Micro Devices", false), None);
        assert_eq!(
            gpu_class_for("Advanced Micro Devices", true),
            Some("Dedicated".into())
        );
    }
    #[test]
    fn cim_entry_parser_maps_name_vendor_and_driver() {
        let item = serde_json::json!({
            "Name": " NVIDIA GeForce RTX 2050 ",
            "AdapterCompatibility": "NVIDIA",
            "DriverVersion": "31.0.15.1694"
        });
        let (name, vendor, driver) = parse_gpu_entry(&item).expect("parsed entry");
        assert_eq!(name, "NVIDIA GeForce RTX 2050");
        assert_eq!(vendor, "NVIDIA");
        assert_eq!(driver.as_deref(), Some("31.0.15.1694"));
        let missing =
            serde_json::json!({ "Name": "Basic adapter", "AdapterCompatibility": "Contoso" });
        let (name, _, driver) = parse_gpu_entry(&missing).expect("parsed entry");
        assert_eq!(name, "Basic adapter");
        assert_eq!(driver, None);
        assert!(parse_gpu_entry(&serde_json::json!({ "Name": "  " })).is_none());
    }
    #[test]
    fn snapshot_serialization_uses_camel_case_contract() {
        let cpu = CpuMetrics {
            name: "cpu".into(),
            vendor: "vendor".into(),
            architecture: Some("x86-64".into()),
            physical_cores: Some(1),
            logical_processors: 1,
            usage_percent: None,
            frequency_mhz: None,
            max_frequency_mhz: None,
            temperature_c: None,
            cores: vec![],
        };
        let json = serde_json::to_value(&cpu).expect("serialize");
        assert!(json.get("usagePercent").is_some());
        assert!(json.get("architecture").is_some());
        assert!(json.get("usage_percent").is_none());
        let point = HistoryPoint {
            timestamp: "t".into(),
            cpu_percent: None,
            memory_percent: Some(1.5),
            gpu_percent: None,
            vram_used_bytes: None,
            disk_read_bytes_per_second: Some(7),
            disk_write_bytes_per_second: None,
        };
        let json = serde_json::to_value(&point).expect("serialize");
        assert_eq!(json["cpuPercent"], serde_json::Value::Null);
        assert_eq!(json["diskReadBytesPerSecond"], 7);
    }
    #[test]
    fn history_aggregates_only_available_gpu_memory() {
        let snapshot = SystemSnapshot {
            collected_at: "now".into(),
            monitoring: true,
            cpu: CpuMetrics {
                name: "cpu".into(),
                vendor: "vendor".into(),
                architecture: None,
                physical_cores: Some(1),
                logical_processors: 1,
                usage_percent: Some(10.0),
                frequency_mhz: None,
                max_frequency_mhz: None,
                temperature_c: None,
                cores: vec![],
            },
            memory: MemoryMetrics {
                total_bytes: 100,
                used_bytes: 50,
                available_bytes: 50,
                free_bytes: None,
                usage_percent: Some(50.0),
                cached_bytes: None,
                committed_bytes: None,
                commit_limit_bytes: None,
                pagefile_used_bytes: None,
                pagefile_total_bytes: None,
            },
            gpus: vec![GpuMetrics {
                name: "gpu".into(),
                vendor: "x".into(),
                provider: "x".into(),
                utilization_percent: Some(25.0),
                dedicated_used_bytes: Some(2),
                dedicated_total_bytes: Some(4),
                dedicated_available_bytes: Some(2),
                shared_used_bytes: None,
                vram_usage_percent: Some(50.0),
                memory_utilization_percent: None,
                temperature_c: None,
                core_clock_mhz: None,
                memory_clock_mhz: None,
                power_watts: None,
                fan_percent: None,
                driver_version: None,
                gpu_class: None,
            }],
            storage: vec![],
            processes: vec![],
            hardware: HardwareInfo {
                os_name: "x".into(),
                os_version: "x".into(),
                host_name: "x".into(),
                gpus: vec![],
            },
            errors: vec![],
        };
        let point = snapshot.history_point();
        assert_eq!(point.vram_used_bytes, Some(2));
        assert_eq!(point.gpu_percent, Some(25.0));
        assert_eq!(point.cpu_percent, Some(10.0));
        assert_eq!(point.disk_read_bytes_per_second, None);
    }
    #[test]
    fn settings_reject_unreasonable_intervals() {
        let mut settings = MonitoringSettings::default();
        settings.refresh_interval_ms = 100;
        assert!(
            validate_settings(&settings).is_err(),
            "100 ms is below the 250 ms floor"
        );
        settings.refresh_interval_ms = 60_001;
        assert!(
            validate_settings(&settings).is_err(),
            "over one minute exceeds the ceiling"
        );
        settings.refresh_interval_ms = 1000;
        assert!(validate_settings(&settings).is_ok());
        settings.history_duration_minutes = 0;
        assert!(
            validate_settings(&settings).is_err(),
            "history duration must be at least 1 minute"
        );
    }
    #[test]
    fn monitor_engine_starts_publishes_and_stops() {
        let directory = tempfile::tempdir().expect("temporary database directory");
        let database = Arc::new(
            Database::open_at_path(directory.path().join("engine.sqlite3")).expect("open"),
        );
        let memory = Arc::new(MemoryEngine::new(Arc::clone(&database)));
        let engine = MonitorEngine::new(Arc::clone(&database), memory);
        // The first collection performs one-off OS probing (a sysinfo refresh, an
        // NVML probe and a `powershell.exe Get-CimInstance` GPU enumeration on
        // Windows), which can take several seconds on a cold or loaded machine.
        // Wait on a wall-clock deadline instead of a fixed two second budget.
        let deadline = Instant::now() + Duration::from_secs(30);
        let mut published = false;
        while Instant::now() < deadline {
            if engine.snapshot().is_ok() {
                published = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        assert!(published, "the monitor loop must publish a snapshot");
        assert!(engine.history().is_ok());
        drop(engine);
    }
}
