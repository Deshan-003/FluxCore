# Build and install (Windows)

## Development run

1. Install Node.js 20+, Rust stable MSVC, Microsoft C++ Build Tools and WebView2 Runtime.
2. From this directory run `npm install`.
3. Start FluxCore with `npm run tauri dev`.

## Validate and release

```powershell
npm run lint
npm run build
npm test
npm run test:frontend
npm run tauri build
```

Tauri writes Windows installers in `src-tauri/target/release/bundle/` (MSI and NSIS targets
configured). The MSI/NSIS installer signing is not configured; sign binaries according to your
organization's release process before distributing.

FluxCore does not require elevation. NVIDIA telemetry is optional and requires compatible
drivers/NVML. See [HARDWARE_COMPATIBILITY.md](HARDWARE_COMPATIBILITY.md) and
[WINDOWS_COMPATIBILITY.md](WINDOWS_COMPATIBILITY.md) for unavailable provider metrics. The app
database is created under `%APPDATA%\FluxCore\fluxcore.sqlite3`; migrations 0002 (Phase 2
memory-intelligence tables) and 0003 (nullable Phase 1 snapshot percentages, row-preserving
rebuild) run idempotently on startup.

## Benchmark harness

Phase 2 ships a benchmark example measuring the Phase 1 collector baseline, memory-engine tick
cost (cold + steady state), combined pipeline overhead and FluxCore's own footprint — all from
real system data, without modifying system state:

```powershell
cargo run --release --manifest-path src-tauri/Cargo.toml --example benchmark [iterations]
```

It uses a throwaway SQLite database in the temp directory and removes it on exit.

Phase 4 adds a second harness for the intelligence engine: analysis-only cost, full sample with
SQLite persistence, the gated tick inside the 15-second window, database growth and process
footprint. It also uses live collector data and a throwaway database:

```powershell
cargo run --release --manifest-path src-tauri/Cargo.toml --example intelligence_benchmark [iterations]
```

Measured results (Linux logic path) are recorded in [PHASE4_TEST_REPORT.md](PHASE4_TEST_REPORT.md).

## Note for machines with Smart App Control

Windows Smart App Control (SAC) blocks locally built, unsigned executables — including cargo
build scripts and proc-macro DLLs — with "An Application Control policy has blocked this file
(os error 4551)". This prevents host-native `cargo build`/`cargo test` on such machines; it is
an OS policy condition, not a FluxCore code issue. Disabling SAC to build locally is
irreversible; that decision belongs to the machine owner.

The repository therefore ships a container verification path (`tools/verify/`) that was used
for Phase 2 sign-off. Build the image once (mingw-w64 toolchain and the
`x86_64-pc-windows-gnu` Rust target are pre-installed):

```powershell
docker build -t fluxcore-verify ..\tools\verify
```

**Step 1 — verify every Windows API path compiles** (all `cfg(windows)` glue typechecks
against the real `windows` crate):

```powershell
docker run --rm -v "${PWD}:/work" -v fluxcore-cargo:/usr/local/cargo/registry `
  -v fluxcore-target:/target -e CARGO_TARGET_DIR=/target fluxcore-verify `
  cargo check --manifest-path /work/src-tauri/Cargo.toml --all-targets `
  --target x86_64-pc-windows-gnu -j 1
```

**Step 2 — execute the full test suite and the benchmark example natively** (Linux host
target). The engine logic under test is platform-independent; only the raw collectors use
non-Windows fallbacks in this mode:

```powershell
docker run --rm -v "${PWD}:/work" -v "${PWD}\..\tools\verify:/scripts" `
  -v fluxcore-cargo:/usr/local/cargo/registry -v fluxcore-target:/target `
  -e CARGO_TARGET_DIR=/target fluxcore-verify sh /scripts/run-native.sh
```

The named volumes (`fluxcore-cargo`, `fluxcore-target`) persist the cargo registry and build
artifacts between runs. Give the WSL2 VM enough headroom before building — on an 8 GB host,
`%USERPROFILE%\.wslconfig` with `memory=5GB` and `swap=10GB` under `[wsl2]` prevented
out-of-memory kills during rustc codegen (keep `-j 1`/`-j 2` in that setup).

Executing the Windows test binaries under wine was attempted and abandoned (wineserver
crashes in the container's headless display chain); see the verification note in
[WINDOWS_COMPATIBILITY.md](WINDOWS_COMPATIBILITY.md) for the resulting coverage gap.
