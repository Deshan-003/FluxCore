# Phase 3 — Frozen Interface Contract (main agent sign-off)

Status: **FROZEN**. Supersedes conflicting details in `PHASE3_ARCHITECTURE.md` §3/§8/§9 only where
explicitly noted below. `PHASE3_ARCHITECTURE.md` remains authoritative for *behaviour*, decision
order, workload catalogue, safety rules, UI plan and the database schema. This document is
authoritative for **exact Rust/TS signatures and file ownership**.

All subagents MUST read `PHASE3_ARCHITECTURE.md` (behaviour) and this file (signatures).
No subagent may change the global architecture or edit a file it does not own. Deviations must be
reported to the main agent, not silently applied.

## 0. Verified environment (measured 2026-11-13, this machine)

| Item | Measured value |
| --- | --- |
| OS | Windows (win32), shell Git Bash |
| CPU / GPU | NVIDIA GeForce RTX 2050, 4096 MiB dedicated VRAM, WDDM |
| Driver | 591.59 (`nvidia-smi`), CUDA driver API 13.1 |
| `nvcuda.dll`, `nvml.dll` | both present in `C:\Windows\System32` |
| `nvcc`, `CUDA_PATH` | **absent** — no CUDA toolkit |
| Rust | cargo/rustc 1.99.1 at `C:\Users\hdgan\.cargo\bin\cargo.exe` (NOT on PATH) |
| Node | v22.22.2, npm 10.9.7 |
| wgpu | 30.0.2 already in the local cargo registry cache |
| Git branch | `feature/phase3-gpu-acceleration` |

Consequences: CUDA compute uses the **driver API** (`libloading` + `nvcuda.dll`) with **embedded PTX**
(no `nvcc`, no build script, no `cust`/`cuda-sys`). A vendor-neutral **wgpu 30 WGSL** path covers
DX12/Vulkan adapters (AMD/Intel/NVIDIA) without a toolkit. `pollster = "1"` is used for `block_on`
rather than a hand-rolled no-op waker (reliability overrides §8.3's "custom first" note).

No vendor support claim may be made for hardware that was not actually exercised. AMD/Intel coverage
is via wgpu adapter discovery only; if no such adapter exists here, report it as **untested**.

## 0bis. Main-agent arbitration of reported contract conflicts (2026-11-13)

Subagent 1 reported five conflicts. Rulings, binding on every other subagent:

- **C3 (binding): `AccelerationConfig::vram_budget_percent` is a PERCENT in `1.0..=90.0`,
  default `50.0` — NOT a 0..1 fraction.** `budget_bytes = min(dedicated_total *
  vram_budget_percent / 100.0, cap_mb*MiB when cap>0)`. `models.rs` has been corrected by the main
  agent. Subagent 4 (`resources.rs`) and Subagent 5 (slider) must use percent semantics. §5.4's
  prose "fraction … Default 0.5" is overridden.
- C1: freeze §3 wins over architecture §3.1/§5.3 for `run_cpu(&inputs, ..)`, `make_inputs`,
  `ExecutionOutcome.timing`, `ExecutionRequest.validate`. Confirmed.
- C2: `WorkloadParams` uses one `#[serde(tag="kind", rename_all="camelCase")]`; `PartialEq` +
  `Copy` (not `Eq`, because of `f32`). Confirmed.
- C4: `(4096,1024)` row_sum is reachable by submitting an explicit `WorkloadParams::RowSum`, and by
  the UI's secondary-size control; `default_params` maps a single `size` to `(size,size)`. Confirmed.
- C5: unstated §5.4 defaults chosen by Subagent 1 (`min_system_ram_available_percent = 15`,
  `retention_days = 30`, etc.) are approved.

## 1. Frozen answers to §15 open questions

1. `wgpu = "30"` approved; **`pollster = "1"` approved and will be added by the main agent**.
2. Workload size catalogue in §7.1 approved. Benchmark example defaults to cheaper sizes so runs finish
   quickly; the full catalogue stays available in the UI.
3. `execution.rs` is owned **entirely by Subagent 3** (no split with Subagent 2). Subagent 2 owns only
   `providers/*` and `capability.rs`; its only obligation to execution is the `GpuProvider::execute`
   contract below. This removes the shared-file race.
4. Nav group `'GPU INTELLIGENCE'`, page label `'GPU Acceleration'` approved.
5. `gpu_task_history` written **on task completion only**, never per monitor tick, approved.

## 2. File ownership (STRICT — one writer per file)

| Owner | Files it may create/modify |
| --- | --- |
| **Main agent** | `src-tauri/Cargo.toml`, `src-tauri/src/lib.rs`, `src-tauri/src/database.rs`, `src-tauri/src/gpu_engine/mod.rs`, `src/App.tsx`, `PHASE3_CONTRACT_FREEZE.md`, `PHASE3_COMPLETION_REPORT.md`, `README.md`, `PHASE3_BENCHMARK_REPORT.md` (final edit) |
| **Subagent 1** | `src-tauri/src/gpu_engine/models.rs`, `PHASE3_ARCHITECTURE.md` (append-only: diagrams / dependency analysis / integration requirements) |
| **Subagent 2** | `src-tauri/src/gpu_engine/capability.rs`, `src-tauri/src/gpu_engine/providers/{mod,cpu,cuda,wgpu}.rs`, `src-tauri/src/gpu_engine/providers/wgsl/*.wgsl`, `src-tauri/ptx/*.ptx` |
| **Subagent 3** | `src-tauri/src/gpu_engine/{profiler,scheduler,execution}.rs` |
| **Subagent 4** | `src-tauri/src/gpu_engine/{resources,persistence}.rs` |
| **Subagent 5** | `src/types/gpu.ts`, `src/services/gpu.ts`, `src/stores/gpu.ts`, `src/pages/GpuAcceleration.tsx`, `src/components/gpu/*.tsx`, `src/components/gpu/*.test.tsx`, `src/App.css` (append `gpu-*` section only) |
| **Subagent 6** | `src-tauri/src/gpu_engine/benchmark.rs`, `src-tauri/tests/gpu_*.rs`, `src-tauri/examples/gpu_benchmark.rs`, `PHASE3_BENCHMARK_REPORT.md` (draft), `PHASE3_TEST_RESULTS.md` |
| **Subagent 7** | `PHASE3_IMPLEMENTATION.md`, `GPU_COMPATIBILITY.md`, `GPU_ACCELERATION_GUIDE.md`, `GPU_SAFETY.md`, `PHASE3_TEST_REPORT.md`, `PHASE3_REVIEW.md` |

**Nobody edits `gpu_engine/mod.rs`, `lib.rs`, `Cargo.toml`, `database.rs` or `App.tsx` except the main
agent.** The main agent has already written `mod.rs`/`lib.rs` against the signatures below, so those
files double as an executable contract.

## 3. `models.rs` (Subagent 1) — exact contents

Implement **every** type from `PHASE3_ARCHITECTURE.md` §5 (all enums/structs, exact field names and
`#[serde(rename_all = "camelCase")]`), plus §6 `SubmitTaskRequest`, plus the following additions which
resolve contract gaps. `models.rs` is a **leaf module**: it may depend only on `serde`, `chrono`,
`std`. No dependency on any other `gpu_engine` module.

```rust
// --- provider-side shared data (from architecture §3.1, MOVED here so providers
