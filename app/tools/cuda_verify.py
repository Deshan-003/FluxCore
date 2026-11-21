#!/usr/bin/env python3
"""Verify the shipped PTX kernels against the live NVIDIA driver.

This exists because the developer machine's code-integrity policy blocks running
locally built Rust executables. A signed Python interpreter can still call the
signed nvcuda.dll, so this harness JIT-compiles the exact PTX files shipped in
app/src-tauri/ptx and runs them with the same inputs and launch geometry as
cuda.rs. No CUDA toolkit or nvcc is required.

Run:  python -I tools/cuda_verify.py --out <report.md>
"""

from __future__ import annotations

import argparse
import ctypes
import json
import pathlib
import struct
import sys
import time

import numpy as np

MASK64 = (1 << 64) - 1
FNV_OFFSET = 0xCBF29CE484222325
FNV_PRIME = 0x100000001B3
VECTOR_CHUNK = 1 << 20
TOLERANCE = 1e-3

U64 = ctypes.c_uint64
VOIDP = ctypes.c_void_p


def fnv_mix(h: int, word: int) -> int:
    return ((h ^ (word & MASK64)) * FNV_PRIME) & MASK64


def f32_bits(value: float) -> int:
    return struct.unpack("<I", struct.pack("<f", value))[0]


class XorShift64:
    """Bit-for-bit port of gpu_engine::models::XorShift64."""

    def __init__(self, seed: int) -> None:
        self.state = seed & MASK64 or 1

    def next_u64(self) -> int:
        x = self.state
        x ^= (x << 13) & MASK64
        x ^= x >> 7
        x ^= (x << 17) & MASK64
        self.state = x & MASK64
        return self.state

    def next_f32(self) -> np.float32:
        top = self.next_u64() >> 40
        unit = top / 16_777_216.0
        return np.float32(unit * 2.0 - 1.0)


def seed_for(workload: dict) -> int:
    h = FNV_OFFSET
    kind = workload["kind"]
    if kind in ("axpy", "scale"):
        h = fnv_mix(h, 1 if kind == "axpy" else 2)
        h = fnv_mix(h, workload["n"])
        h = fnv_mix(h, f32_bits(workload["a"]))
    elif kind == "matmul":
        h = fnv_mix(h, 3)
        for key in ("m", "k", "n"):
            h = fnv_mix(h, workload[key])
    elif kind == "rowsum":
        h = fnv_mix(h, 4)
        h = fnv_mix(h, workload["rows"])
        h = fnv_mix(h, workload["cols"])
    return h | 1


def make_inputs(workload: dict) -> tuple[np.ndarray, np.ndarray]:
    kind = workload["kind"]
    if kind in ("axpy", "scale"):
        x_len, y_len = workload["n"], workload["n"] if kind == "axpy" else 0
    elif kind == "matmul":
        x_len = workload["m"] * workload["k"]
        y_len = workload["k"] * workload["n"]
    else:
        x_len, y_len = workload["rows"] * workload["cols"], 0
    rng = XorShift64(seed_for(workload))
    x = np.array([rng.next_f32() for _ in range(x_len)], dtype=np.float32)
    y = np.array([rng.next_f32() for _ in range(y_len)], dtype=np.float32)
    return x, y


def cpu_reference(workload: dict, x: np.ndarray, y: np.ndarray) -> np.ndarray:
    """Ascending, non-fused f32 accumulation, matching the PTX and cuda.rs."""
    kind = workload["kind"]
    if kind == "axpy":
        return np.add(np.multiply(np.float32(workload["a"]), x, dtype=np.float32), y, dtype=np.float32)
    if kind == "scale":
        return np.multiply(np.float32(workload["a"]), x, dtype=np.float32)
    if kind == "matmul":
        m, k, n = workload["m"], workload["k"], workload["n"]
        a = x.reshape(m, k)
        b = y.reshape(k, n)
        out = np.zeros((m, n), dtype=np.float32)
        for t in range(k):
            out += np.outer(a[:, t], b[t, :]).astype(np.float32)
        return out.reshape(m * n)
    rows, cols = workload["rows"], workload["cols"]
    matrix = x.reshape(rows, cols)
    out = np.zeros(rows, dtype=np.float32)
    for c in range(cols):
        out += matrix[:, c]
    return out


