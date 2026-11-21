# FluxCore UI Design System

**Version:** 1.0 (Stage-1 proposal) · **Status:** Pending user approval — implemented only in the isolated preview (`src/redesign/preview/`)
**Applies to:** every redesigned page, component and chart. This document is the contract the preview follows and the reference integration will follow.

---

## 1. Design Principles

1. **Answer first.** Each page leads with the one thing a user wants to know ("Is my system OK?", "What is using my RAM?"), then details.
2. **Explain every number.** No technical value is shown naked: description, info-tip, or caption accompanies it. Technical terms stay, but are explained.
3. **Progressive disclosure.** Overview → details → technical details. Advanced data is expandable, never deleted (advanced users lose nothing).
4. **Status is never color-only.** Icon + label + color, always; thresholds documented where a status is computed.
5. **One system.** Same card, button, table, chart, spacing and voice on every page.
6. **Honest data.** Carried over from the existing product: an unavailable sensor is shown as "Unavailable" with a reason — never estimated, never zero.

---

## 2. Color System

Dark-first. Every color is a semantic CSS variable (namespace `--rx-*`) — no hardcoded hex in components or charts. Light theme is a full curated token set, **not** a filter.

### 2.1 Dark theme (default)

| Token | Value | Usage |
|---|---|---|
| `--rx-bg` | `#0a0f16` (deep navy-black) | App background |
| `--rx-surface` | `#101823` | Cards, panels, sidebar surface |
| `--rx-surface-2` | `#151f2d` | Raised elements: inputs, table headers, hover fills |
| `--rx-surface-3` | `#1b2737` | Popovers, dialog background |
| `--rx-border` | `#1f2b3c` | Card borders, separators |
| `--rx-border-strong` | `#2c3d55` | Hover borders, focused inputs |
| `--rx-text` | `#e6edf6` | Primary text (contrast on `--rx-bg` ≈ 13.9:1) |
| `--rx-text-secondary` | `#a9b6c8` | Body/secondary text (≈ 8.3:1) |
| `--rx-text-muted` | `#7d8ba0` | Captions, axis labels (≈ 4.9:1 — AA at 12 px normal) |
| `--rx-accent` | `#4cc2e0` (cyan) | Primary actions, active nav, links, CPU |
| `--rx-accent-hover` | `#6fd0e9` | Accent hover |
| `--rx-accent-subtle` | `rgba(76,194,224,0.12)` | Active nav fill, selected row tint |
| `--rx-success` | `#46d19a` | Healthy / Normal states, confirm actions |
| `--rx-warning` | `#e8b45c` | Needs Attention state |
| `--rx-danger` | `#ef6b7a` | Critical states, destructive actions |
| `--rx-info` | `#7aa5f7` | Informational notes |
| `--rx-neutral` | `#8b97a8` | Unavailable / paused states |
| `--rx-ring` | `#4cc2e0` | `:focus-visible` outline (2 px, offset 2 px) |

### 2.2 Light theme

Full token override on `[data-theme="light"]`: bg `#f4f6fa`, surface `#ffffff`, surface-2 `#eef2f7`, border `#d8e0ea`, text `#1a2433` / secondary `#46566b` / muted `#5f7085`, accent `#0e7fa3`, success `#12855a`, warning `#a86d12`, danger `#c23448`, info `#3b64c9`, neutral `#6b7a8d`. All meet ≥ 4.5:1 against their surfaces for text use.

### 2.3 Resource identity colors (fixed mapping, dark & light)

Used identically in metric cards, charts, legends and bars:

| Resource | Dark value | Light value |
|---|---|---|
| CPU | `#4cc2e0` cyan | `#0e7fa3` |
| Memory (RAM) | `#9d8cff` violet | `#6a55c9` |
| GPU | `#e8b45c` amber | `#a86d12` |
| VRAM | `#ef8aa4` rose | `#b04767` |
| Storage | `#46d19a` teal-green | `#12855a` |
| Network/other | `#7aa5f7` blue | `#3b64c9` |

### 2.4 Status language (semantic, never color-only)

| Status | Meaning (plain language) | Icon | Color |
|---|---|---|---|
| **Healthy** | Resource has plenty of headroom; nothing to do | CheckCircle2 | success |
| **Normal** | Typical working load; no action needed | CircleCheck | success (dim) |
| **Needs Attention** | Sustained high load; worth looking at | AlertTriangle | warning |
| **Critical** | Resource saturated; performance is affected now | AlertOctagon | danger |
| **Unavailable** | Sensor/provider didn't report; never estimated | MinusCircle | neutral |

**Documented thresholds** (shown next to computed statuses, e.g., on the dashboard): Normal < 60 % · Needs Attention ≥ 60 % sustained · Critical ≥ 85 %. (Preview uses these values; final thresholds are configurable at integration and displayed with the status.)

---

## 3. Typography

CSP-safe, offline, Windows-native. No web fonts.

