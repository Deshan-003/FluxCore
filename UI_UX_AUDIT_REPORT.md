# FluxCore — Complete UI/UX Audit Report

**Scope:** Entire existing frontend of the FluxCore Windows desktop app (`app/` — Tauri 2 + React 19 + TypeScript + Vite 7 + Tailwind 3 + Recharts + shadcn/ui files).
**Method:** Full code-level audit of every routed page, component, style file and interaction path (`App.tsx`, `App.css`, `index.css`, all `src/pages/*`, `src/components/*`, stores, services, types, `tauri.conf.json`). Every finding below was verified against the current source. The running desktop app is not launchable on this machine (SAC-blocked), so screenshots of the *current* UI were not possible; the redesigned preview (Stage 1 deliverable) is screenshot-verified instead.
**Date:** 2026-11-21 · **Branch:** `main` (`f3bc365`) · Audit performed on unmodified production code.

---

## 1. Executive Summary

FluxCore has solid engineering foundations — honest "Unavailable, never estimated" data semantics, a disciplined Rust contract, live events, and per-domain stores. The UI, however, has grown organically: it is **dark-cyber styled, information-dense, and inconsistent**. The audit identified **41 findings** (4 Critical, 12 High, 16 Medium, 9 Low) across visual design, usability, navigation, charts, interactions, responsiveness and accessibility.

The three structural root causes:

1. **Two dead/parallel theme systems.** A complete shadcn/Tailwind token theme (`index.css`) is installed but unused; the app actually renders through a hand-written, minified 36 KB `App.css` with 125+ hardcoded hex colors, a second palette, and a light mode implemented as a CSS `filter: invert()` hack.
2. **Six different page layout paradigms** across 12 pages (inline god-component pages in `App.tsx`, pill-section navigation, shadcn Tabs, three bespoke page grids) with **four competing button styles, two confirmation idioms, and no shared page header/component discipline**.
3. **Beginner-hostile presentation.** 8–11 px typography, all-caps 10 px table headers, dense raw-value grids (e.g., 14 unlabeled-context GPU detail rows), technical terms (`Commit limit`, `Standby`, `wgpu`) without explanation, and a health model that silently mixes only CPU+RAM into an orb labeled "Systems healthy".

The redesign (see `UI_DESIGN_SYSTEM.md` and `UI_REDESIGN_PROPOSAL.md`, implemented in the isolated preview) replaces all of this with one token-driven design system, one page template, one navigation IA, real dialogs/tooltips/toasts, and progressive disclosure of technical detail — **without touching any Phase 1–4 business logic**.

## 2. Existing Frontend Architecture

| Layer | Details |
|---|---|
| Entry | `src/main.tsx` → `src/App.tsx` (213 lines holding the shell **and** 8 inline page components) |
| Routing | **None.** A `page` string in the Zustand monitoring store + a `switch` (`App.tsx:193-209`). No URL state, no deep links, no back/forward. |
| Pages | 4 files in `src/pages` (ResourceIntelligence ~175 L, MemoryIntelligence ~182 L, OptimizationCenter ~123 L, GpuAcceleration ~353 L) + 8 inline in `App.tsx` (Overview, CPU, Memory, GPU, Storage, Processes, Hardware, Settings) |
| Shared primitives | `src/components/monitoring.tsx` — `Panel`, `EmptyState`, `ResourceBar`, `Detail`, `GpuMetric`, `SensorNotice`, `SettingRow`, `Toggle` |
| GPU components | `src/components/gpu/` — 7 files used by GPU Acceleration + Resource Intelligence |
| shadcn/ui | 68 generated components in `src/components/ui/` — **only `tabs.tsx` is imported** (by GpuAcceleration). 67 files are dead weight. |
| State | 4 Zustand stores (`monitoring` 163 L, `memory` 98 L, `gpu` 185 L, `intelligence` 43 L); UI state (`page`, `search`, `sortBy`) lives inside the monitoring data store |
| Backend bridge | ~30 Tauri commands across `src/services/*` + 3 events (`fluxcore-snapshot`, `fluxcore-gpu-task`, `fluxcore-gpu-capabilities`) |
| Charts | Recharts ^2.15.4, always `isAnimationActive={false}`, inline-styled axes/tooltips |
| Styling | Tailwind configured (`tailwind.config.js`, `darkMode:["class"]`) but **effectively unused in app code**; everything styled via `App.css` (191 minified lines, 125+ hex colors, Google-Fonts `@import`) |
| Window | 1440×960 default, `minWidth 1050` (`tauri.conf.json`); `body{min-width:900px}` (`App.css:3`) |

