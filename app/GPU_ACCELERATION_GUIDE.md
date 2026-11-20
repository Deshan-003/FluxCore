# FluxCore — GPU Acceleration Guide (Phase 3)

How to use GPU workload acceleration in FluxCore, what to expect on a 4 GB RTX 2050-class machine,
and how to read the numbers honestly.

## 1. Open the page

Sidebar → **GPU INTELLIGENCE → GPU Acceleration**. Five tabs:

| Tab | What it shows |
| --- | --- |
| Overview | per-provider device/capability cards, VRAM budget panel, safety notes, environment (driver, CUDA path, wgpu backend), Re-probe button |
| Workload Intelligence | the four supported workloads with live suitability classification, reason, CPU/GPU/transfer estimates, VRAM need vs budget, size picker and Run |
| Acceleration Center | execution mode, resource limits, validation/history/event toggles, provider priority order, per-workload Manual pins |
| Task Monitor | active tasks (status, backend, progress, timings, speedup, VRAM, errors/warnings, cancel) + SQLite task history |
| Performance Comparison | run a CPU-vs-GPU benchmark (workload, size, warm-up/measured runs) and see measured medians, transfer overhead, speedup/slowdown verdicts and history |

## 2. Execution modes

- **CPU Only** — everything on CPU; GPU still monitored. `require_gpu` submissions are rejected.
- **GPU Acceleration** — supported workloads use the GPU when available and safe.
- **Intelligent Auto** (default) — per-workload decision from size, cost model and *measured* history.
- **Manual** — per-workload backend pins in Acceleration Center are honoured.

The decision never routes to the GPU because utilisation is low; utilisation is only a safety gate.
Small inputs usually stay on CPU because transfer + launch overhead dominates — that is correct
behaviour, not a failure.

## 3. Running a workload

1. Workload Intelligence → pick a size → **Run**.
2. The task appears in Task Monitor as `queued`, then `running`, then a terminal status.
3. If the GPU path fails and fallback is allowed, the task succeeds with a **→ CPU** badge and a
   warning explaining the real GPU error. It is never labelled as GPU execution.
4. Cancel is cooperative: a task inside a driver call stops at the next checkpoint.

Supported workloads: `vector_axpy` (a·x+y), `vector_scale` (a·x), `matmul` (naive m×k×n),
`row_sum` (row-wise reduction). Sizes and caps come from the backend registry; oversized or
VRAM-exceeding submissions are rejected with a reason.

## 4. Benchmarks

Performance Comparison → choose workload/size/warm-up/measured runs → **Run benchmark**.
Identical deterministic inputs on both backends; warm-ups discarded; medians reported with
min/mean/max/stddev; transfer time shown separately; speedup = CPU median / GPU end-to-end median.
**A slowdown is displayed as a slowdown.** If no GPU path ran, the GPU column is empty and the
verdict explains why. Results persist in SQLite and feed future Auto decisions (`estimateSource:
measured`).

## 5. What to expect on RTX 2050 4 GB / 8 GB RAM

- Default budget = min(50 % of 4 GiB, 1024 MiB cap) = **1 GiB**. 16 M-element vectors (768 MiB of
  buffers) and 1024³ matmul may be rejected as `InsufficientGpuResources` when other apps hold VRAM —
  expected, not a bug.
- Naive matmul and small vectors are often **slower** on GPU once PCIe transfer is included. The UI
  says so; Auto mode then keeps them on CPU.
- WDDM TDR (~2 s) is avoided by chunked launches and per-task timeouts; a hung device call surfaces
  as `TimedOut`, never as a silent success.

## 6. When acceleration is unavailable

The page shows an explicit unsupported state with the real reason (missing driver, PTX JIT failure,
no DX12/Vulkan adapter, …). CPU workloads, Phase 1 monitoring and Phase 2 memory intelligence keep
working unchanged. No sample or demo numbers are ever substituted.

## 7. Building / verifying from source

```powershell
# frontend (native Windows)
npm --prefix app install
npm --prefix app run build
npm --prefix app run test:frontend
npm --prefix app run lint

# Rust logic tests (WSL Ubuntu or any Linux host with the Tauri deps)
cargo test --manifest-path app/src-tauri/Cargo.toml

# real CPU-vs-GPU benchmark, no Tauri/DB needed
cargo run --release --manifest-path app/src-tauri/Cargo.toml --example gpu_benchmark 5
```

On machines without MSVC Build Tools, cross-compile the Windows binary from Linux/WSL with
`--target x86_64-pc-windows-gnu` (mingw-w64) and run it natively; see `GPU_COMPATIBILITY.md` §5.