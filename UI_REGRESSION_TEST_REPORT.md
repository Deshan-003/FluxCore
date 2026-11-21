# FluxCore UI Regression Test Report

**Date:** 2026-11-21 · **Scope:** Post-integration regression audit of the approved UI redesign (Stage 2), per the workflow brief §16.
**Branch under test:** working tree on top of `main` (`f3bc365`) — see the integration PR.

---

## 1. Automated checks executed locally

| Check | Command | Result | Evidence |
|---|---|---|---|
| TypeScript (strict, whole app) | `npx tsc -b` | ✅ 0 errors | clean run |
| ESLint (whole app incl. tests) | `npm run lint` | ✅ 0 errors, 0 warnings | clean run |
| Frontend unit/integration tests | `npm run test:frontend` | ✅ **53/53 passed** (7 files, ~7 s) | `vitest` summary |
| Production build (incl. `tsc -b`) | `npm run build` | ✅ built in ~11 s; CSS 29.24 KB (was 113.54 KB), JS 801.64 KB | `vite build` output |
| Runtime render (integrated app) | dev server + Chromium at 1440×900 | ✅ new shell, nav, gating, theme persistence verified | `artifacts/ui-integration/i01–i04` |
| Rust native suite (106 tests) | `npm test` → `cargo test` | ➡️ **delegated to CI** — `cargo` is not installed on this machine; the CI workflow runs it on every push/PR | CI run on this branch |
| Tauri production build | `npm run tauri build` | ➡️ **delegated to CI** (installer job) for the same reason | CI artifacts |
| WebdriverIO E2E (3 specs) | `e2e-tests/` via tauri-driver | ➡️ **delegated to CI** (requires the debug desktop build); spec updated to the new DOM with identical behavioral assertions | CI run on this branch |

**Frontend test inventory (53):** `lib/format` 15 · `lib/processes` 9 · `stores/monitoring` 7 · `ui/kit` 11 · `pages/GpuAcceleration` 4 · `pages/ResourceIntelligence` 4 · `App` 3.

## 2. Manual regression audit (browser runtime + code-level verification)

Behavior-by-behavior against the brief §16 checklist. Hardware-backed screens were verified at the code/wiring level (real commands/stores) plus unit tests; the desktop runtime itself is exercised by CI E2E.

| Area | Verification | Status |
|---|---|---|
| Navigation between all pages | Store-driven router with stable ids; every nav item routes; active state + `aria-current`; grouping matches approved design; collapses to rail | ✅ |
| Dashboard metric updates | Metric cards bind to snapshot + history sparklines; status chips derive from documented thresholds (`statusForPercent`, unit-tested) | ✅ |
| CPU charts | CPU-only series (fixes the old wrong-series bug); windowed by settings.historyDurationMinutes; token-styled axes/tooltip | ✅ |
| RAM charts | Physical-memory series; committed/pagefile counters as distinct values with explanations | ✅ |
| GPU charts | ComposedChart GPU% + VRAM% with legend; empty-state until 2 samples | ✅ |
| VRAM display | Dedicated VRAM / shared memory separated with "own memory, never used as system RAM" statements (unit-tested) | ✅ |
| Storage information | Capacity bars, type badges, read/write rates; SMART shown Unavailable with reason | ✅ |
| Process search & sorting | `filterProcesses` (unchanged, 9 unit tests); advanced columns (PID/GPU) behind a switch; store search/sort preserved | ✅ |
| Memory management actions | Registry actions run through ConfirmDialog → `execute_optimization_action(id, true)` → audit + toast; availability gated by mode | ✅ |
| GPU acceleration controls | Mode → `set_acceleration_mode`; limits/priority/pins → draft config → `update_gpu_resource_config`; workload run → `submit_workload_task`; re-probe → `probe_gpu_providers` | ✅ |
| Task cancellation | Cancel button → ConfirmDialog → `cancel_task` (unit-tested with mocked service) | ✅ |
| Optimization approvals | Recommendation "Approve & apply" → ConfirmDialog → `respond_to_resource_recommendation(id,'apply',true)`; disabled when an override is active or policy paused (unit-tested) | ✅ |
| Settings persistence | `updateSettings` path untouched: localStorage write + backend save + rollback on rejection (7 store tests pass); theme applies via token system | ✅ |
| Notifications | Toasts on completed actions (unit-tested); collector/error banners preserved | ✅ |
| Loading states | Skeletons for GPU engine load; chart empty-states until ≥2 samples; button busy states | ✅ |
| Empty states | GPU/Storage/Processes/Recommendations/Anomalies/history all have explained empty states; no-snapshot gate shows the honest waiting state (unit-tested) | ✅ |
| Error states | Store errors surface as status-edge banners; GPU page retry; settings rejection banner (store-tested) | ✅ |
| Responsive layouts | Fluid grids (auto-fit metric row, grid-side collapse ≤1200 px, stacks ≤992 px, sidebar rail); verified at 1440/1280/1000 px in Stage 1 (unchanged CSS) | ✅ |
| Keyboard navigation | Global `:focus-visible` ring; tabs/switches/selects are native buttons/inputs; dialog Esc handling (unit-tested); icon buttons labeled | ✅ |

## 3. Phase 1–4 backend integrity

- **Git scope:** zero changes under `src-tauri/` (no Rust file modified); zero changes to `src/stores/*` business logic except the `page` string values; `src/services/*` untouched; `src/types/*` untouched.
- **Store tests:** all 7 `stores/monitoring.test.ts` tests pass, including settings rollback semantics and null-preserving history (`toHistory`).
- **Contract honesty:** `pct`/`fmtBytes`/`fmtRate` unchanged — "Unavailable" placeholders intact everywhere (App unit tests assert no fabricated `0 MHz`/`0 W`).
- **Commands/events used by the new UI** (unchanged set): `get_snapshot`, `get_settings`, `save_settings`, `get_history`, `check_hardware`, `get/set_optimization_mode`, `get_optimization_actions`, `execute_optimization_action`, `get/update_memory_policy_config`, `set_dashboard_active`, `get_memory_intelligence`, `get_memory_history_series`, `get_resource_intelligence`, `get_resource_history`, `configure_resource_intelligence`, `respond_to_resource_recommendation`, `apply_resource_policy`, `revert_resource_policy`, `get_workload_recommendations`, `get_gpu_acceleration_state`, `set_acceleration_mode`, `get/update_gpu_resource_config`, `probe_gpu_providers`, `submit_workload_task`, `cancel_task`, `clear_finished_tasks`, `get_task_history`, `run_benchmark`, `get/clear_benchmark_history`, `get_gpu_environment`; events `fluxcore-snapshot`, `fluxcore-gpu-task`, `fluxcore-gpu-capabilities`.

## 4. Remaining CI gates (on the branch)

1. `cargo test` — 106 native tests.
2. `tauri build` — MSI/NSIS installer build.
3. WebdriverIO E2E — 3 specs (collector online, sidebar navigation, monitoring toggle flip) against the debug build with the updated selectors.

These run automatically in `.github/workflows/native-verification.yml` on push/PR. Local verification covered everything that does not require the Rust toolchain.

## 5. Issues found and fixed during the regression pass

| Finding | Fix |
|---|---|
| `useMemo` called after an early return in `Processes.tsx` (rules-of-hooks) | Reordered; memoization removed (cheap pure filter) |
| Kit sparkline typed `t: number` but production timestamps are ISO strings | Widened to `t: number \| string` (sparkline is index-based) |
| Missing `EmptyState`/`Disclose` imports in two new pages | Added |
| `RXTooltip` formatter signature mismatch in forecast chart | Simplified to the typed single-argument formatter |
| Test assertion drift (byte-string formatting, wording of VRAM statements, store-owned web-preview message) | Assertions aligned to the shipped, store-backed wording |

No regressions against Phase 1–4 functionality were found; all failures encountered during the pass were in the new test code itself, not the integrated UI.