| Role | Stack | Size / weight / line-height | Usage |
|---|---|---|---|
| Display (page hero numbers) | `"Segoe UI Variable Display", "Segoe UI", system-ui, sans-serif` | 28–34 px / 650 / 1.1 | Dashboard hero metric, big values |
| H1 page title | same | 20 px / 650 / 1.3 | One per page |
| H2 section title | same | 15 px / 600 / 1.4 | Section headers (`h2`) |
| Body | `"Segoe UI Variable Text", "Segoe UI", system-ui, sans-serif` | **13 px / 400 / 1.55** | Default text — floor of the system |
| Secondary/caption | same | 12 px / 400 / 1.45 | Captions, axis labels, helper text — floor 12 px |
| Label (buttons, chips, table headers) | same | 12.5 px / 600 | Sentence case, never all-caps |
| Metrics & numerals | `"Cascadia Mono", "Consolas", ui-monospace` + `font-variant-numeric: tabular-nums` | sizes per role | All values, table numbers, timestamps, PIDs |
| Weights | 400 regular · 600 semibold · 650 bold | | No thin/light weights |

Letter-spacing: none except +0.02 em on small uppercase eyebrow labels (used sparingly). No all-caps table headers or body text.

---

## 4. Layout

### 4.1 App shell

```
┌──────────┬────────────────────────────────────────────────┐
│ Sidebar  │ Topbar (52 px): section · page │ status │ age │
│ 248 px   ├────────────────────────────────────────────────┤
│ (68 px   │ Page header: h1 + purpose + primary actions    │
│ collapsed)│ Content (max-width 1280 px, 24 px gutters)    │
│          │                                                │
│          ├────────────────────────────────────────────────┤
│          │ Status bar (28 px): collection note · privacy  │
└──────────┴────────────────────────────────────────────────┘
```

- Sidebar groups: **Overview · Monitoring · Optimization · Intelligence · System** (matches the 12 real pages; no invented pages).
- Sidebar collapses to a 68 px icon rail (manual toggle + auto below 992 px); active item = icon + 3 px accent bar + tinted fill + tooltip when collapsed.
- Topbar right side: system status chip (monitoring state / alert count), data-age ("Updated 5 s ago"), no ticking clock.
- Status bar carries the privacy/honesty lines ("Local collection · No cloud upload", "Unavailable sensors are never estimated").

### 4.2 Grid & spacing

- Base unit 4 px. Card padding 16 px (20 px on ≥1440). Card gap 16 px. Section gap 24 px.
- Content grids: 12-column fluid grid; metric rows `repeat(auto-fit, minmax(200px, 1fr))`; detail/technical columns collapse at 992 px.
- Breakpoints: **≥1440** full layout · **≥1200** standard · **≥992** sidebar auto-collapse + 2-col → 1-col details · **≥768** stacked cards, tables become scroll containers with sticky first column.
- Content max-width 1280 px centered for readability on large windows; charts stay fluid.

---

## 5. Components

All components are implemented in the preview under `src/redesign/preview/ui.tsx` with the classes below. States listed are mandatory: default / hover / active / focus-visible / disabled / loading.

### 5.1 Card
Surface `--rx-surface`, 1 px `--rx-border`, radius 12 px, padding 16 px. Header row: icon (16 px, muted) + title (13 px/600) + optional right-side action. Variants: **plain**, **metric** (see 5.2), **status** (left 3 px status edge), **interactive** (hover border-strong + surface-2, cursor pointer, used for navigation cards).

### 5.2 MetricCard
Icon + name + plain-language caption · hero value (28 px mono) + unit · progress bar (6 px, resource color, with track at surface-2) · status chip · sparkline (40 px, resource color, token-styled). Whole card is a button navigating to the resource page. If the value is unavailable: hero shows "—" with an Unavailable chip and a one-line reason.

### 5.3 StatusChip
`[icon + label]` pill: tinted background (status color at 14 %), status-colored icon and text, 12.5 px/600. Replaces every bare health dot in the current app.

### 5.4 ResourceBar
Label + value/total row (12.5 px, mono values) + 6 px bar in resource color; >85 % fill switches the chip to Critical; legend text explains the split when a bar has segments (e.g., used vs cached vs free).

### 5.5 Button
Radius 8 px, height 32 px (28 px compact), 12.5 px/600, focus ring always.
- **Primary**: accent bg, `#06222c`-dark text; hover accent-hover; loading spinner replaces label.
- **Secondary**: surface-2 bg, border, primary text.
- **Ghost**: transparent, secondary text; hover surface-2.
- **Danger**: danger bg, white text — reserved for destructive/confirm flows.
- **Icon button**: 32 px square ghost with mandatory `aria-label` + tooltip.

### 5.6 Switch
Radix `Switch` (dependency already present): 36×20 track, on = accent, off = surface-3/border-strong; `role="switch"` + `aria-checked` preserved; 3:1 non-text contrast on both states; each switch row has title + one-line description (SettingRow pattern).

### 5.7 Select
Radix `Select` for all dropdowns (theme, sort, interval): consistent height, check mark on selected, keyboard/AT support. Native `<select>` is not used in redesigned pages.

