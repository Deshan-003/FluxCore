# Performance prediction

Predictions are lightweight statistical estimates, not guaranteed future values. They use at most 30 minutes of local Phase 4 observations sampled every 15 seconds and are recalculated no more than once per minute. CPU, RAM, commit charge, adapter-scoped GPU/VRAM use and per-volume disk capacity/free bytes use separately identified series. Phase 2's existing memory pressure/exhaustion forecast remains authoritative for memory pressure. Disk capacity is only forecast where a real volume total/free counter exists. Process workload prediction is not implemented.

The engine requires a current metric, at least eight observations, two minutes of observation span, at least 75% metric coverage, and no observation gap greater than 90 seconds. Otherwise it returns an unavailable estimate with its data requirement. It evaluates recent walk-forward error for a rolling mean, time-adjusted exponential smoother and linear trend, then uses the least-error model for the requested lead. CPU/RAM/commit/GPU/VRAM-use/disk-capacity percentages are bounded to their meaningful 0–100 range; byte forecasts cannot be negative.

