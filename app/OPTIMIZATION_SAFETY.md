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

