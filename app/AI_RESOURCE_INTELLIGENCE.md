# AI Resource Intelligence

FluxCore Phase 4 uses **offline statistical analysis**, not a language model or claim of generative AI. Its intelligence loop consumes the actual `SystemSnapshot` from Phase 1 on the existing monitoring thread, then reads the Phase 2 `MemoryIntelligenceSnapshot` and Phase 3 GPU resource/task summary. It does not enumerate hardware or processes a second time.

One observation at most every 15 seconds includes available CPU use/frequency/core load, physical RAM and optional commit/pagefile/page-read counters, distinct GPU and VRAM metrics, per-volume capacity/read/write rates, active process count and the measurement cost of Phase 2 detailed process scans. Phase 2 contributes process-private-byte rankings only for currently matching PIDs and start times. Phase 3 contributes the existing budget/queue summary and recent validated, non-fallback workload runs. Missing values remain absent. Hardware and process telemetry stay local in SQLite; there is no external service or model.

