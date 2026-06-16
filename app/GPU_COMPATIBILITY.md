# FluxCore — GPU Compatibility (Phase 3)

Measured on the primary test machine, 2026-11-17. Nothing here is assumed; untested paths are
labelled untested.

## 1. Primary test machine (measured)

| Item | Value | Source |
| --- | --- | --- |
| Laptop | Lenovo IdeaPad Gaming 3 15ACH6 class (RTX 2050 / Ryzen 5 5500H / 8 GB) | user-supplied target; GPU/RAM confirmed by `nvidia-smi` and WSL/Windows probes |
| GPU | NVIDIA GeForce RTX 2050, 4096 MiB dedicated VRAM, WDDM | `nvidia-smi` |
| Driver | 591.59 | `nvidia-smi` |
| CUDA driver API | 13.1 (`nvcuda.dll` present in `C:\Windows\System32`) | `nvidia-smi` header + file probe |
| Compute capability | 8.6 (Ampere). Embedded PTX targets `sm_75`, ISA 7.0, `.address_size 64`; the driver JITs it forward to 8.6 | `ptx/*.ptx` headers; CUDA forward-compatibility rule |
| CUDA toolkit / `nvcc` / `CUDA_PATH` | **absent** | `where nvcc`, env probe |
| CUDA runtime API (`cudart`) | absent (ships with the toolkit). FluxCore uses the **driver API only** | `capability.rs::environment` |
| Vulkan / DX12 | present via WDDM driver; wgpu adapter discovery is the only claim made | `providers/wgpu.rs` enumerate_adapters |
| System RAM | 8 GB | OS probe |

## 2. Supported compute paths

| Path | Requirement | Status on this machine |
| --- | --- | --- |
| CPU reference (`cpu`) | none | always available; reference for validation and fallback |
| NVIDIA CUDA (`cuda`) | `nvcuda.dll` + driver ≥ PTX ISA 7.0 (sm_75); **no toolkit needed** | available; kernels JIT-loaded at initialize; see `PHASE3_BENCHMARK_REPORT.md` for measured runs |
| wgpu WGSL (`wgpu`) | DX12 or Vulkan adapter | discovered on Windows; **unavailable inside WSL/Linux** (no adapter) and reported honestly as such |