### 5.8 Tabs
Radix `Tabs` (dependency already present) — the **only** within-page navigation: underline-style active tab with accent bar, 13 px/600 labels, counts in a neutral pill where relevant (e.g., "Anomalies (2)"). Used by Resource Intelligence sections and GPU Acceleration areas; replaces the pill-nav and the inconsistent tab styling.

### 5.9 Dialog / ConfirmDialog
Radix `Dialog`. Confirm variant: title ("Clear finished GPU tasks?"), body explains **what happens**, **benefit** and **risk**; footer: Cancel (secondary) + Confirm (danger or primary). Used for: applying an optimization, executing a manual memory action, applying a policy override, clearing task history. Never for harmless actions (opening pages, refreshing, searching).

### 5.10 Toast
Small custom stack (no new dependency): bottom-right, icon + message + optional "Details" disclosure, auto-dismiss 6 s (errors persist until dismissed). Every completed async action reports success/failure here, in addition to inline result text where the action lives.

### 5.11 InfoTip
Inline `(? icon)` after a technical term: hover/focus opens a 280 px popover with a plain-language explanation + optional "how FluxCore gets this" line. Keyboard accessible (`button` + `aria-describedby`), native `title` set as fallback.

### 5.12 Table kit
Sentence-case headers (12.5 px/600, muted), rows 36 px, zebra-free with hover fill, numeric cells right-aligned mono, sortable headers show direction arrows, search field and per-column filters share one style. Tables live in a scroll container at <768 px (sticky header). "Advanced columns" (PID, GPU per-process) behind a disclosure toggle.

### 5.13 Charts (`ChartFrame`)
One wrapper for all Recharts usage: token-styled grid (`--rx-border` dashed), axis text 12 px `--rx-text-muted` (≥4.5:1), token tooltip (surface-3, border, 12 px, mono values), legend with text labels + resource colors, units in the Y-axis label and tooltip. Resource→color from §2.3. Prediction series render **dashed with a shaded forecast band** and an explicit "Predicted" legend entry. Loading = skeleton, not empty text.

### 5.14 Skeleton / EmptyState / ErrorState
- **Skeleton**: shimmer blocks matching the target card/chart/table shape; used while any panel is loading.
- **EmptyState**: icon + title + one-line explanation + a suggested next action (button/link) — e.g., "No GPU detected — FluxCore will show GPU metrics when Windows reports a display adapter."
- **ErrorState**: friendly one-liner ("Monitoring engine unavailable") + "Retry" primary button + collapsed "Technical details" with the raw error.

### 5.15 PageHeader
H1 + purpose sentence + right-aligned primary actions + data-age chip. Present on every page; description is page-specific (not boilerplate).

---

## 6. Voice & Content Rules

- Sentences, not labels, where explanation is needed: "Committed memory — what Windows has promised to processes (RAM + pagefile)." 
- Predictions are always labeled "Predicted/Forecast" and never rendered identically to measured data.
- Actions describe consequences before they happen ("Frees standby memory. Running apps are not interrupted.").
- Numbers: bytes via shared formatter (B → KB → MB → GB → TB, 1 decimal); percentages rounded to whole; rates as `<bytes>/s`; frequencies in MHz/GHz; temperatures °C with one decimal.
- Unavailable ≠ zero: "Unavailable" + reason string from the backend, unchanged.

---

## 7. Accessibility Contract

- WCAG 2.2 AA targets: text contrast ≥ 4.5:1 (≥3:1 for ≥18.66 px bold), non-text UI contrast ≥ 3:1, visible `:focus-visible` ring on **every** interactive element, keyboard-complete operation (tabs, switches, selects, dialogs trap/restore focus), `aria-label`s on icon buttons, status chips carry text, charts have text legends and titles (`role="img"` + `aria-label` summary), heading order `h1 → h2` per page.
- Respects `prefers-reduced-motion`: sparkline/chart animations off (already the product norm), skeleton shimmer disabled.
- Desktop-first: layouts verified at 1440×900, 1280×720, and narrow-width reflow ≥768 px.

---

## 8. What this replaces (mapping to current code)

| Current | Redesign |
|---|---|
| `App.css` filter-hack light mode | Curated `[data-theme="light"]` token set |
| 125+ hardcoded hex colors | `--rx-*` tokens only |
| `Panel` + 4 bespoke card styles | Card family (§5.1–5.2) |
| 4 button styles | Button system (§5.5) |
| `window.confirm` + double-click confirm | ConfirmDialog (§5.9) |
| Native `title` tooltips | InfoTip (§5.11) |
| Inline banner results | Toast + inline result (§5.10) |
| "Loading history" empty text | Skeletons (§5.14) |
| Pill-nav (Resource Intelligence) + shadcn Tabs (GPU) | One Tabs (§5.8) |
| Bare status dots | StatusChip (§5.3) |
| Google Fonts | Local Windows font stack (§3) |
| Duplicated `.gpu-grid`, bespoke page grids | Layout contract (§4) |