def check(status: int, what: str) -> None:
    if status != 0:
        raise RuntimeError(f"{what} failed with CUDA status {status}")


class Driver:
    def __init__(self) -> None:
        self.lib = ctypes.CDLL("nvcuda.dll")
        lib = self.lib

        lib.cuInit.argtypes = [ctypes.c_uint]
        lib.cuInit.restype = ctypes.c_int
        lib.cuDeviceGet.argtypes = [ctypes.POINTER(ctypes.c_int), ctypes.c_int]
        lib.cuDeviceGet.restype = ctypes.c_int
        lib.cuCtxCreate_v2.argtypes = [ctypes.POINTER(VOIDP), ctypes.c_uint, ctypes.c_int]
        lib.cuCtxCreate_v2.restype = ctypes.c_int
        lib.cuCtxDestroy_v2.argtypes = [VOIDP]
        lib.cuCtxDestroy_v2.restype = ctypes.c_int
        lib.cuDeviceGetAttribute.argtypes = [ctypes.POINTER(ctypes.c_int), ctypes.c_int, ctypes.c_int]
        lib.cuDeviceGetAttribute.restype = ctypes.c_int
        lib.cuModuleLoadData.argtypes = [ctypes.POINTER(VOIDP), ctypes.c_char_p]
        lib.cuModuleLoadData.restype = ctypes.c_int
        lib.cuModuleGetFunction.argtypes = [ctypes.POINTER(VOIDP), VOIDP, ctypes.c_char_p]
        lib.cuModuleGetFunction.restype = ctypes.c_int
        lib.cuMemAlloc_v2.argtypes = [ctypes.POINTER(U64), ctypes.c_size_t]
        lib.cuMemAlloc_v2.restype = ctypes.c_int
        lib.cuMemFree_v2.argtypes = [U64]
        lib.cuMemFree_v2.restype = ctypes.c_int
        lib.cuMemcpyHtoD_v2.argtypes = [U64, VOIDP, ctypes.c_size_t]
        lib.cuMemcpyHtoD_v2.restype = ctypes.c_int
        lib.cuMemcpyDtoH_v2.argtypes = [VOIDP, U64, ctypes.c_size_t]
        lib.cuMemcpyDtoH_v2.restype = ctypes.c_int
        lib.cuLaunchKernel.argtypes = [
            VOIDP, ctypes.c_uint, ctypes.c_uint, ctypes.c_uint,
            ctypes.c_uint, ctypes.c_uint, ctypes.c_uint,
            ctypes.c_uint, VOIDP, ctypes.POINTER(VOIDP), VOIDP,
        ]
        lib.cuLaunchKernel.restype = ctypes.c_int
        lib.cuCtxSynchronize.argtypes = []
        lib.cuCtxSynchronize.restype = ctypes.c_int
        lib.cuMemGetInfo_v2.argtypes = [ctypes.POINTER(ctypes.c_size_t), ctypes.POINTER(ctypes.c_size_t)]
        lib.cuMemGetInfo_v2.restype = ctypes.c_int
        lib.cuDeviceGetName.argtypes = [ctypes.c_char_p, ctypes.c_int, ctypes.c_int]
        lib.cuDeviceGetName.restype = ctypes.c_int

        check(lib.cuInit(0), "cuInit")
        device = ctypes.c_int()
        check(lib.cuDeviceGet(ctypes.byref(device), 0), "cuDeviceGet")
        self.ctx = VOIDP()
        check(lib.cuCtxCreate_v2(ctypes.byref(self.ctx), 0, device), "cuCtxCreate_v2")
        self.device = device.value
        name = ctypes.create_string_buffer(256)
        check(lib.cuDeviceGetName(name, 256, device), "cuDeviceGetName")
        self.name = name.value.decode("utf-8", "replace")
        max_threads = ctypes.c_int()
        check(
            lib.cuDeviceGetAttribute(ctypes.byref(max_threads), 1, device),
            "cuDeviceGetAttribute(MAX_THREADS_PER_BLOCK)",
        )
        self.max_threads = max_threads.value

    def close(self) -> None:
        self.lib.cuCtxDestroy_v2(self.ctx)

    def vram_free(self) -> int:
        free = ctypes.c_size_t()
        total = ctypes.c_size_t()
        check(self.lib.cuMemGetInfo_v2(ctypes.byref(free), ctypes.byref(total)), "cuMemGetInfo_v2")
        return free.value

    def load_kernel(self, ptx_path: pathlib.Path, entry: str) -> VOIDP:
        source = ptx_path.read_text(encoding="utf-8").encode("utf-8") + b"\x00"
        module = VOIDP()
        check(self.lib.cuModuleLoadData(ctypes.byref(module), source), f"cuModuleLoadData({ptx_path.name})")
        function = VOIDP()
        check(
            self.lib.cuModuleGetFunction(ctypes.byref(function), module, entry.encode("ascii")),
            f"cuModuleGetFunction({entry})",
        )
        return function

    def alloc(self, size: int) -> U64:
        ptr = U64()
        check(self.lib.cuMemAlloc_v2(ctypes.byref(ptr), size), "cuMemAlloc_v2")
        return ptr

    def upload(self, device_ptr: U64, host: np.ndarray) -> None:
        check(self.lib.cuMemcpyHtoD_v2(device_ptr, host.ctypes.data_as(VOIDP), host.nbytes), "cuMemcpyHtoD_v2")

    def download(self, device_ptr: U64, host: np.ndarray) -> None:
        check(self.lib.cuMemcpyDtoH_v2(host.ctypes.data_as(VOIDP), device_ptr, host.nbytes), "cuMemcpyDtoH_v2")

    def launch(self, function: VOIDP, grid: tuple[int, int, int], block: tuple[int, int, int], values: list) -> None:
        keepalive = []
        pointers = []
        for value in values:
            holder = ctypes.c_uint64(value.value) if isinstance(value, U64) else value
            keepalive.append(holder)
            pointers.append(ctypes.cast(ctypes.byref(holder), VOIDP))
        params = (VOIDP * len(pointers))(*pointers)
        check(
            self.lib.cuLaunchKernel(function, *grid, *block, 0, None, params, None),
            "cuLaunchKernel",
        )
        check(self.lib.cuCtxSynchronize(), "cuCtxSynchronize")
        del keepalive


