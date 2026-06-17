# Phase 4 progress audit

The starting branch `main` (`d7953fe`) contains Phases 1–2, but no workload scheduler. Phase 4 builds on the repository's `feature/phase3-gpu-acceleration` tip (`49c902b`), which descends from `main`. No Phase 3 code is being recreated.

| Phase and capability | Status at audit | Reusable interface and evidence |
| --- | --- | --- |
| 1: CPU, physical RAM, processes | COMPLETE for documented counters; PARTIAL for sensors | `MonitorEngine`, `SystemSnapshot`, and the existing monitor worker provide aggregate and per-core load, frequency, process CPU and physical memory. CPU temperature and per-process GPU attribution are not supplied. |
| 1: GPU, VRAM and storage | PARTIAL | NVML provides supported NVIDIA telemetry; other adapters may only be enumerated. Mounted volumes expose capacity and, where supported, read/write rates. Throughput is neither busy time nor latency and cannot prove a storage bottleneck. |
| 1: Historical samples | PARTIAL for Phase 4 | SQLite `performance_snapshots` stores aggregate percentages, VRAM bytes and I/O rates, but conflates GPU identities and lacks per-volume capacity and pressure signals. Do not backfill unsupported fields or mix this data with device-scoped forecasts. |
| 2: Pressure, process growth, memory prediction | COMPLETE for source interfaces; Windows counter coverage PARTIAL | `MemoryEngine::tick`, `MemoryIntelligenceSnapshot`, the existing pressure hysteresis, process-growth analysis and memory history. Commit, pagefile and physical page-read counters are optional. A page read is a paging proxy, not attribution to the pagefile. |
| 2: Optimization settings and execution | PARTIAL for adaptation | `PolicyConfig` persists and existing actions are limited to FluxCore or guidance. The Phase 2 mode is in-memory. Cache eviction and one-shot pauses are not reversible and are not eligible for Phase 4 automation. |
