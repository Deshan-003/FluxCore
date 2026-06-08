# Windows Compatibility (Memory Engine)

Target: Windows 10/11 x64. Every API below is a documented Win32/PDH interface reached through
the official `windows` crate (0.58) or read-only registry access. No kernel drivers, no
undocumented calls, no injection into other processes.

## APIs used and what they provide

| API | Feature | FluxCore use |
| --- | --- | --- |
| `GlobalMemoryStatusEx` | `Win32_System_SystemInformation` | Total/available physical RAM, memory load, total/available pagefile (engine **and** the Phase 1 collector's pagefile used/total pair) |
| `GetNativeSystemInfo` | `Win32_System_SystemInformation` | Phase 1 CPU architecture (`PROCESSOR_ARCHITECTURE_AMD64` → x86-64, `ARM64`, …) |
| `K32GetPerformanceInfo` | `Win32_System_ProcessStatus` | Commit charge (CommitTotal), commit limit, system cache |
| `K32GetProcessMemoryInfo` with `PROCESS_MEMORY_COUNTERS_EX2` | `Win32_System_ProcessStatus` | Per-process private bytes (`PrivateUsage`), working set, peak working set, private working set |
| `OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION)` / `CloseHandle` | `Win32_System_Threading` | Least-privilege handles for per-process reads |
| `CreateFileW` (0 desired access, share read/write) | `Win32_Storage_FileSystem` + `Win32_Security` | Phase 1: opens `\\.\X:` volume handles for storage device queries without elevated rights |
| `DeviceIoControl` | `Win32_System_IO` | Phase 1: issues the storage IOCTLs below |
| `IOCTL_DISK_PERFORMANCE` | `Win32_System_Ioctl` | Phase 1: cumulative per-volume `BytesRead`/`BytesWritten`, differenced between ticks into read/write B/s |
| `IOCTL_STORAGE_QUERY_PROPERTY` (`StorageDeviceProperty`, `StorageDeviceSeekPenaltyProperty`) | `Win32_System_Ioctl` | Phase 1: bus type (17 → NVMe) and seek-penalty flag → NVMe SSD / SSD / HDD classification |
| PDH query API (`PdhOpenQueryW`, `PdhAddEnglishCounterW`, `PdhCollectQueryData`, `PdhGetFormattedCounterValue`, `PdhCloseQuery`) | `Win32_System_Performance` | Counter set listed below |
| `RegOpenKeyExW` / `RegQueryValueExW` / `RegEnumValueW` | `Win32_System_Registry` | Pagefile configuration (`HKLM\SYSTEM\CurrentControlSet\Control\Session Manager\Memory Management\PagingFiles`, REG_MULTI_SZ) and startup Run keys (HKCU/HKLM), read-only |

Other Phase 1 providers: `sysinfo` (CPU/memory/process/volume), CIM `Win32_VideoController`
(identity, driver version), optional NVML via `libloading` (`nvmlInit_v2` once per collector
session, per-tick sensor reads, `nvmlShutdown` on drop) — see
[HARDWARE_COMPATIBILITY.md](HARDWARE_COMPATIBILITY.md).

## PDH counters

English counter names are used so localized Windows installations work identically:

- `\Memory\Compressed Bytes` — Windows memory compression store size
- `\Memory\Standby Cache Core Bytes`, `\Memory\Standby Cache Normal Priority Bytes`,
  `\Memory\Standby Cache Reserve Bytes` — standby cache breakdown (reported as reclaimable)
- `\Memory\Modified Page List Bytes` — modified pages awaiting write
- `\Memory\Cache Bytes` — system cache working size
- `\Memory\Free and Zero Page List Bytes` — truly free pages
- `\Memory\Page Reads/sec` — hard faults (paging I/O)
- `\Memory\Page Faults/sec` — total fault rate
- `\Paging File(_Total)\% Usage` — pagefile utilization

The Phase 1 collector runs one additional, independent PDH query for
`\Processor Information(_Total)\Processor Frequency` (live CPU clock in MHz, `DOUBLE` format,
`CStatus` validated); when the counter is unavailable the OS-registered base clock is shown
instead and no value is invented.

Counters are collected in a single PDH query per tick; values are read as `LARGE`/`DOUBLE`
formatted values through the documented PDH API.

## Graceful degradation

Every counter is optional at runtime:

- If a counter cannot be added or read (missing on the OS edition, performance libraries
  disabled, counter not yet initialized), the analyzer records it in `limitations` and the UI
  shows "Unavailable" plus the limitation note. Analysis continues with the remaining signals.
