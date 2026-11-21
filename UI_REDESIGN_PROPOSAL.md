# FluxCore UI Redesign Proposal

**Status:** Stage-1 deliverable — **awaiting your explicit approval**. Nothing in the production app has been changed; the redesign lives in an isolated preview (`app/src/redesign/preview/`), mounted only when the URL hash starts with `#/redesign` (see isolation guarantees below).
**Companion documents:** `UI_UX_AUDIT_REPORT.md` (41 verified findings) · `UI_DESIGN_SYSTEM.md` (the design contract) · `UI_REDESIGN_SCREENSHOT_INDEX.md` (25 verified runtime screenshots).

---

## 1. What is proposed

A complete, token-driven redesign of all **12 existing pages** plus the app shell, implementing the design system in `UI_DESIGN_SYSTEM.md`:

| Area | Change |
|---|---|
| **Foundations** | One semantic token layer (`--rx-*`, dark-first + real curated light theme) replacing the dual theme systems, 125+ hardcoded hex colors, the filter-hack light mode, and the CSP-blocked Google Fonts. Local Windows font stack (Segoe UI Variable / Cascadia Mono numerals). |
| **Navigation** | Sidebar grouped **Overview / Monitoring / Optimization / Intelligence / System** (maps 1:1 to the 12 real pages — no invented pages); collapsible to an icon rail; hash-based page routing (`#/redesign/<page-id>`) demonstrating deep-linkable navigation with stable page ids separated from labels. |
| **Page template** | Every page: `h1` + page-specific purpose sentence → key metrics → primary chart(s) → detail sections → collapsible "Technical details". Sentence-case headings, consistent spacing. |
| **Language** | Beginner-friendly explanations everywhere via info-tips and captions; technical terms kept but explained; predictions always labeled; "Unavailable" always carries a reason. |
| **Interaction kit** | One Button system, unified ConfirmDialog (What happens / Benefit / Risk) replacing `window.confirm` and double-click confirms, toast notifications for every action outcome, skeletons for loading, friendly errors with technical disclosure, InfoTip for metric explanations. |
| **Status language** | Healthy / Normal / Needs Attention / Critical / Unavailable — always icon + text + color, with documented thresholds; the dashboard verdict explains its own calculation. |
| **Charts** | Single token-styled chart approach; fixed resource→color mapping (CPU cyan, Memory violet, GPU amber, VRAM rose, Storage teal); unit formatters; legends with labels; measured vs predicted visually distinguished. |

## 2. How the existing UI maps to the redesign

| Current page | Redesigned page (preview route) | Notes |
|---|---|---|
| Overview | `#/redesign/dashboard` | Documented per-resource statuses replace the CPU+RAM-only orb; insights & quick actions added |
| CPU | `#/redesign/cpu` | CPU-only history chart; per-core grid retained |
| Memory | `#/redesign/memory` | Physical vs committed vs pagefile separation with explanations |
| GPU | `#/redesign/gpu` | VRAM / shared memory as first-class explained cards; technical details collapsed |
| Storage | `#/redesign/storage` | Drive cards + provenance ("where each number comes from") |
| Processes | `#/redesign/processes` | Advanced columns (PID/GPU) behind a switch |
| Hardware | `#/redesign/hardware` | "PC at a glance" + diagnostics with status chips |
| Memory Intelligence | `#/redesign/memory-management` | Renamed **Memory Management**; tabbed history; explained actions |
| Optimization Center | `#/redesign/adaptive-optimization` | Renamed **Adaptive Optimization**; mode cards state what each mode may change |
| GPU Acceleration | `#/redesign/gpu-acceleration` | Same 5 areas, unified Tabs, explanations for budget/workloads/benchmarks |
| Resource Intelligence | `#/redesign/resource-intelligence` | Pill-nav → unified Tabs; forecast vs measured distinction |
| Settings | `#/redesign/settings` | Real theme switching; unwired features marked "Planned" |

Rename note: the two renamed pages keep their store/contract identity — only display labels change ("Memory Intelligence" → "Memory Management", "Optimization Center" → "Adaptive Optimization"). If you prefer the current names, integration can keep them; the design does not depend on the rename.

## 3. Isolation guarantees (what was and wasn't touched)

