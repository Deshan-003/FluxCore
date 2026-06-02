<p align="center">
  <img src="app/src-tauri/icons/128x128.png" alt="FluxCore logo" width="96" height="96" />
</p>

<h1 align="center">FluxCore</h1>

<p align="center">
  Windows-first desktop hardware monitoring and memory intelligence.<br />
  Real operating-system measurements only — unsupported sensors are reported as unavailable, never invented.
</p>

<p align="center">
  <img alt="Platform: Windows 10/11 x64" src="https://img.shields.io/badge/platform-Windows%2010%2F11%20x64-0078D6" />
  <img alt="Tauri 2" src="https://img.shields.io/badge/Tauri-2-FFC131" />
  <img alt="React 19" src="https://img.shields.io/badge/React-19-61DAFB" />
  <img alt="Rust" src="https://img.shields.io/badge/Rust-stable-DEA584" />
  <img alt="TypeScript 5.9" src="https://img.shields.io/badge/TypeScript-5.9-3178C6" />
</p>

---

FluxCore is a native Windows desktop application built with **React 19, TypeScript, Vite, Tauri 2 and Rust**.
It collects live CPU, memory, GPU, storage and process telemetry from documented Windows APIs, and adds an
intelligent memory engine that detects pressure, ranks applications, forecasts availability and recommends
action — without ever performing destructive "RAM cleaning".

Three rules govern every number in the UI:

- **Never invent telemetry.** A missing provider or counter serializes as `null` and renders as *Unavailable*.
- **Never conflate memory kinds.** GPU VRAM, shared GPU memory, committed memory and physical RAM are separate fields.
- **Never destroy user state.** No process termination, working-set trimming, standby purging or pagefile changes — rejected by the safety engine itself, not merely hidden in the UI.

## Features

### Phase 1 — Hardware monitoring foundation

- **CPU** — name, vendor, architecture, logical processors, physical cores (when exposed), total and per-core utilization, live frequency via the PDH processor-frequency counter with the OS base clock as fallback and reported maximum.
- **Memory** — total, used (`total - available`), available, free, cached, pagefile used/total and OS commit charge as distinct values.
- **Processes** — PID, process CPU, resident memory and memory share.
- **Storage** — mounted filesystem capacity/free/used, drive-type classification (NVMe SSD / SSD / HDD / removable) and per-volume read/write throughput from `IOCTL_DISK_PERFORMANCE` deltas.
- **GPU** — identity and driver version via Windows CIM with conservative class inference; NVIDIA utilization, dedicated VRAM, clocks, temperature, power and fan telemetry through NVML when installed and supported.
- **Dashboard** — dark theme, dedicated CPU / memory / GPU / storage / process / hardware / settings pages, event-driven live updates, timestamp-windowed history charts, health thresholds and configurable refresh.

### Phase 2 — Intelligent memory management

- **Memory Intelligence dashboard** — physical / available / committed / compressed / pagefile / standby tiles, all measured with documented Windows APIs.
- **Pressure detection** — four states (normal / elevated / high / critical) from multiple concurrent signals, with hysteresis and minimum durations so a single sample never causes flapping. Sustained periods are recorded as events.
- **Application memory ranking** — per-process private bytes, working set, private working set, peak, share and growth trend with search/filter/sort. "Suspected growth pattern" labels require ≥ 8 samples over ≥ 4 minutes with a strong regression fit.
- **Prediction** — OLS short-term forecast of available memory and commit charge, with an explicit confidence score, horizon and disclaimer.
- **History** — RAM usage/availability, committed, compressed and pagefile series persisted in SQLite.
- **Recommendation engine** — advice generated only from live measurements; benefits are qualitative, never invented byte savings.
- **Optimization Center** — three modes (Analysis Only / Smart Assist / Manual), a full policy editor, a seven-action registry limited to FluxCore's own resources or read-only guidance, two-step confirmation for sensitive actions, and a complete audit trail.

## Safety guarantees

The following operations are **rejected by the backend safety validator regardless of mode or user confirmation**:

| Forbidden operation | Reason |
| --- | --- |
| Terminating applications | Destroys unsaved user work |
| Trimming process working sets | Forces page faults, degrades performance |
| Purging standby / modified lists | Discards useful cache, harms responsiveness |
| Resizing or disabling the pagefile | Risks commit-limit failures and crashes |
| Disabling memory compression | Removes an OS memory-saving mechanism |

Every executed, rejected or failed operation is written to an audit trail. See [OPTIMIZATION_SAFETY.md](app/OPTIMIZATION_SAFETY.md).

## Architecture

```
┌──────────────────────────── React 19 + TypeScript (src/) ────────────────────────────┐
│  Pages & components ── Zustand stores ── typed IPC services (services/*.ts)          │
│        ▲                    ▲                                                        │
│        │  fluxcore-snapshot │ get_* / set_* commands                                 │
└────────┼────────────────────┼────────────────────────────────────────────────────────┘
         │                    │  Tauri 2 IPC
┌────────┴────────────────────┴────────────────────────────────────────────────────────┐
│                            Rust backend (src-tauri/)                                 │
│  monitoring.rs ── named worker thread, RwLock snapshot, one collection per tick      │
│        │                                                                             │
│        ├─→ database.rs ── SQLite, idempotent migrations, retention                   │
│        ├─→ memory_engine/ ── analyzer, pressure, prediction, policies, safety        │
│        └─→ app.emit("fluxcore-snapshot")                                             │
└──────────────────────────────────────────────────────────────────────────────────────┘
```

