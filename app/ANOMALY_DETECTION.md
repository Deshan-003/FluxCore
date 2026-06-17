# Resource anomaly detection

The detector is local and statistical, not malware/security detection. It tracks CPU, logical-core peak, RAM and commit percentages, physical page-read rate, adapter-scoped GPU/VRAM use, per-volume storage capacity and sustained throughput, only when those actual metrics are reported.

Absolute resource pressure limits are 90% CPU/GPU, 97% logical-core peak, 92% physical RAM/VRAM, 90% commit, and 95% used storage capacity. Throughput has no universal hardware threshold: it is compared against a recent baseline. That baseline uses at least eight samples from 90 seconds to 30 minutes before the current reading, with median and median absolute deviation. For high-sided percentage/counter events, the detector chooses the more conservative bound from the absolute threshold and a baseline delta. It asks for 45 seconds of continuous abnormal readings before opening an event.

An open event clears only after at least 45 seconds below a half-delta hysteresis band. Missing data, unsupported hardware, a gap longer than 90 seconds, or a monitoring/history mode transition closes an event as **interrupted** rather than claiming recovery. Events carry resource, value, threshold, start/end and a plain explanation. Severity values are information, low, medium, high and critical; they are rule-based triage, not a probability.

The engine reduces one-off spikes and resets baselines naturally through bounded sampling. Process-memory growth recommendations reuse Phase 2 sustained-history/leak indicators and are only presented for a currently matching process identity. A high resource reading is not evidence of malicious intent. No process is killed or altered.

Tests cover spike filtering, sustained opening/recovery, missing-data interruption, robust outlier baseline and Phase 2 compatibility. Real-world false-positive rates and long-duration workload baselines have not been measured.
