# Development

## Setup

Use a Windows 10/11 x64 machine with Node.js 20+, npm, Rust stable for `x86_64-pc-windows-msvc`, Visual Studio C++ Build Tools, and WebView2. Install frontend dependencies with `npm install`.

## Commands

- `npm run tauri dev` — run the desktop application and Vite dev server.
- `npm run dev` — browser preview; live metrics are intentionally unavailable outside Tauri.
- `npm run lint` — ESLint.
- `npm run build` — TypeScript project build and Vite production bundle.
- `npm test` — Rust unit tests.
- `npm run test:frontend` — vitest suite (jsdom + @testing-library/react). Tests are colocated
  as `src/**/*.test.ts[x]`; the Tauri IPC boundary (`@tauri-apps/api`) and
  `services/monitoring` are mocked, so the suite runs without a Windows backend.
- `cargo fmt --manifest-path src-tauri/Cargo.toml -- --check` — Rust formatting check.
- `cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings` — Rust static analysis.
- `npm run tauri build` — Windows installer/release bundle.

## Frontend testability conventions

Keep pure logic out of components so it can be unit-tested without rendering: display
formatting lives in `src/lib/format.ts`, process search/sort rules in `src/lib/processes.ts`,
and shared presentational components in `src/components/monitoring.tsx`. Unavailable sensors
must render as explicit placeholders (`Unavailable`, `—`) — tests assert that a null reading
never becomes `0`.

## Adding providers

Keep providers isolated: hardware identity may be cached, but dynamic readings should be refreshed at the poll interval. Represent unsupported or failed sensor data with `Option`/nullable fields. Do not turn capacity, shared memory, or committed memory into unrelated metrics. Provider-specific failures should be appended as diagnostics and must not terminate other collection.

## Database

Schema version 1 is represented in `src-tauri/migrations/0001_initial.sql` and mirrored in the startup migration in `src-tauri/src/database.rs`. Phase 2 adds migration 0002 (memory-intelligence tables). Migration 0003 rebuilds `performance_snapshots` in place so cpu/memory percent columns are nullable, preserving existing rows. Apply future migrations transactionally and retain bounded history.

## Benchmarking

No fixed resource budget is claimed without measurement. For a release build, use Windows Task Manager/Performance Monitor to record idle CPU and working set during a 5-minute run at 1-second UI refresh, with and without NVML; compare against 5-second and 10-second intervals. The collector currently keeps a bounded in-memory frontend series and writes aggregate history at a five-second cadence.