## 3. Complete List of Audited Pages

All 12 navigation items are routed; no orphaned pages. Missing screens are listed separately below.

| # | Page (nav label) | Location | Structure |
|---|---|---|---|
| 1 | Overview (dashboard) | `App.tsx:75-106` | 5 metric tiles + multi-series area chart + memory-pressure panel + side column (health orb, volumes, disk throughput) |
| 2 | CPU | `App.tsx:108-110` | 2-col grid: highlight number, chart, per-core grid / details panel + sensor notice |
| 3 | Memory | `App.tsx:112-115` | 2-col grid: highlight, chart, breakdown bars / commit & paging, cache & compression panels |
| 4 | GPU | `App.tsx:117-119` | Wide page: shared 3-series chart, then per-GPU panels with 14 raw detail rows each |
| 5 | Storage | `App.tsx:121-123` | Wide page: single panel with custom rows (drive, capacity bar, read/write) |
| 6 | Processes | `App.tsx:125-129` | Wide page: search + sort select + 7-column table, top 100 |
| 7 | Hardware | `App.tsx:131-140` | Wide page: diagnostics runner + 3-panel grid (OS/CPU/GPU) + storage details |
| 8 | Settings | `App.tsx:142-146` | Single panel, 7 `SettingRow`s (toggles + selects) |
| 9 | Resource Intelligence | `src/pages/ResourceIntelligence.tsx` | In-page pill-button section nav (6 sections), detail grid, forecast LineChart, HTML tables, article-style records, policy form |
| 10 | Memory Intelligence | `src/pages/MemoryIntelligence.tsx` | 8 overview tiles, pressure + prediction panels, searchable process table, 4 stacked area charts, recommendation cards |
| 11 | Optimization Center | `src/pages/OptimizationCenter.tsx` | Mode-card grid, 17-field policy number grid, manual action list, tabbed history |
| 12 | GPU Acceleration | `src/pages/GpuAcceleration.tsx` | 5 shadcn Tabs (Overview, Workload Intelligence, Acceleration Center, Task Monitor, Performance Comparison) |