- **Production UI untouched.** `src/main.tsx` gained an 8-line hash-gate: without `#/redesign` in the URL, the production `App` renders exactly as before. The preview is a separate lazy-loaded chunk (`dist/assets/RedesignApp-*.js`) — the production bundle contains none of the preview code at load time.
- **No backend changes.** Zero changes to Rust commands, stores, services, types, or the SQLite/GPU/intelligence logic (Phases 1–4 untouched).
- **Preview data is fenced.** All sample data lives in `preview-data.ts` with a header banner; no production store or command is imported by the preview.
- **Verified:** `tsc -b` clean · `eslint .` clean · all **50 existing frontend tests pass** · `vite build` (production) succeeds · `App.test.tsx` and friends render the production UI unchanged.
- New files: `src/redesign/**` (README + preview code). Modified files: **only `src/main.tsx`** (the hash-gate).

## 4. Data mapping for integration (after approval)

Each preview page already consumes data shaped like the real contracts; integration swaps `preview-data.ts` for the existing stores — no page redesign needed:

| Preview section | Real source (existing) |
|---|---|
| Metric cards, hero bands, charts | `useMonitoringStore` (`snapshot`, `history`, `fluxcore-snapshot` event, `get_history`) |
| Memory Management tiles/analysis/actions | `useMemoryStore` (`get_memory_intelligence`, `get_memory_history_series`, `execute_optimization_action`, …) |
| Adaptive Optimization modes/policy/history | `useMemoryStore` (`get/set_optimization_mode`, `get/update_memory_policy_config`, `get_optimization_actions`) |
| GPU Acceleration (all 5 tabs) | `useGpuStore` + 17 commands + `fluxcore-gpu-task` / `fluxcore-gpu-capabilities` events |
| Resource Intelligence (all 6 tabs) | `useResourceStore` (`get_resource_intelligence`, `get_resource_history`, `apply_resource_policy`, …) |
| Hardware diagnostics | `check_hardware` command |
| Settings | `useMonitoringStore.updateSettings` (backend-validated, rollback preserved) |

Interaction semantics to preserve at integration: two-click confirms and `window.confirm` flows become the ConfirmDialog; inline result lines **additionally** surface as toasts; status thresholds stay configurable in the backend and are displayed with the status.

## 5. What integration will do (only after your approval)

1. Move the approved components from `src/redesign/preview/ui.tsx` into production shared components; adopt the token stylesheet as the app stylesheet (retiring `App.css` overrides and the unused 67 shadcn components).
2. Rewire each redesigned page to its real store/service per the table above; wire every control (toggles, selects, mode cards, actions) to existing commands — no decorative controls in production.
3. Implement hash-routing for the production app (page ids) while preserving the Zustand page state for compatibility.
4. Remove the `#/redesign` gate and `preview-data.ts`; keep the scenario toolbar out of production.
5. Preserve: monitoring/optimization logic, settings validation & rollback, event subscriptions, "Unavailable, never estimated" semantics, and all Phase 1–4 functionality.
6. Clean up obsolete CSS/component code only after the new pages are verified against the real backend.
7. Run the full regression pass from the brief (§16): navigation, charts, actions, settings persistence, empty/loading/error states, keyboard access, TypeScript, lint, tests, production build, Tauri build, E2E specs — and produce `UI_INTEGRATION_REPORT.md` + `UI_REGRESSION_TEST_REPORT.md` with final screenshots.

## 6. Known limitations of this stage

- Screenshots use sample data (browser cannot run the Windows collector) — clearly marked in every capture; see the index's limitations section.
- The preview's confirm dialog is a lightweight accessible implementation; integration swaps in the Radix-based dialog (dependency already installed) with the same design.
- Skeleton loading states are implemented in the kit but only visibly trigger with real backend latency; they're demonstrated in code and can be captured during integration testing.
- 390 px mobile width is intentionally out of scope (desktop app, 1050 px minimum window); responsive verification done at 1280 and 1000 px.

## 7. Approval gate

**Nothing has been integrated.** To proceed I need your explicit approval, with any adjustments you want (colors, sidebar grouping, dashboard layout, card designs, level of technical detail, charts, navigation, naming, individual pages). Changes will be made in the isolated preview with updated screenshots first; production integration happens only after you sign off, followed by the full regression audit and the two integration reports.
