# Phase 4 — Resource Intelligence architecture

## Data path

The existing `MonitorEngine` remains the only hardware collector. Its loop invokes the existing `MemoryEngine::tick` and then the Phase 4 `IntelligenceEngine::tick` on the same `SystemSnapshot`. No second CPU, RAM, GPU, storage, or process collector was introduced. Phase 2 contributes corroborating memory pressure, optional page-read counters, process private-byte rankings, and sustained process-growth indicators. Phase 3 contributes a read-only GPU resource-budget/task summary and its existing profiler; Phase 4 never submits a GPU task, changes the GPU mode, or changes its budget.

A Phase 4 observation takes one bounded sample every 15 seconds. Metrics stay optional; GPU adapters and volumes have distinct labels and capacity is distinguished from utilization. The observation includes cumulative time/count spent in the Phase 2 detailed process scans so an owned policy can be measured. A UTC Unix-second key is used for observations and forecast target matching. The Tauri commands are `get_resource_intelligence`, `get_resource_history`, `configure_resource_intelligence`, `respond_to_resource_recommendation`, `apply_resource_policy`, and `revert_resource_policy`.

## Analysis

The offline engine uses robust median/MAD baselines, persistence-duration and recovery hysteresis for anomalies, and lightweight walk-forward selection among rolling mean, time-adjusted exponential smoothing, and linear trend. Forecasts are withheld until there are at least eight current, reasonably covered samples spanning two minutes; longer horizons receive lower confidence when the observations do not support them. Forecast intervals are heuristic residual bands, explicitly not calibrated probabilities. Existing Phase 2 memory forecasting remains the authority for memory-pressure estimates.

Resource interpretations use corroborating signals and plain-language limitations. For example, physical page reads are not pagefile-attributed I/O, storage throughput does not measure latency, and simultaneous metrics do not establish causation. No resource metric is interpreted as malicious activity. For GPU workload suitability, reuse Phase 3’s registered profiler and its benchmark/capability/VRAM safety gates.