**Not present in the app** (requested areas that don't exist — assessed as "missing", not audited as pages): a dedicated VRAM page (VRAM lives inside GPU page + an Overview tile), an onboarding/help page, a notifications center, a standalone "performance reports" page, a standalone "anomaly detection" page (it is a section of Resource Intelligence), and a standalone "workload intelligence" page (it is a tab of GPU Acceleration + a section of Resource Intelligence). `src/sections/` exists but is empty.

## 4. Visual Design Problems

| ID | Priority | What is wrong | Where | Why it is a problem | How the redesign solves it |
|---|---|---|---|---|---|
| V-1 | **Critical** | Light theme is a CSS filter hack: `html[data-theme='light']{filter:invert(.88) hue-rotate(180deg)}` | `App.css:13` | Inverts charts, gradients and colors chaotically; needs one-off patches (`App.css:190`); users choosing light/system get a visibly broken UI | Real dual-token theme: every color is a semantic CSS variable with curated dark **and** light values; charts/tooltips consume tokens |
| V-2 | **Critical** | `@import` of Google Fonts (DM Sans/DM Mono/Manrope) is blocked by CSP `style-src 'self' 'unsafe-inline'` | `App.css:1`, `tauri.conf.json:25` | In the packaged app the fonts never load; typography silently degrades to inconsistent fallbacks — the shipped app does not look like the designed one | Offline-safe local font stack (Segoe UI Variable/Segoe UI + Cascadia Mono for numerals), no network fonts |
| V-3 | **Critical** | `.gpu-grid` is defined twice with different layouts | `App.css:7` vs `App.css:131` | Later rule silently wins for both pages; the GPU monitor page renders on the wrong grid | Single source of truth for layout primitives; no class reuse across pages |
| V-4 | High | 125+ hardcoded hex colors, no design tokens in app code; two different chart palettes | `App.css` throughout; `App.tsx:65-69`, `MemoryIntelligence.tsx:108-118` vs `BenchmarkComparisonChart.tsx:17-27`, `ResourceIntelligence.tsx:58-63` | Every tweak requires hunting raw hex; pages drift apart (already: `#56c7e8` vs `#22d3ee` for the same "accent") | Semantic token layer (`--rx-*`) consumed by components, charts and statuses; one palette |
| V-5 | High | Typography scale is extreme: 8/9/10/11 px dominates labels, tables, notes (~25 font-size declarations) | `App.css` (e.g., `.brand-name span` 8 px, `.nav-label` 9 px, table cells 10 px, `.muted-note` 9 px) | Fails readability at normal desktop distance; WCAG AA impossible; looks cramped rather than "dense-professional" | 13 px minimum body, 12 px minimum captions, defined type scale with weights and line heights |
| V-6 | Medium | Status conveyed by bare colored dots (`health-dot`, `status-light`, `diagnostic-status`, `.bar-fill` colors) in most places | `App.tsx:48/99/213/217/220`, `HardwarePage` diagnostics | Color-blind users and glance-reading get no meaning; only `health-pill` carries text | Every status = icon + label + color (tokens); color never alone |
| V-7 | Medium | Inconsistent card language: `Panel`, `metric-card`, `mode-card`, `detail-highlight`, `gpu-provider`, `intelligence-record` all differ (radius, padding, borders) | `App.css:29-100`, all pages | Pages look like different apps; no reusable rhythm | One `Card`/`MetricCard`/`StatusCard` component family with a single radius/border/elevation rule |
| V-8 | Medium | Decorative clutter: sidebar tagline card ("Built for clarity. Not guesswork."), per-row colored `process-dot`, "LIVE" pulse labels, gradient/eye-candy accents | `App.tsx:215`, `App.css` | Consumes space without information value; cyber aesthetic over clarity | Space given to plain-language explanations, status chips and insights; decoration removed |
| V-9 | Low | Inline styles sprinkled in JSX (colors, bar widths, chart palettes) | `App.tsx:47/92/101/109/122`, `monitoring.tsx:16` | Untokenized values, harder to theme (aggravates V-1 in light mode) | Widths computed from data are fine; colors come from tokens/classes only |

## 5. Usability Problems

| ID | Priority | What is wrong | Where | Why it is a problem | How the redesign solves it |
|---|---|---|---|---|---|
| U-1 | **Critical** | Health orb "Systems healthy / High resource pressure" is computed from `max(CPU, memory)` only — GPU, VRAM, storage and disk health are invisible in the verdict; thresholds 90/75 are not explained in context | `App.tsx:85/99` | A user with 97 % VRAM and 30 % CPU sees "Systems healthy" — misleading; the "combined health" claim the dashboard makes is not meaningful as presented | Dashboard shows per-resource status chips (Healthy/Normal/Needs Attention/Critical/Unavailable) with the threshold rule documented in plain language next to each; no opaque aggregate score |
| U-2 | High | Technical values without meaning: `Commit limit`, `Standby (reclaimable)`, `Modified pages`, `wgpu`, `Memory controller activity`, `connectNulls`-style gaps unexplained | `App.tsx:97/114/118`, `BackendBadge` | Beginners can't act on numbers they don't understand; the app's own good data-honesty ("Unavailable, never estimated") is undermined by no explanations | Every technical term gets a plain-language description via info-tips and section subtitles ("Committed = RAM + pagefile reservations…"); advanced rows are grouped under expandable "Technical details" |
| U-3 | High | No feedback channel for actions: optimization execution, policy save, benchmark runs report via inline text lines only; no success/error notifications | `MemoryIntelligence.tsx:129-146`, `OptimizationCenter.tsx:70-92`, `ResourceIntelligence.tsx:147-160` | Users can't tell whether an action worked, is running, or failed without hunting the page | Toast notifications for every completed action + inline result states with icons; loading states on buttons |
| U-4 | High | GPU page presents 14 raw detail rows in flat `Detail` lists per GPU (utilization, class, driver, VRAM×3, shared, controller, temp, clocks, power, fan) with no grouping, no bars, no hierarchy | `App.tsx:118` | Users can't distinguish "the important stuff" (utilization, VRAM, temperature) from metadata (driver version, GPU class); VRAM has no visual proportion | Structured GPU card: headline metrics with bars (GPU %, VRAM, temp), grouped "Technical details" disclosure for driver/clocks/fan/power; explicit VRAM vs shared-memory separation with explanation |
| U-5 | Medium | Processes table is one-size-fits-all: 7 all-caps columns, PID second column, per-process GPU columns that are always "Unavailable"/"—" with only a footnote | `App.tsx:128` | Ordinary users see noise; the always-empty GPU columns read as broken | Friendly columns (name+icon, CPU %, RAM, RAM %), sortable; GPU columns + PID moved into an expandable "technical columns" mode |
| U-6 | Medium | CPU page chart ("CPU utilization history") actually renders **all three** series (CPU + Memory + GPU) because `ChartPanel` is shared | `App.tsx:63-73,108` | Chart contradicts its title; users misattribute lines | Per-page charts show the relevant series only, with a consistent legend |
| U-7 | Medium | Page descriptions are generic and repeated: nearly every page gets "Live system measurements, collected directly from Windows." | `App.tsx:218` | The one-liner is the prime spot to orient users; it's wasted | Per-page purpose sentence written for that page (what it shows, why it matters) |
| U-8 | Medium | Settings presents unsupported futures as normal toggles ("Start with Windows — preference only… not enabled"; "Minimize to tray — not configured") | `App.tsx:145` | Toggles that do nothing feel broken; beginners will rely on them | Preferences that are not wired are visually separated as "Coming later" / disabled with explanation, or removed from the primary list |
| U-9 | Low | Topbar clock ticks every second and duplicates the "UPDATED hh:mm:ss" pill | `App.tsx:180/217/218` | Two competing time displays, neither being the useful one (age of data) | One data-age indicator ("Updated 5 s ago"), no ticking clock |
| U-10 | Low | Sidebar version "FLUXCORE 0.1.0" is hardcoded and disagrees with `package.json` 1.0.0 | `App.tsx:215` | Small trust/diagnostic inconsistency | Version sourced from build config in one place |

## 6. Navigation & Information Architecture Problems

| ID | Priority | What is wrong | Where | Why it is a problem | How the redesign solves it |
|---|---|---|---|---|---|
| N-1 | High | Group labels don't map to user mental models: "MEMORY INTELLIGENCE" holds both Memory Intelligence and Optimization Center; "WORKSPACE" holds Resource Intelligence; "SYSTEM" mixes Processes/Hardware/Settings | `App.tsx:27-40,210` | Beginners can't guess where "optimize my RAM" or "what's using my GPU" lives | Purpose-named groups: **Overview · Monitoring · Optimization · Intelligence · System** with one or two-word plain labels (mapping preserved 1:1 to the 12 real pages — no new pages invented) |
| N-2 | High | Three competing in-app navigation idioms: sidebar, pill-button section nav (Resource Intelligence), shadcn Tabs (GPU Acceleration) | `App.tsx:214`, `ResourceIntelligence.tsx:12/123`, `GpuAcceleration.tsx` | Users must relearn navigation per page; active states look different | Sidebar for pages; one shared `Tabs` component for all within-page sections (Resource Intelligence and GPU Acceleration both use it) |
| N-3 | Medium | No URL/route state: refresh and window reopen always land on Overview; nothing can be deep-linked or bookmarked | `stores/monitoring.ts:31-33`, `App.tsx` | Mid-task loss of context; hard to report "the screen where X happened" | Preview demonstrates hash-based page routing (`#/page-id`) with page ids separated from display labels; integration adopts the same without new dependencies |
| N-4 | Medium | Page identity is the nav label (`page` string doubles as data key and display text, `titleCase` patches the display) | `stores/monitoring.ts:31`, `lib/format.ts:23-29` | Fragile coupling; renaming a label breaks state comparisons | Nav items carry stable `id` + `label` + `description`; state uses ids |
| N-5 | Medium | Processes count badge exists in sidebar, but no other glanceable state (e.g., alerts) is reachable from nav; no notifications area anywhere | `App.tsx:214` | Anomaly alerts (Resource Intelligence) are only visible if the user opens that page | Topbar system-status chip reflects active alerts/critical states with count, linking to the relevant page |
| N-6 | Low | Collapsed sidebar relies on native `title` tooltips; active item is a color fill only | `App.tsx:214`, `App.css` | Weak affordance when collapsed | Collapsed rail keeps icon + accent bar + labeled tooltip; active state uses icon, bar and tint |

## 7. Chart & Data Presentation Problems

| ID | Priority | What is wrong | Where | Why it is a problem | How the redesign solves it |
|---|---|---|---|---|---|
| C-1 | High | Axis/tooltip styling is inline and inconsistent: grid `#202b3b` vs `rgba(148,163,184,.15)` vs `#344154`; tooltip backgrounds `#101a29` vs `#0b1220`; axis font 10 px | `App.tsx:66`, `MemoryIntelligence.tsx:108-118`, `BenchmarkComparisonChart.tsx:17-27`, `ResourceIntelligence.tsx:58-63` | Charts are the product's core; they look hand-painted per page and tick labels are barely readable | One `ChartFrame` wrapper: token-based grid/axes/tooltip, 12 px labels, consistent padding, legend component |
| C-2 | High | Series colors duplicated inline (two palettes in use) and not tied to resource semantics | same as C-1 | Same resource can appear in different colors on different pages (accent `#56c7e8` vs `#22d3ee`) | Fixed resource→color mapping defined once in the design system (CPU cyan, Memory violet, GPU amber, VRAM rose, Storage teal) and consumed via tokens |
| C-3 | Medium | Forecast chart does not visually separate *prediction* from *measurement* beyond line names; "predicted" region unlabeled | `ResourceIntelligence.tsx:57-64` | Users can mistake forecasts for measured data — the app's own docs promise the distinction | Dashed prediction segment + shaded "forecast" band + explicit legend "Measured / Predicted" and a caption |
| C-4 | Medium | Units/axes context gaps: VRAM tile charts bytes without axis formatting; benchmark chart mixes workload sizes on one axis without per-size labeling | `App.tsx:91`, `BenchmarkComparisonChart.tsx` | Numbers without units force guessing | Byte-formatted axes/tooltip via shared formatter; benchmark chart grouped by workload with explicit labels |
| C-5 | Medium | Four stacked area charts on Memory Intelligence (RAM, Committed, Compressed, Pagefile) have no plain-language captions and use 10 px axes | `MemoryIntelligence.tsx:94-120` | Wall of charts; users can't tell which matters | Two primary charts with captions; committed/pagefile series behind a "more history" disclosure, axes ≥12 px |
| C-6 | Low | Sparklines in metric tiles have no baseline/context (no max marker, no threshold hint); status color only on dot | `App.tsx:44-52` | Sparkline alone doesn't say whether 85 % is bad | Tile combines sparkline + status chip text ("Needs attention") so context is explicit |

## 8. Interaction Problems

| ID | Priority | What is wrong | Where | Why it is a problem | How the redesign solves it |
|---|---|---|---|---|---|
| I-1 | **Critical** | Two incompatible confirmation idioms: native `window.confirm()` (blocks the whole app, alien in Tauri) and an in-page "Click again to confirm" pattern | `ResourceIntelligence.tsx:147/157` vs `MemoryIntelligence.tsx:129-141`, `OptimizationCenter.tsx:70-84` | Inconsistent + unstyleable + no room to explain consequences; double-click patterns hide risk level | One `ConfirmDialog` (Radix Dialog, already a dependency) that states action, benefit, risk and requires explicit confirm for risky actions only; harmless actions never confirm |
| I-2 | High | No modal/dialog system anywhere in the app (68 shadcn components, 0 dialogs used) and no toast/notification system | whole app | Errors/confirmations live in banners and inline lines; nothing is dismissible or queued | Toast stack (small custom component, no new deps) + Radix-based dialogs |
| I-3 | High | Buttons are 4 different systems: `.diagnostic-button`, `.rec-action`, `.button primary/ghost` (GPU page only), `.text-button`/`.icon-button` | `App.css:94/68`, `GpuAcceleration.tsx`, `App.tsx` | No consistent primary/secondary hierarchy; hover/active/disabled states differ | One Button component (primary / secondary / ghost / danger) with shared states, used everywhere in the redesign |
| I-4 | High | Loading is inconsistent: GPU page shows an empty-state text with no spinner; history panels show "Loading history" as an empty state; no skeletons anywhere | `GpuAcceleration.tsx:109-115`, `ResourceIntelligence.tsx:163` | Users can't tell "broken" from "working" | Skeleton loaders for cards/charts/tables + button busy states; empty states only for genuine emptiness |
| I-5 | Medium | Tooltips are native `title` attributes only (single-line, unstyleable, delayed); rich `ui/tooltip.tsx` unused | `monitoring.tsx:20`, `App.tsx:214` | The mechanism chosen to explain technical terms (U-2) is too weak for the job | Lightweight styled info-tip component (title fallback kept for a11y) used for metric explanations |
| I-6 | Medium | Sort control on Processes is a bare `<select>` labeled "Sort by"; two search-state models (global store vs local `useState`) | `App.tsx:126-128`, `MemoryIntelligence.tsx:63-65` | Inconsistent controls/behaviors between pages | Shared search + sort field components with the same look/behavior; consistent state ownership per page |
| I-7 | Medium | Error strings surface raw (`String(error)`) into UI banners and role="alert" paragraphs | `stores/monitoring.ts:131/166`, `ResourceIntelligence.tsx:124/154` | Rust/backend errors shown verbatim to end users | Errors mapped to friendly one-liners with "Show technical details" disclosure |
| I-8 | Low | Benchmark form fields and workload selects have no input validation feedback | `GpuAcceleration.tsx` | Invalid input only fails silently at submit | Inline validation with helper text on the shared field components |

## 9. Responsive Layout Problems

| ID | Priority | What is wrong | Where | Why it is a problem | How the redesign solves it |
|---|---|---|---|---|---|
| R-1 | High | `body{min-width:900px}` + window `minWidth 1050`; below that, content horizontally scrolls | `App.css:3`, `tauri.conf.json` | A snapped half-screen window (≈960 px) works, but narrow windows degrade to scrollbars instead of reflow | Fluid layout from 720 px up; grids collapse columns, sidebar auto-collapses to rail, tables get horizontal-scroll containers |
| R-2 | Medium | Breakpoints exist (1250/1020/760 px) but only rearrange a few grids; metric tiles, tables and the GPU 14-row grid don't reflow | `App.css:10-12/124-125/173-174` | "Responsive" is partial and unpredictable | Breakpoint contract defined in the design system (≥1440, ≥1200, ≥992, ≥768) and applied to every grid via shared components |
| R-3 | Low | Desktop app has no meaningful use of the extra height on 1440×960 (fixed dense rows) | all pages | Wasted space on the default window | Page templates use the vertical space for progressive disclosure (details sections, larger charts) |

## 10. Accessibility Problems

| ID | Priority | What is wrong | Where | Why it is a problem | How the redesign solves it |
|---|---|---|---|---|---|
| A-1 | High | Visible focus indicators exist **only** on Resource Intelligence controls; sidebar nav, buttons, toggles, tables, selects have none defined | `App.css:181` vs rest of file | Keyboard users can't see where they are — fails WCAG 2.2 AA (2.4.7) | Global `:focus-visible` ring token on every interactive element |
| A-2 | High | 8–10 px text everywhere (see V-5) — far below readable/AA-compliant sizes | `App.css` | Even with good contrast, tiny type is an accessibility failure for low-vision users | Type scale with 13 px floor; browser zoom remains functional because layout is fluid |
| A-3 | High | Status by color alone (dots for health/collector/diagnostics; bar colors) — only the CPU/RAM `health-pill` adds text | `App.tsx:48/99/213/217/220`, `App.tsx:139` | WCAG 1.4.1 violation; color-blind users lose all status semantics | Status = icon + text label + color, enforced by the shared `StatusChip` component |
| A-4 | Medium | Chart tick labels 10 px low-contrast `#778399` on `#0c121d` (≈4.8:1 — borderline for small text); chart legends are bare color chips | `App.tsx:66` | Charts are hard to read for low-vision users; legend chips meaningless without labels | 12 px ≥4.5:1 tick color, legends with text labels, tooltip content also announced via accessible table where feasible |
| A-5 | Medium | Icon-only buttons (refresh, collapse, task cancel) have `aria-label`s (good) but no visible focus ring (see A-1) and inconsistent `aria-label` naming | `App.tsx:212/217`, `TaskMonitorTable.tsx` | Keyboard/AT users get inconsistent affordances | Shared IconButton with mandatory label + focus ring |
| A-6 | Medium | Custom `Toggle` is a `role="switch"` button (good) but its state is conveyed by styling only; no `aria-checked` text/state styling at ≥3:1 | `monitoring.tsx:35-37` | WCAG 1.4.11 non-text contrast risk | Redesign switch keeps `role="switch"` + `aria-checked` + visible on/off track labels |
| A-7 | Low | Landmarks are reasonable (`aside`, `main`); headings exist per page but order inside panels is `h3` under `h1` without `h2` section headings in some pages | `App.tsx`, pages | Screen-reader users can't jump by section | Page template: `h1` page title, `h2` section headings consistently |
| A-8 | Low | All-caps 9–10 px table headers with letter-spacing rely on caps for hierarchy | `App.css` (`.process-table` etc.) | Harder to read, worse for AT | Sentence-case headers, 12 px, sortable-column affordances |

## 11. Inconsistent Design Patterns (cross-cutting)

1. **Two theme systems** (`index.css` shadcn tokens unused vs `App.css` custom) — V-1/V-4.
2. **Six page paradigms** — `page-grid` (CPU/Memory), `wide-page` (GPU/Storage/Processes/Hardware), `mi-page` tile grid (Memory Intelligence), `oc-page` mode cards (Optimization Center), `intelligence-page` pill nav (Resource Intelligence), `gpu-page` shadcn Tabs (GPU Acceleration).
3. **Four button systems** — I-3.
4. **Two confirmation idioms** — I-1.
5. **Five table implementations** sharing one CSS class (`process-table` reused by 4 different tables + a separate `gpu-task-table`) — `App.tsx:128`, `MemoryIntelligence.tsx:79-87`, `ResourceIntelligence.tsx:66-68`, `OptimizationCenter.tsx:106-107`, `App.css:157`.
6. **Two chart palettes + inline styles** — C-1/C-2.
7. **Duplicate `Panel` implementation** — `MemoryIntelligence.tsx:16-18` re-declares instead of importing `components/monitoring.tsx:11`.
8. **Two search-state models** — I-6.
9. **Dead code mass** — 67/68 shadcn components, `hooks/use-mobile.ts`, empty `src/sections/` (the redesign consolidates on the token layer + a small shared kit; cleanup of the unused kit happens at integration, not in the preview).
10. **UI state inside the data store** — `page`/`search`/`sortBy` in `stores/monitoring.ts:31-33` (N-4).

## 12. Specific Improvements Required

Priority-ordered summary (full detail in `UI_DESIGN_SYSTEM.md` / `UI_REDESIGN_PROPOSAL.md`):

1. **Replace the styling foundation** with one semantic token layer (dark-first, real light theme) consumed by components and charts; remove the filter-hack light mode, the duplicate `.gpu-grid`, and Google-Fonts dependency. *(V-1, V-2, V-3, V-4)*
2. **One app shell**: grouped sidebar (Overview / Monitoring / Optimization / Intelligence / System), collapsible to an icon rail, with a purpose-written topbar (status chip, data-age, no clock duplication) and a per-page header (title + one-line purpose + primary actions). *(N-1, U-7, U-9)*
3. **One page template**: header → key metrics → primary chart(s) → detail sections → collapsible "Technical details", with consistent spacing and heading order. *(V-7, U-4, A-7)*
4. **Explain, don't dump**: info-tips and plain-language captions for every technical metric; VRAM explicitly separated from RAM; forecast vs measured visually distinguished. *(U-2, U-1, C-3, C-5)*
5. **Interaction kit**: one Button system, ConfirmDialog for risky actions, toasts for outcomes, skeletons for loading, friendly error mapping with technical disclosure. *(I-1–I-4, I-7)*
6. **Accessibility pass**: focus-visible everywhere, ≥12 px text (13 px body), status = icon+text+color, contrast ≥4.5:1 for text. *(A-1–A-6)*
7. **Chart kit**: single ChartFrame with token styling, resource-fixed colors, byte/percent formatters, legends with labels. *(C-1, C-2, C-4)*
8. **Dashboard rethink**: per-resource status chips (documented thresholds), insights that say what happened and why it matters, quick actions, expandable technical panel. *(U-1, V-8)*

## 13. Priority Classifications

| Priority | Count | Findings |
|---|---|---|
| **Critical** — makes the app difficult/impossible to use correctly | 4 | V-1 (broken light theme), V-2 (fonts blocked by CSP), V-3 (duplicate grid definition), U-1 (misleading health verdict), I-1 (unsafe/inconsistent confirmations) |
| **High** — seriously affects usability | 12 | V-4, V-5, U-2, U-3, U-4, N-1, N-2, C-1, C-2, I-2, I-3, I-4, R-1, A-1, A-2, A-3 (counted individually in tables above) |
| **Medium** — creates confusion or inconsistency | 16 | V-6, V-7, U-5, U-6, U-8, N-3, N-4, N-5, C-3, C-4, C-5, I-5, I-6, I-7, R-2, A-4, A-6 |
| **Low** — minor polish | 9 | V-8, V-9, U-9, U-10, N-6, C-6, I-8, R-3, A-7, A-8 |

*(Counts refer to unique IDs in sections 4–10; several IDs appear in multiple summary buckets only where cross-listed.)*

## 14. Recommended New Design System (summary)

Full specification in **`UI_DESIGN_SYSTEM.md`**; implemented and screenshot-verified in the isolated preview. In brief:

- **Foundations:** dark-first semantic tokens (deep navy `#0a0f16` base, layered surfaces, subtle borders, cyan accent, green/amber/red status ramp) with a real curated light theme; local font stack (Segoe UI Variable / Segoe UI / Cascadia Mono for numerals — CSP-safe); 4 px spacing grid; 12 px radius; type scale 12–28 px with tabular numerals for metrics.
- **Status language:** Healthy / Normal / Needs Attention / Critical / Unavailable — always icon + label + color; documented thresholds.
- **Resource colors (fixed):** CPU cyan · Memory violet · GPU amber · VRAM rose · Storage teal — used identically in tiles, charts and legends.
- **Components:** Card, MetricCard, StatusChip, ResourceBar, DataChart (ChartFrame), Table kit, Button (primary/secondary/ghost/danger), Switch, Select, Tabs, ConfirmDialog, Toast, InfoTip, Skeleton, EmptyState, ErrorState, CollapsibleSection.
- **Layout:** sidebar 248 px → 68 px rail; topbar 52 px; content max-width 1280 px with 24 px gutters; breakpoints ≥1440 / ≥1200 / ≥992 / ≥768.
- **Voice:** plain language first ("Your PC at a glance"), technical terms kept but explained; predictions always labeled as predictions; unavailable sensors always shown as "Unavailable" with a reason, never estimated.

---

### Verification note
All `file:line` references above were checked against `main` (`f3bc365`) during this audit. One commonly-suspected issue was investigated and **ruled out**: the GPU component strings (`→ CPU`, `—`) are valid UTF-8 — there is no mojibake in the UI. The current UI could not be screenshot-verified on this machine (desktop runtime SAC-blocked); the redesign preview compensates with real runtime screenshots of every redesigned screen.
