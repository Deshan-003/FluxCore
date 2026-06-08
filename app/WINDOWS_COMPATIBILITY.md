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
- If the "Memory Compression" process is not visible, compression analysis falls back to the
  PDH counter only, and says so (`compression.limitation`).
- Per-process reads that fail (protected or exited processes) are skipped individually; the
  process simply does not appear in the ranking for that scan.
- `private_working_set_bytes` is `Option<u64>`: exposed only when the EX2 structure is returned
  with a non-zero value.
- Non-Windows compilation targets keep the engine logic testable with `None`/no-op stubs; no
  fake values are produced on Windows itself.

## Known limitations (deliberate)

| Limitation | Reason |
| --- | --- |
| Per-process **virtual size** is not reported | Requires `NtQueryInformationProcess`/`QueryWorkingSetEx` paths that are not exposed as documented APIs in the `windows` crate. Private bytes + working set + private working set cover the meaningful signals. |
| Pagefile configuration is **read-only** | Sizing/moving/disabling the pagefile is guidance-only by policy (see [OPTIMIZATION_SAFETY.md](OPTIMIZATION_SAFETY.md)). Registry values describe configuration; system-managed entries report sizes as 0 until Windows materializes them. |
| Standby-cache breakdown availability | The three standby counters are not present on every SKU/configuration; totals then come from `GlobalMemoryStatusEx` alone and the limitation is surfaced. |
| Hard-fault attribution per process | Windows does not expose per-process page-read rates through documented APIs; fault rates are system-wide only. |
| No working-set manipulation at all | `EmptyWorkingSet`/`SetProcessWorkingSetSize` are intentionally never called — see forbidden operations. |

## Privileges

FluxCore runs **unelevated**. `PROCESS_QUERY_LIMITED_INFORMATION` is sufficient for
private-bytes/working-set reads of normal user processes; processes owned by other users or
protected processes may return nothing and are skipped. Registry reads use `KEY_READ` on
documented keys. PDH counters are available to standard users.

## Verification environment note

This development machine enforces Smart App Control, which blocks locally built unsigned
executables (including cargo build scripts), so host-native `cargo build` is prevented by OS
policy — not by FluxCore code. The crate was therefore verified inside a Linux container
(see [BUILD_AND_INSTALL.md](BUILD_AND_INSTALL.md)):

1. `cargo check --all-targets --target x86_64-pc-windows-gnu` — every `cfg(windows)`
   Windows API path (PDH counters, `GlobalMemoryStatusEx`, `GetNativeSystemInfo`,
   `K32GetProcessMemoryInfo`, registry reads, `CreateFileW`/`DeviceIoControl` storage queries,
   NVML dynamic loading) compiles against the real `windows` crate.
2. The full unit-test suite and the benchmark example run natively (Linux host target).
   The engine logic under test — analysis, pressure hysteresis, prediction, policies,
   safety, optimizer registry, audit trail — is platform-independent and identical on both
   targets; only the raw collectors use non-Windows fallbacks in this mode.

Executing the compiled Windows test binaries under wine in the same container was attempted
and abandoned after wineserver repeatedly crashed in the headless display chain. Runtime
execution of the Windows collector paths on a real Windows host therefore remains a known
verification gap, covered statically by step 1.
