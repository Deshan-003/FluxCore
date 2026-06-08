# Phase 1 test report

All results below come from commands that were actually executed on 2026-11-12. Raw logs are
kept in `tools/`. Nothing is reported as passed that was not run.

## Verification environment

This workstation enforces Windows **Smart App Control**, which blocks locally built unsigned
binaries (os error 4551). Host-native `cargo build`/`cargo test` is therefore impossible here.
Verification used two channels:

1. **Docker container** `fluxcore-verify` (rust:1-bookworm + mingw-w64 + `x86_64-pc-windows-gnu`
   target), with named volumes `fluxcore-cargo` (registry, mounted at `/usr/local/cargo/registry`)
   and `fluxcore-target` (`CARGO_TARGET_DIR=/target`), app mounted at `/work`:
   - full **execution** of the Rust test suite and benchmark against the Linux (platform-
     independent engine logic) code path;
   - **compile/type verification** of every `cfg(windows)` API path via
     `cargo check --all-targets --target x86_64-pc-windows-gnu`.
2. **Host npm** (JavaScript only, allowed under SAC): vitest suite, `tsc -b && vite build`,
   ESLint.

Running the Windows test binaries under wine was attempted earlier and abandoned (wineserver
crashes in the container's headless display chain); this remains a documented coverage gap.

## 1. Rust unit tests — executed, 66/66 passed

Command (container): `sh /tools/verify/run-native.sh` →
`cargo test --manifest-path /work/src-tauri/Cargo.toml -j 2`

Log: `tools/test-native4.log`

```
running 66 tests
test result: ok. 66 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.83s
```

| Module | Tests | Phase 1 additions in this pass |
|---|---|---|
| `memory_engine::*` (Phase 2, preserved) | 54 | — (fixtures updated for nullable DTO fields) |
| `monitoring` | 9 | `rate_helper_derives_bytes_per_second`, `drive_classification_uses_documented_queries`, `gpu_class_inference_is_conservative`, `cim_entry_parser_maps_name_vendor_and_driver`, `snapshot_serialization_uses_camel_case_contract` (camelCase + null contract), `history_aggregates_only_available_gpu_memory`, `settings_reject_unreasonable_intervals`, `monitor_engine_starts_publishes_and_stops` (worker publishes a real snapshot, then stops cleanly), `memory_usage_is_physical_used_over_total` |
| `database` | 3 | `null_percentages_roundtrip_through_history` (NULL stays NULL), `migration_three_preserves_legacy_rows` (legacy NOT NULL rows survive the rebuild; idempotent reopen), `settings_and_history_persist_in_sqlite` |

Synthetic data appears **only** inside these tests; production collectors read real OS APIs.

## 2. Windows API compile verification — executed, exit 0

Command (container):
`cargo check --manifest-path /work/src-tauri/Cargo.toml --all-targets --target x86_64-pc-windows-gnu -j 2`

Log: `tools/check-docker10.log`

```
Compiling fluxcore v0.1.0 (/work/src-tauri)
Finished `dev` profile [unoptimized + debuginfo] target(s) in 4.78s
CROSSCHECK_EXIT=0
```

This exercises every `cfg(windows)` path against the real `windows` 0.58 crate: PDH frequency
counter, `GlobalMemoryStatusEx` pagefile, `GetNativeSystemInfo`, `CreateFileW` +
`IOCTL_DISK_PERFORMANCE`, `IOCTL_STORAGE_QUERY_PROPERTY` (bus type + seek penalty), NVML
session, CIM GPU queries, Tauri event emission.

**Honest correction:** an earlier cross-check claim for this branch (`tools/check-docker7.log`,
reported as "exit 0") was a shell exit-code capture artifact; the log actually contained 19
compile errors introduced by the Phase 1 backend refactor (missing cargo features
`Win32_Security`/`Win32_System_IO`, `DeviceIoControl` module path, `windows` 0.58 field/result
renames, PDH status-code types, `w!` macro path, NVML symbol derefs, `tauri::Manager` import,
two borrow-checker conflicts). All were fixed and the check now genuinely passes; exit codes
are captured with a reliable `&& echo … || echo …` pattern.

## 3. Frontend tests — executed, 40/40 passed

Command (host): `npm run test:frontend` (vitest 5, jsdom, @testing-library/react 16)

Log: `tools/frontend-tests.log`

```
✓ src/lib/processes.test.ts        (9 tests)
✓ src/lib/format.test.ts           (12 tests)
✓ src/stores/monitoring.test.ts    (7 tests)
✓ src/components/monitoring.test.tsx (9 tests)
✓ src/App.test.tsx                 (3 tests)
Test Files  5 passed (5)
     Tests  40 passed (40)
```

Coverage highlights:

