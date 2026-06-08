# Optimization Safety

FluxCore Phase 2 is deliberately the opposite of "RAM booster" utilities. Most consumer RAM
cleaners create a misleading result — forcibly emptying working sets so the free-RAM number
jumps — while making the system slower (pages immediately fault back in from disk) and hiding
real memory problems. FluxCore optimizes **understanding** first, touches only its own resources,
and refuses entire categories of operations by design.

## Forbidden operations (rejected by the safety engine)

`safety.rs` maintains a hard rejection list. These IDs are refused in every mode, with or
without confirmation, and are not part of the action registry:

| Forbidden | Why |
| --- | --- |
| `kill_process` / `terminate_process` | FluxCore never terminates applications — data loss and instability are unacceptable side effects of "cleaning RAM". |
| `empty_working_set` / `trim_working_set` | Working-set trimming is not routine RAM cleaning. It forces hard page faults, slows the target application, and the freed bytes are usually re-committed within seconds. The apparent gain is cosmetic. |
| `purge_standby_list` | Blindly purging standby/cache throws away warm data Windows would otherwise reuse for free; the cache refills at the cost of disk I/O. |
| `resize_pagefile` / `disable_pagefile` | The pagefile is never resized, moved or disabled programmatically. FluxCore shows the current configuration, explains the manual Windows steps and their consequences (commit limit, crash dumps, app compatibility), and leaves the decision — and the action — to the user. |
| `disable_compression` | Windows memory compression is an OS-managed feature; disabling it via undocumented techniques is out of scope. |

No kernel drivers are used or introduced. No undocumented APIs are called. All data collection
uses documented Win32/PDH interfaces (see [WINDOWS_COMPATIBILITY.md](WINDOWS_COMPATIBILITY.md)).

## Action registry — everything that *can* run

Exactly seven actions exist (`optimizer.rs`). None changes system state (`changes_system: false`
for all); four affect only FluxCore itself, three are read-only guidance:

1. **Clear FluxCore caches** — drops in-memory history buffers beyond retention and prunes
   expired database rows. Side effect: older charts lose detail. Confirmation required.
2. **Release unused FluxCore resources** — shrinks trend windows, recommendation log and audit
   ring to minimums. Side effect: trend/prediction quality degrades until windows refill.
   Confirmation required.
3. **Pause FluxCore background activity** — suspends process scanning and persistence for five
   minutes; lightweight analysis continues. Side effect: a five-minute gap in trends/history.
   Confirmation required.
4. **Refresh stale monitoring data** — forces a process rescan on the next tick. No
   confirmation (no side effect beyond one normal-cost collection).
5. **Guidance: closing unnecessary applications** — lists top private-memory consumers so the
   *user* decides. FluxCore never closes anything.
6. **Guidance: managing startup applications** — reads the documented Run registry keys
   (HKCU/HKLM, read-only) and lists entries. Never modifies them.
7. **Guidance: pagefile configuration** — shows current pagefile setup, explains manual steps
   and consequences. Never changes it.

Anything not in this list cannot be invoked: `execute_optimization_action` resolves the ID
against the registry first, and `safety::validate` then re-checks the forbidden list, the current
mode, and the confirmation flag. A unit test asserts the registry contains no destructive action
and that every forbidden ID is rejected in every mode.

## Validation flow

```
UI click → execute_optimization_action(action_id, confirmed)
  1. descriptor = registry.lookup(action_id)      → unknown ID: rejected + audited
  2. safety::validate(mode, descriptor, confirmed)
       - forbidden ID               → rejected + audited
       - mode == AnalysisOnly       → rejected + audited
       - requires_confirmation and !confirmed → rejected + audited
  3. run_action(descriptor)                       → success/failed + audited
  4. AuditEntry { action, mode, confirmed, status, detail, timestamp }
```

Every outcome — executed, rejected, failed or cancelled — is written to `optimization_actions`
and shown in the Optimization Center history with mode, confirmation state and detail text.

## User confirmation

- Recommendations that map to an action show **Approve & run**; the first click arms the button
  ("Click again to confirm"), the second executes with `confirmed: true`.
- Manual actions with `requires_confirmation` use the same two-step flow.
- `analysisOnly` mode renders action buttons inert with an explanation — the mode guarantee is
  enforced in the backend, not just the UI.

## Honesty rules baked into the engine

- **No invented savings.** Recommendation `expected_benefit` is qualitative ("reduces background
  overhead while you work"); byte-count promises appear nowhere.
- **Cached/standby ≠ unavailable.** The analyzer reports standby, modified and cache bytes as
  reclaimable memory Windows is using *well*, and available memory excludes them correctly.
- **VRAM is not RAM.** GPU memory is never added to physical RAM, never offered as an extension
  of it, and no action moves application memory to the GPU.
- **Labels require evidence.** "Suspected growth pattern" appears only after ≥ 8 samples over
  ≥ 4 minutes with strong regression fit; a single large allocation never triggers it.
- **Limitations are surfaced.** Missing PDH counters, unavailable private-working-set values and
  other gaps are collected into `limitations` and rendered in the UI rather than papered over.

## Stability first

The engine never sacrifices system stability for a better-looking number: it cannot terminate,
trim, purge, resize or disable anything belonging to Windows or other applications. The worst
case of any FluxCore action is degraded *FluxCore* telemetry quality, which the side-effect
descriptors state up front.
