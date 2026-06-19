# Adaptive optimization

The policy is additive to the existing Phase 2 memory-policy configuration and Phase 3 GPU workload configuration. It is persistent in migration 5 and starts in `analysisOnly`.

| Mode | Behavior |
| --- | --- |
| Analysis Only | Observe local measurements, forecasts, anomalies and recommendations. No actions. Default. |
| Smart Assist | Offer the supported scan-cadence recommendation for user approval. Validate and confirm in the backend before applying. |
| Adaptive Optimization | Requires explicit enablement. It may apply the supported, temporary detailed-process scan interval only under the constraints below, record an action, compare measurements and revert. |
| Manual Control | Let the user select the same supported saved scan interval and apply/revert it manually. |

## Guarded action

The only action is a minimum interval on the existing Phase 2 detailed process scans. It does not pause or slow hardware collection or alter Windows or third-party app settings. To be considered, CPU must be sustained at the existing anomaly threshold, memory pressure must be normal, the current sample must be recent, and both monitoring and persisted history must be enabled. The backend also checks a healthy database, user-configured interval/max/cooldown, no existing override and an explicit adaptive consent record. Smart Assist and Manual require an IPC confirmation flag as well as user confirmation in the frontend.

The override lasts at most three minutes. Elevated memory pressure, unknown pressure, a mode/policy change, disabling collection/history, or expiry restores the Phase 2 cadence. Adaptive mode additionally reverts on sustained CPU recovery or a measured regression. A cooldown limits repeated changes. Every application/close/evaluation is recorded locally. A restart treats an unfinished action as interrupted; the Phase 2 setting was never changed persistently or in Windows.

Outcome evaluation compares the Phase 2 measured scan cost before and after and checks CPU, RAM, process-count, and observed GPU utilization stability. Missing or materially changed workloads make the result inconclusive. This is association under observed conditions, not evidence that FluxCore improved overall responsiveness. Results and limitations are displayed in Optimization History.

No supported adaptive action changes GPU mode or budgets, selects a workload execution mode, modifies monitoring intervals, disables optional Windows tasks, changes unrelated application settings, alters memory compression/pagefile, or deletes data. GPU decisions use existing Phase 3 recommendations only; users submit those workloads through the existing Phase 3 UI themselves.
