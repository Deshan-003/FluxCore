# Phase 1 completion report

Scope: audit of the existing FluxCore implementation against every Phase 1 requirement,
implementation of all missing/incomplete Phase 1 features, bug fixes, integration, testing and
verification. Phase 2 code (memory intelligence engine) is preserved unchanged in behavior and
was **not** extended; no Phase 2 features were added in this pass.

## Ground rules honored

- Work stayed inside the existing repository; no new project, no architecture replacement.
- No mock/synthetic hardware data outside tests; no hardcoded metrics; unsupported readings
  serialize as `null` and render as "Unavailable".
- VRAM is never presented as system RAM; dedicated/shared/system pools are separate fields.
- No Windows Memory Manager replacement, no process termination, no admin privileges, no kernel
  driver, no working-set trimming, no standby purging, no pagefile modification.
- No working functionality was removed; UI identity/design unchanged (only correctness fixes,
  new detail rows and honest-unavailable rendering).

## Audit findings and what was implemented

### Backend (`src-tauri/src/`)

| Finding (before) | Fix (after) |
|---|---|
| NVML was initialized on **every** collection tick and never shut down | `NvmlSession` opens once per collector (`nvmlInit_v2`), enumerates devices once, releases the refcount via `nvmlShutdown` in `Drop` |
| GPU clocks/power/fan read once at init → stale values | Every supported NVML sensor is re-read on each tick |
| CPU frequency floored with `.max(1)` → fabricated "1 MHz" | Null-safe: PDH live counter → base clock → `None` |
| Percentages computed with fake-zero fallbacks (`0.0` when total = 0) | `percent()` helper returns `Option<f32>`; DTO fields are nullable end to end; SQLite migration **0003** relaxes `performance_snapshots` percent columns to NULL via a row-preserving table rebuild |
| Pagefile always `(None, None)` even on Windows | `GlobalMemoryStatusEx` → used/total pagefile |
| Storage read/write rates always `None` | `IOCTL_DISK_PERFORMANCE` cumulative counters differenced between ticks (`rate_bytes_per_second`), no elevated rights |
| Drive "kind" came from the filesystem API — not a real SSD/HDD classification | `IOCTL_STORAGE_QUERY_PROPERTY`: bus type 17 → NVMe SSD, seek-penalty property → SSD/HDD, removable flag; honest fallback string when the device does not answer |
| CPU architecture missing | `GetNativeSystemInfo` → `x86-64` / `ARM64` / … |
| GPU driver version and class missing | CIM `Win32_VideoController.DriverVersion`; conservative class inference (NVML/NVIDIA → Dedicated, Intel → Integrated, else Unknown) |
| NVIDIA adapters appeared twice (NVML + CIM) | CIM entries skipped when NVML already reported the adapter |
| No live push to the UI — frontend polled `get_snapshot` every second | Monitor loop emits `fluxcore-snapshot` Tauri event each tick (`set_app_handle` wired in `lib.rs` setup) |
| Only the OS base clock was available as "frequency" | PDH `\Processor Information(_Total)\Processor Frequency` provides live MHz (English counter names, `PDH_FMT_DOUBLE`, CStatus validated), base clock exposed as maximum |

Compile fixes found only by real verification runs: missing `tauri::Manager` import for the
setup hook, borrow-checker conflicts between the PDH counter (`&mut self`) and sysinfo borrows
(hoisted the live-frequency read; two-pass disk collection), `windows` 0.58 API mismatches
(`DeviceIoControl` in `Win32::System::IO`, `Win32_Security`/`Win32_System_IO` cargo features,
`ullTotalPageFile`/`ullAvailPageFile`, `IncursSeekPenalty`, `GlobalMemoryStatusEx` → `Result`,
PDH raw-`u32` status codes, NVML `Symbol` deref), and test-fixture type fixes.

### Frontend (`src/`)

| Finding (before) | Fix (after) |
|---|---|
| VRAM sparkline plotted the **system-RAM** percentage | `metricSeriesKey('VRAM') === 'vramUsedBytes'`; tile charts dedicated VRAM bytes |
| Chart window assumed a fixed 5 s cadence (`slice(-N)`) | Timestamp-based cutoff honoring `historyDurationMinutes` for both 1 s live points and 5 s SQLite rows |
| Disk throughput rendered a fake `0 B/s` | `hasIo` guard: real rates or "Unavailable" + explanatory note |
| Store polled every second; no event subscription | Event-driven store: subscribes to `fluxcore-snapshot`; appends in-memory points only when persistence is off; re-reads SQLite history on a 5 s cadence when persistence is on; interval polling kept solely as a fallback if `listen` fails |
| Settings saved optimistically with no rollback | Backend-validated save with full rollback (state + localStorage) and a rejection message that survives the follow-up refresh |
| Dead "GPU" sort option in Processes (no per-process GPU data exists) | Removed; sort by CPU (null readings last) or memory; backend `memoryPercent` shown instead of a frontend recomputation |
| `—%` rendering bug, paused state showed stale "active" labels | `pct(null) → '—'`; sidebar pill shows "Monitoring paused" |
| `useMemo(() => void refreshSeries(), …)` side effect (fires twice under StrictMode) | Converted to `useEffect` |
| "System" theme did nothing | `matchMedia('(prefers-color-scheme: light)')` effect with change listener |
