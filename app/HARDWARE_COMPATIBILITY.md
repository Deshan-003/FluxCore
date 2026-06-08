# Hardware compatibility

FluxCore does not require administrator privileges. Actual sensor availability depends on Windows, device firmware, driver version and vendor-provided APIs.

| Feature | Current provider | Behavior / limitation |
|---|---|---|
| CPU utilization and per-core usage | `sysinfo` OS counters | Collected at the configured interval. If the OS returns no core counters, utilization serializes as `null` and a diagnostic error is surfaced — it is never shown as 0%. |
| CPU model / vendor / core counts | OS CPU descriptors | Physical core count may be unavailable. |
| CPU architecture | `GetNativeSystemInfo` (`PROCESSOR_ARCHITECTURE_*`) | Reported as e.g. `x86-64` or `ARM64`; `null` when the OS call fails. |
| CPU frequency | PDH `\Processor Information(_Total)\Processor Frequency` + OS descriptors | Current MHz is the live PDH counter where present, otherwise the OS-registered base clock. The base clock is also exposed as the base/maximum frequency. No value is fabricated when both are unavailable. |
| CPU temperature | None | Unavailable; no unsafe/undocumented sensor access. |
| Physical and committed memory | `GlobalMemoryStatusEx`, `K32GetPerformanceInfo`, `sysinfo` | Physical used = total − available. Commit charge, commit limit and system cache are kept separate from physical RAM. |
| Pagefile usage / capacity | `GlobalMemoryStatusEx` (`TotalPageFile` / `AvailPageFile`) | Windows reports the commit-limit pair; used = total − available. |
| Process CPU and resident memory | Windows process APIs through `sysinfo` | CPU is provider-reported process usage; per-process memory percent is computed against physical total. GPU attribution is unavailable. |
| GPU identity, driver version, class | Windows `Win32_VideoController` CIM (+ NVML for NVIDIA) | Read-only detection. Driver version comes from CIM. Class is inferred conservatively: NVML-backed or NVIDIA → Dedicated, Intel → Integrated, otherwise Unknown. Adapters reported by both NVML and CIM are de-duplicated. |
| NVIDIA utilization/VRAM/temperature/clocks/power/fan | Dynamic NVML (`nvml.dll` / `nvml64.dll`) | Requires a compatible installed NVIDIA driver. NVML is initialized once per collector session and shut down on drop; every supported sensor is refreshed on each tick. Individual NVML functions may be unsupported and report unavailable. |
| AMD/Intel GPU telemetry | None | Adapter can be identified; telemetry reports unavailable. No generic stable Windows API provides these metrics across vendors. |
| Shared GPU memory and GPU memory budget | None | Unavailable in the current provider implementation; serialized as `null` and never conflated with system RAM or dedicated VRAM. |
| Volume size/free space | `sysinfo` mounted-volume API | Per mounted filesystem; not a physical device inventory. |
| Drive type (NVMe SSD / SSD / HDD / removable) | `IOCTL_STORAGE_QUERY_PROPERTY` | Bus type 17 → NVMe SSD; seek-penalty property → SSD/HDD; removable flag → removable volume. When the device does not answer the query the UI shows "Mounted volume (device type unavailable)" instead of guessing. |
| SSD/HDD temperature and SMART health | None | Unavailable; the volume provider does not expose them. |
| Disk read/write throughput | `IOCTL_DISK_PERFORMANCE` deltas between collection ticks | Per-volume read/write bytes-per-second derived from cumulative counters. `null` until the first successful delta or when the IOCTL is unsupported — capacity usage is never presented as disk activity. |

GPU adapter detection is best-effort and attempts to continue when NVML is absent or fails. Unsupported hardware is a normal state and should not prevent CPU, memory, process or volume collection.

VRAM is dedicated GPU framebuffer memory. It is tracked in separate nullable fields from system RAM and shared GPU memory; the dashboard never presents one as the other.
