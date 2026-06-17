# Resource anomaly detection

The detector is local and statistical, not malware/security detection. It tracks CPU, logical-core peak, RAM and commit percentages, physical page-read rate, adapter-scoped GPU/VRAM use, per-volume storage capacity and sustained throughput, only when those actual metrics are reported.

Absolute resource pressure limits are 90% CPU/GPU, 97% logical-core peak, 92% physical RAM/VRAM, 90% commit, and 95% used storage capacity. Throughput has no universal hardware threshold: it is compared against a recent baseline. That baseline uses at least eight samples from 90 seconds to 30 minutes before the current reading, with median and median absolute deviation. For high-sided percentage/counter events, the detector chooses the more conservative bound from the absolute threshold and a baseline delta. It asks for 45 seconds of continuous abnormal readings before opening an event.

