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
| AMD / Intel | wgpu DX12/Vulkan adapter only | **UNTESTED** — no such adapter on this machine. No vendor claim is made beyond exercised hardware |

## 3. What is deliberately NOT supported

- Pre-sm_75 NVIDIA devices: PTX JIT fails → `GpuError::Unsupported` with the driver error string;
  CPU/wgpu remain. Reported, never pretended.
- CUDA Runtime API / NVRTC / `cust` / `cuda-sys`: require the toolkit at build time; excluded by design.
- Generic AMD/Intel GPU *telemetry* (Phase 1 scope) and shared-VRAM accounting: out of scope, UI says so.
- macOS/Linux native GPU compute: the app is Windows-first; Linux is a verification host only.

## 4. If a dependency is missing

| Missing | Behaviour | Install instruction (optional) |
| --- | --- | --- |
| NVIDIA driver / `nvcuda.dll` | `cuda` provider `available=false`, detail = driver error; app runs on CPU/wgpu | Install the NVIDIA Game Ready/Studio driver for the GPU |
| CUDA toolkit | **not required**; `cudaToolkitPresent=false` is reported, compute still works via driver API + PTX | Only needed for third-party CUDA development: NVIDIA CUDA Toolkit ≥ 12.x |
| DX12/Vulkan adapter | `wgpu` provider unavailable with enumerated reason; CUDA/CPU remain | Update GPU driver |
| MSVC Build Tools / WebView2 | affects *building* the desktop app, not GPU compute | Visual Studio Build Tools "Desktop development with C++"; WebView2 Runtime (evergreen) |

## 5. Verification-host notes (why tests run in WSL)

- Native MSVC linking is unavailable on this machine (no `link.exe`/`cl.exe`; the PATH `link.exe`
  is Git's hardlink utility) → `cargo check` for the MSVC host fails at build-script link.
- The rustup-downloaded GNU **host** toolchain is blocked by Smart App Control (os error 4551).
- Locally compiled and mingw-cross-compiled Windows binaries execute normally.
- Therefore: logic tests run in WSL Ubuntu (Linux host), and Windows GNU binaries are cross-built
  with `x86_64-w64-mingw32-gcc` and executed natively on Windows for real-driver verification.