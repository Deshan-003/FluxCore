# Adaptive optimization

The policy is additive to the existing Phase 2 memory-policy configuration and Phase 3 GPU workload configuration. It is persistent in migration 5 and starts in `analysisOnly`.

| Mode | Behavior |
| --- | --- |
| Analysis Only | Observe local measurements, forecasts, anomalies and recommendations. No actions. Default. |
| Smart Assist | Offer the supported scan-cadence recommendation for user approval. Validate and confirm in the backend before applying. |
| Adaptive Optimization | Requires explicit enablement. It may apply the supported, temporary detailed-process scan interval only under the constraints below, record an action, compare measurements and revert. |
| Manual Control | Let the user select the same supported saved scan interval and apply/revert it manually. |

## Guarded action