The memory engine is ticked from the existing monitor loop — it never spawns its own polling threads and
never triggers extra hardware queries. The frontend store subscribes to the backend event and falls back to
interval polling only if subscription fails. Full detail: [ARCHITECTURE.md](app/ARCHITECTURE.md).

## Tech stack

| Layer | Technology |
| --- | --- |
| Desktop shell | Tauri 2 (Rust) |
| UI | React 19, TypeScript 5.9, Vite 7 |
| Styling | Tailwind CSS 3, Radix UI primitives, lucide-react |
| State | Zustand 5 |
| Charts | Recharts 2 |
| System telemetry | `sysinfo`, PDH, `GlobalMemoryStatusEx`, `K32GetPerformanceInfo`, storage IOCTLs, Windows CIM, NVML |
| Persistence | SQLite (local, idempotent migrations) |
| Tests | `cargo test`, Vitest + Testing Library |

## Repository layout

```
.
├── app/                  # The FluxCore application
│   ├── src/              # React/TypeScript frontend (pages, components, stores, services)
│   ├── src-tauri/        # Rust backend (monitoring, memory_engine, database, migrations)
│   └── *.md              # Deep technical documentation
├── reference/            # Original design reference and Phase 1 specification
└── README.md             # This file
```

## Getting started

### Prerequisites

- Windows 10/11 x64
- Node.js 20+ and npm
- Rust stable MSVC toolchain (`x86_64-pc-windows-msvc`)
- Microsoft C++ Build Tools and the WebView2 Runtime (Tauri prerequisites)

### Run in development

```powershell
cd app
npm install
npm run tauri dev
```

The web-only preview (`npm run dev`) is intentionally an empty live-data state: it detects the absence of
Tauri and shows an unsupported state rather than substituting sample measurements.

### Build installers

```powershell
cd app
npm run tauri build
```

Tauri writes MSI and NSIS installers to `app/src-tauri/target/release/bundle/`. Installer signing is not
configured; sign binaries according to your release process before distributing. FluxCore does not require
elevation.

## Verification

```powershell
cd app
npm run lint          # ESLint
npm run build         # tsc -b && vite build
npm test              # cargo test (Rust)
npm run test:frontend # vitest run
```

Benchmark harness — real measurements, never modifies system state, uses a throwaway SQLite database:

```powershell
cargo run --release --manifest-path app/src-tauri/Cargo.toml --example benchmark [iterations]
```

## Data and privacy

No data leaves the device. The local database lives at `%APPDATA%\FluxCore\fluxcore.sqlite3` and stores
settings, hardware identification, sampled aggregate resource values, memory snapshots, pressure events,
top-N process memory points, recommendations, predictions and the optimization audit trail.

Retention: Phase 1 history 7 days (5-second sampling); memory intelligence 30 days with a per-table row
floor; action audit 90 days. Nullable percentages persist as SQL `NULL`, never as fabricated zeros.

## Documentation

| Document | Contents |
| --- | --- |
| [app/README.md](app/README.md) | Feature-level overview and quick reference |
| [ARCHITECTURE.md](app/ARCHITECTURE.md) | System boundaries, data flow, IPC surface |
| [MEMORY_ENGINE.md](app/MEMORY_ENGINE.md) | Engine architecture, pressure model, leak detection, prediction, persistence |
| [OPTIMIZATION_SAFETY.md](app/OPTIMIZATION_SAFETY.md) | Forbidden operations, action registry, validation and audit flow |
| [WINDOWS_COMPATIBILITY.md](app/WINDOWS_COMPATIBILITY.md) | Every Windows API used, fallbacks and deliberate limitations |
| [HARDWARE_COMPATIBILITY.md](app/HARDWARE_COMPATIBILITY.md) | Supported hardware and unavailable provider metrics |
| [BUILD_AND_INSTALL.md](app/BUILD_AND_INSTALL.md) | Build, test and packaging |
| [DEVELOPMENT.md](app/DEVELOPMENT.md) | Development workflow |
| [PHASE1_COMPLETION_REPORT.md](app/PHASE1_COMPLETION_REPORT.md) · [FEATURE_MATRIX](app/PHASE1_FEATURE_MATRIX.md) · [TEST_REPORT](app/PHASE1_TEST_REPORT.md) | Phase 1 audit, implementation and executed-verification record |

## Current limitations

Tray/startup integration, generic AMD/Intel GPU telemetry, shared GPU memory accounting, disk
temperature/SMART health, GPU process attribution and per-process virtual size are **not** in scope, and the
UI does not claim they are active. NVIDIA telemetry requires compatible drivers and NVML.