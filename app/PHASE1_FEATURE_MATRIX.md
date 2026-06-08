# Phase 1 feature matrix

Status of every Phase 1 requirement after the completion audit and implementation pass.
Evidence references are real files and executed tests — logs are kept under `tools/`
(`test-native4.log`, `check-docker10.log`, `frontend-tests.log`, `frontend-build2.log`,
`frontend-lint.log`).

| # | Feature | Status | Implementation evidence | Verification evidence |
|---|---------|--------|------------------------|----------------------|
| 1 | CPU monitoring (usage, per-core, speed, cores/threads, architecture) | Complete | `src-tauri/src/monitoring.rs` — sysinfo usage/per-core, PDH `\Processor Information(_Total)\Processor Frequency` for live MHz, OS base clock as maximum, `GetNativeSystemInfo` architecture; all nullable, never fabricated | Rust tests `memory_usage_is_physical_used_over_total`, `snapshot_serialization_uses_camel_case_contract`; windows-gnu cross-check exit 0 |
| 2 | RAM monitoring (total/used/available, cached, commit, pagefile) | Complete | `monitoring.rs` — sysinfo totals + `GlobalMemoryStatusEx` (pagefile used/total) + `K32GetPerformanceInfo` (cache/commit/commit limit); `MemoryPage` shows physical vs commit vs cache/standby separately | Rust tests `null_percentages_roundtrip_through_history`, `missing_counters_become_limitations_not_zeros`; vitest store tests |
| 3 | GPU monitoring (identity, utilization, clocks, temperature, power, fan) | Complete (NVIDIA telemetry via NVML; other vendors identification-only, documented) | `monitoring.rs` — `NvmlSession` initialized once per collector, all sensors refreshed each tick, `nvmlShutdown` on drop; CIM `Win32_VideoController` identity + driver version; NVML/CIM entries de-duplicated | Rust tests `gpu_class_inference_is_conservative`, `cim_entry_parser_maps_name_vendor_and_driver`; cross-check exit 0 |
| 4 | VRAM monitoring (dedicated used/total/available, VRAM %) | Complete | `monitoring.rs` — NVML `nvmlDeviceGetMemoryInfo` dedicated pool; `vram_usage_percent` via null-safe `percent()`; dedicated/shared/system-RAM kept as separate nullable fields | Rust test `history_aggregates_only_available_gpu_memory`; vitest `App GPU page` tests assert the VRAM tile charts `vramUsedBytes`, never the system-RAM series |
| 5 | Storage monitoring (volumes, capacity, drive type, read/write throughput) | Complete | `monitoring.rs` — sysinfo volumes; `IOCTL_STORAGE_QUERY_PROPERTY` bus type 17 → NVMe SSD, seek penalty → SSD/HDD; `IOCTL_DISK_PERFORMANCE` cumulative counters differenced between ticks into B/s | Rust tests `drive_classification_uses_documented_queries`, `rate_helper_derives_bytes_per_second`; cross-check exit 0 |
| 6 | Process monitoring (name, PID, CPU, memory, memory %) | Complete | `monitoring.rs` process pass with `percent()` memory share; UI search/sort/cap in `src/lib/processes.ts`; FluxCore never terminates processes (notice shown in UI) | vitest `processes.test.ts` (9 tests: search by name/PID, CPU-null-last sort, memory sort, 100-row cap) |
| 7 | Background monitoring engine (worker thread, settings, pause, clean stop) | Complete | `monitoring.rs` — `MonitorEngine` named worker thread, `RwLock` shared state, `validate_settings` (250 ms–60 s), stop signal + join in `Drop`, per-tick `fluxcore-snapshot` event emission | Rust tests `settings_reject_unreasonable_intervals`, `monitor_engine_starts_publishes_and_stops` |
| 8 | Frontend dashboard (overview, CPU, memory, GPU, storage, processes, hardware, settings pages) | Complete | `src/App.tsx` + `src/components/monitoring.tsx` + `src/lib/format.ts`; event-driven zustand store with polling fallback, timestamp-windowed charts, settings rollback, system-theme support | `npm run build` exit 0, `npm run lint` exit 0, vitest 40/40 incl. web-preview state and unavailable-sensor rendering |
| 9 | SQLite persistence (settings, snapshots, 5 s cadence, 7-day retention) | Complete | `src-tauri/src/database.rs` — WAL, idempotent migrations 0001–0003; migration 0003 makes snapshot percentages nullable via row-preserving table rebuild | Rust tests `settings_and_history_persist_in_sqlite`, `migration_three_preserves_legacy_rows` |
| 10 | Hardware compatibility handling (honest unavailable metrics, diagnostics) | Complete | `Option`/null end to end (percent/rate helpers return `None` on zero totals), limitation strings appended to snapshot `errors`, `check_hardware` diagnostic report; no hardcoded or synthetic values outside tests | Rust test `snapshot_serialization_uses_camel_case_contract` (null contract); vitest `renders unavailable sensors as Unavailable, never as zero` |
| 11 | Performance (low-overhead collection at 1 s cadence) | Complete — measured | Bounded in-memory series, single snapshot clone per UI read, event push instead of 1 s polling; benchmark example in `src-tauri/examples/benchmark.rs` | Benchmark executed in container (`test-native4.log`, debug profile): collector pass avg 4957 µs, memory-engine steady tick 110 µs, combined collect+tick 3747 µs (engine overhead 2.2 % of Phase 1 baseline), resident 5.2 MiB — Linux logic path; a warm re-run measured 2407 µs collector / 1.8 % overhead, i.e. per-tick cost stays far below the 1 s budget; see known gaps |
| 12 | Automated tests (backend + frontend + build) | Complete | 66 Rust unit tests (monitoring, database, memory engine, safety) + 40 vitest frontend tests + tsc/vite build + eslint + windows-gnu `cargo check --all-targets` | All executed: `test-native4.log` 66/66 passed, `frontend-tests.log` 40/40 passed, `frontend-build2.log`/`frontend-lint.log` exit 0, `check-docker10.log` exit 0 |

## Known verification gaps (honest disclosure)

- This workstation enforces Windows Smart App Control, which blocks locally built unsigned
  binaries (os error 4551). Rust verification therefore ran in a Linux container: full test
  execution against the platform-independent engine logic, plus a compile/type check of every
  `cfg(windows)` API path for `x86_64-pc-windows-gnu`.
- No Windows-host **runtime** execution of the NVML/PDH/IOCTL collectors was possible in this
  environment; their API usage is verified by compilation against the real `windows` crate and
  by unit tests of the pure derivation logic (rates, classification, percentages).
- Benchmark numbers come from the container's Linux logic path (Windows collectors stubbed);
  they bound the engine overhead but are not a substitute for on-target Windows measurements
  on the Lenovo IdeaPad Gaming 3 (Ryzen 5 5500H / RTX 2050 / 8 GB / NVMe).