def run_workload(driver: Driver, workload: dict, ptx_dir: pathlib.Path) -> dict:
    kind = workload["kind"]
    entry = {"axpy": "axpy", "scale": "scale", "matmul": "matmul", "rowsum": "rowsum"}[kind]
    function = driver.load_kernel(ptx_dir / f"{entry}.ptx", entry)
    x, y = make_inputs(workload)
    expected = cpu_reference(workload, x, y)
    out = np.zeros_like(expected)

    before = driver.vram_free()
    transfers: list[tuple[U64, U64, U64, int]] = []

    if kind in ("axpy", "scale"):
        n = workload["n"]
        x_dev = driver.alloc(n * 4)
        y_dev = driver.alloc(n * 4) if kind == "axpy" else U64(0)
        out_dev = driver.alloc(n * 4)
        transfers.append((x_dev, y_dev, out_dev, n * 4))
        t0 = time.perf_counter()
        driver.upload(x_dev, x)
        if kind == "axpy":
            driver.upload(y_dev, y)
        transfer_in = time.perf_counter() - t0

        block = min(driver.max_threads, 256)
        scalar = np.float32(workload["a"])
        compute_start = time.perf_counter()
        for offset in range(0, n, VECTOR_CHUNK):
            length = min(n - offset, VECTOR_CHUNK)
            px = U64(x_dev.value + offset * 4)
            py = U64(y_dev.value + offset * 4) if kind == "axpy" else U64(0)
            po = U64(out_dev.value + offset * 4)
            n32 = ctypes.c_uint32(length)
            args = [px, py, po, ctypes.c_float(scalar), n32] if kind == "axpy" else [px, po, ctypes.c_float(scalar), n32]
            driver.launch(function, ((length + block - 1) // block, 1, 1), (block, 1, 1), args)
        compute = time.perf_counter() - compute_start

        t0 = time.perf_counter()
        driver.download(out_dev, out)
        transfer_out = time.perf_counter() - t0
        for ptr in (x_dev, y_dev, out_dev):
            if ptr.value:
                driver.lib.cuMemFree_v2(ptr)
    elif kind == "matmul":
        m, k, n = workload["m"], workload["k"], workload["n"]
        a_dev, b_dev, c_dev = driver.alloc(m * k * 4), driver.alloc(k * n * 4), driver.alloc(m * n * 4)
        transfers.append((a_dev, b_dev, c_dev, (m * k + k * n + m * n) * 4))
        t0 = time.perf_counter()
        driver.upload(a_dev, x)
        driver.upload(b_dev, y)
        transfer_in = time.perf_counter() - t0

        tile = 16
        while tile * tile > driver.max_threads and tile > 1:
            tile //= 2
        args = [a_dev, b_dev, c_dev, ctypes.c_uint32(m), ctypes.c_uint32(k), ctypes.c_uint32(n)]
        compute_start = time.perf_counter()
        driver.launch(function, ((n + tile - 1) // tile, (m + tile - 1) // tile, 1), (tile, tile, 1), args)
        compute = time.perf_counter() - compute_start

        t0 = time.perf_counter()
        driver.download(c_dev, out)
        transfer_out = time.perf_counter() - t0
        for ptr in (a_dev, b_dev, c_dev):
            driver.lib.cuMemFree_v2(ptr)
    else:
        rows, cols = workload["rows"], workload["cols"]
        a_dev, out_dev = driver.alloc(rows * cols * 4), driver.alloc(rows * 4)
        transfers.append((a_dev, U64(0), out_dev, (rows * cols + rows) * 4))
        t0 = time.perf_counter()
        driver.upload(a_dev, x)
        transfer_in = time.perf_counter() - t0
        block = min(driver.max_threads, 256)
        args = [a_dev, out_dev, ctypes.c_uint32(rows), ctypes.c_uint32(cols)]
        compute_start = time.perf_counter()
        driver.launch(function, ((rows + block - 1) // block, 1, 1), (block, 1, 1), args)
        compute = time.perf_counter() - compute_start
        t0 = time.perf_counter()
        driver.download(out_dev, out)
        transfer_out = time.perf_counter() - t0
        driver.lib.cuMemFree_v2(a_dev)
        driver.lib.cuMemFree_v2(out_dev)

    after = driver.vram_free()
    tolerance = TOLERANCE * np.maximum(1.0, np.abs(expected.astype(np.float64)))
    error = float(np.max(np.abs(out.astype(np.float64) - expected.astype(np.float64))))
    passed = bool(np.all(np.abs(out.astype(np.float64) - expected.astype(np.float64)) <= tolerance))
    total = transfer_in + compute + transfer_out
    return {
        "workload": kind,
        "params": {key: value for key, value in workload.items() if key != "kind"},
        "pass": passed,
        "max_abs_error": error,
        "tolerance": TOLERANCE,
        "transfer_in_ms": round(transfer_in * 1000, 4),
        "kernel_ms": round(compute * 1000, 4),
        "transfer_out_ms": round(transfer_out * 1000, 4),
        "gpu_total_ms": round(total * 1000, 4),
        "vram_before_free_mib": round(before / 1048576, 1),
        "vram_after_free_mib": round(after / 1048576, 1),
        "vram_released": after >= before,
    }


WORKLOADS = [
    {"kind": "axpy", "n": 1_048_576, "a": 1.5},
    {"kind": "scale", "n": 1_048_576, "a": 1.5},
    {"kind": "matmul", "m": 256, "k": 256, "n": 256},
    {"kind": "rowsum", "rows": 1024, "cols": 1024},
]


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--ptx", type=pathlib.Path, default=pathlib.Path(__file__).resolve().parent.parent / "src-tauri" / "ptx")
    parser.add_argument("--out", type=pathlib.Path, required=True)
    args = parser.parse_args()

    driver = Driver()
    try:
        results = [run_workload(driver, workload, args.ptx) for workload in WORKLOADS]
    finally:
        driver.close()

    passed = all(result["pass"] for result in results)
    released = all(result["vram_released"] for result in results)
    report = {
        "driver": driver.name,
        "max_threads_per_block": driver.max_threads,
        "all_validated": passed,
        "all_vram_released": released,
        "results": results,
    }
    args.out.parent.mkdir(parents=True, exist_ok=True)
    args.out.write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")
    print(json.dumps(report, indent=2))
    return 0 if passed and released else 1


if __name__ == "__main__":
    sys.exit(main())
