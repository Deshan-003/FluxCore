# AI Resource Intelligence

FluxCore Phase 4 uses **offline statistical analysis**, not a language model or claim of generative AI. Its intelligence loop consumes the actual `SystemSnapshot` from Phase 1 on the existing monitoring thread, then reads the Phase 2 `MemoryIntelligenceSnapshot` and Phase 3 GPU resource/task summary. It does not enumerate hardware or processes a second time.

One observation at most every 15 seconds includes available CPU use/frequency/core load, physical RAM and optional commit/pagefile/page-read counters, distinct GPU and VRAM metrics, per-volume capacity/read/write rates, active process count and the measurement cost of Phase 2 detailed process scans. Phase 2 contributes process-private-byte rankings only for currently matching PIDs and start times. Phase 3 contributes the existing budget/queue summary and recent validated, non-fallback workload runs. Missing values remain absent. Hardware and process telemetry stay local in SQLite; there is no external service or model.

## Interpretation and recommendations

Cross-resource explanations use evidence in the latest same-time observation, existing Phase 2 pressure and durable anomalies. Physical page reads plus low availability may be consistent with memory pressure; they are not proof of pagefile traffic. Read/write throughput is not a disk-latency metric. A busy process may be performing useful work. Resource use alone is never called malware, nor is correlation described as proven cause.

Recommendations include the actual evidence, resource, user-level action, qualitative possible benefit, risk, confidence and approval requirement. CPU-heavy processes, private-memory consumers and growth use measured process records. GPU guidance uses the already-integrated Phase 3 workload profiler and stays read-only. Disk-capacity actions ask the user to review files; FluxCore never deletes them.

Prediction and anomaly methodology: [PERFORMANCE_PREDICTION.md](PERFORMANCE_PREDICTION.md) and [ANOMALY_DETECTION.md](ANOMALY_DETECTION.md). Actions and safety boundaries: [ADAPTIVE_OPTIMIZATION.md](ADAPTIVE_OPTIMIZATION.md) and [OPTIMIZATION_SAFETY.md](OPTIMIZATION_SAFETY.md).
