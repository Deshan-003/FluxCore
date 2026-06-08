# FluxCore

FluxCore is a Windows-first desktop hardware monitoring and memory intelligence application
built with React, TypeScript, Vite, Tauri 2 and Rust. It collects real operating-system
measurements and explicitly reports unsupported sensors as unavailable; it never invents
telemetry, never treats GPU memory as system RAM, and never performs destructive "RAM cleaning".

## Phase 1 — hardware monitoring foundation

- CPU name/vendor/architecture, logical processors, physical core count (when exposed), total/per-core utilization, live frequency (PDH processor-frequency counter with the OS base clock as fallback and as the reported maximum).
- Physical memory total, used (`total - available`), available, free, cached, pagefile used/total and OS committed counters as distinct values.
- Process list with PID, process CPU, resident memory and memory share.
- Mounted filesystem capacity/free/used data, drive-type classification (NVMe SSD / SSD / HDD / removable via documented storage device queries) and per-volume read/write throughput (`IOCTL_DISK_PERFORMANCE` deltas).
- GPU identification and driver version through Windows CIM with conservative class inference; NVIDIA utilization, dedicated VRAM, clocks, temperature, power and fan telemetry through NVML (initialized once per session, every sensor refreshed each tick) when installed and supported.
- Dark dashboard, CPU/memory/GPU/storage/process/hardware/settings pages, event-driven live updates (`fluxcore-snapshot`), timestamp-windowed history charts, health thresholds and configurable refresh.
- Local SQLite preferences and a five-second history sampler with seven-day retention; nullable percentages persist as NULL instead of fabricated zeros.

## Phase 2 — intelligent memory management

- **Memory Intelligence dashboard**: physical/available/committed/compressed/pagefile/standby
  overview tiles, all measured with documented Windows APIs (PDH counters,
  `GlobalMemoryStatusEx`, `K32GetPerformanceInfo`).
- **Pressure detection**: four states (normal/elevated/high/critical) from multiple concurrent
  signals with hysteresis and minimum durations — no flapping on single samples. Sustained
  pressure periods are recorded as events.
- **Application memory ranking**: per-process private bytes, working set, private working set,
  peak, share and growth trend, with search/filter/sort. "Suspected growth pattern" labels
  require ≥ 8 samples over ≥ 4 minutes with strong regression fit — never a single snapshot.
- **Prediction**: OLS-based short-term forecast of available memory and commit charge with an
  explicit confidence score, horizon and disclaimer.
- **History charts**: RAM usage/availability, committed memory, compressed memory and pagefile
  usage over time, persisted in SQLite (migration 0002, seven new tables, Phase 1 data intact).
- **Recommendation engine**: generates advice only from live measurements; benefits are
  qualitative — invented byte savings appear nowhere.
- **Optimization Center**: three modes (Analysis Only / Smart Assist / Manual), a full policy
  configuration editor, a seven-action registry that only touches FluxCore's own resources or
  provides read-only guidance, two-step confirmation for every sensitive action, and a complete
  audit trail of executed/rejected/failed operations.
- **Safety guarantees**: FluxCore never terminates applications, trims working sets, purges
  standby lists, resizes/disables the pagefile or disables compression — these operations are
  rejected by the safety engine itself, not merely hidden in the UI.

## Prerequisites

- Windows 10/11 x64
- Node.js 20 or newer and npm
- Rust stable MSVC toolchain (`x86_64-pc-windows-msvc`)
- Microsoft C++ Build Tools and WebView2 Runtime (Tauri prerequisites)

## Run

```powershell
npm install
npm run tauri dev
```

Web-only preview (`npm run dev`) is intentionally an empty live-data state: it does not
substitute sample measurements.

## Checks

```powershell
npm run lint
npm run build
npm test
npm run test:frontend
npm run tauri build
```

Benchmark harness (real measurements; never modifies system state):

```powershell
cargo run --release --manifest-path src-tauri/Cargo.toml --example benchmark
```

## Documentation

- [MEMORY_ENGINE.md](MEMORY_ENGINE.md) — engine architecture, pressure model, leak detection, prediction, persistence
- [OPTIMIZATION_SAFETY.md](OPTIMIZATION_SAFETY.md) — forbidden operations, action registry, validation and audit flow
- [WINDOWS_COMPATIBILITY.md](WINDOWS_COMPATIBILITY.md) — every Windows API used, fallbacks and deliberate limitations
- [ARCHITECTURE.md](ARCHITECTURE.md) — system boundaries and data flow
- [BUILD_AND_INSTALL.md](BUILD_AND_INSTALL.md) — build, test and packaging
- [DEVELOPMENT.md](DEVELOPMENT.md), [HARDWARE_COMPATIBILITY.md](HARDWARE_COMPATIBILITY.md)
- [PHASE1_COMPLETION_REPORT.md](PHASE1_COMPLETION_REPORT.md), [PHASE1_FEATURE_MATRIX.md](PHASE1_FEATURE_MATRIX.md), [PHASE1_TEST_REPORT.md](PHASE1_TEST_REPORT.md) — Phase 1 audit, implementation and executed-verification record
