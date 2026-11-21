# FluxCore UI Redesign — Screenshot Index

**How these were produced:** real runtime captures of the isolated redesign preview (`app/src/redesign/preview/`) served by the project's own Vite dev server and rendered in a Chromium browser at the stated viewport sizes. **No AI-generated or mocked-up images** — every PNG below is a screenshot of the running redesigned UI. The preview runs on **clearly-marked sample data** (a floating `DESIGN PREVIEW · Sample data — not your PC` toolbar is visible in every shot) because a browser cannot run the Tauri Windows collector; behavior, not data, is what this preview demonstrates.

**Verified:** all 25 files exist in `artifacts/ui-redesign/` with the dimensions below and were visually inspected after capture.

| # | File | Page / state | Viewport | Main design improvements shown | Interaction details |
|---|---|---|---|---|---|
| 01 | `01-dashboard.png` | Dashboard (Normal) | 1440×900 | Hero verdict with **documented** thresholds + per-resource status chips (text+icon, never color-only); metric cards with sparklines, bars, plain-language captions; insights that say what happened & why; quick actions; expandable Technical details | Cards navigate to their page; info-tips (?) on technical terms |
| 02 | `02-cpu-monitoring.png` | CPU | 1440×900 | Hero with model + temp chip; CPU-only chart (fixes audit U-6 "wrong series"); per-core grid; top consumers; sensor-honesty tips | Frequency/temp explanations; "Unavailable never estimated" note |
| 03 | `03-ram-monitoring.png` | Memory (RAM) | 1440×900 | In-use/available hero + pressure; breakdown bars (in use / cached-reclaimable / free); **committed vs physical clearly separated** with plain-language explanation (audit U-2) | Info-tips on pressure, committed, pagefile, compression |
| 04 | `04-gpu-monitoring.png` | GPU | 1440×900 | Utilization / **dedicated VRAM / shared GPU memory as three explained cards** — VRAM explicitly "separate from system RAM" (audit U-4); history chart with GPU+VRAM legend | Technical details (clocks, power, driver) collapsed by default |
| 05 | `05-storage-monitoring.png` | Storage | 1440×900 | Drive cards with type badges, capacity bars, live read/write; provenance table ("where each number comes from") | SMART health shown as Unavailable + why |
| 06 | `06-process-monitoring.png` | Processes | 1440×900 | Friendly columns; PID & GPU columns behind a **"Show technical columns"** switch (audit U-5); search; sort control | Read-only promise stated; empty-search state |
| 07 | `07-hardware-information.png` | Hardware Info | 1440×900 | "Your PC at a glance" summary band; diagnostics runner with status chips | "Run hardware check" → toast result (preview-simulated) |
| 08 | `08-memory-management.png` | Memory Management (was Memory Intelligence) | 1440×900 | Metric tiles with plain captions; "What FluxCore found" verdict + evidence; **prediction labeled "Estimate — not a measurement"**; tabbed history (replaces 4 stacked charts, audit C-5); consumers with trend + leak badges | Actions explain what/benefit/risk before running |
| 09 | `09-gpu-acceleration.png` | GPU Acceleration · Overview | 1440×900 | Availability hero; provider cards; **VRAM budget with the "never used as system RAM" statement** | Budget explanation info-tip |
| 10 | `10-workload-intelligence.png` | GPU Acceleration · Workload Intelligence | 1440×900 | Workload recommendation cards with **recommended path + why + trade-offs** | Run buttons report via toast |
| 11 | `11-task-monitor.png` | GPU Acceleration · Task Monitor | 1440×900 | Active task with progress bar + ETA; history with Completed/Failed/Cancelled status chips | Cancel asks via confirm dialog (audit I-1) |
| 12 | `12-performance-comparison.png` | GPU Acceleration · Performance Comparison | 1440×900 | Benchmark form with busy state; token-styled bar chart; plain verdict ("GPU was 8.7× faster") | Lower-is-better caption; "How benchmarks work" disclosure |
| 13 | `13-ai-resource-intelligence.png` | Resource Intelligence · Overview | 1440×900 | Tabbed page (replaces pill-nav, audit N-2); health verdict; latest readings with one-line meanings | Tab counts (Recommendations 3, Anomalies 1) |
| 14 | `14-forecasts.png` | Resource Intelligence · Forecasts | 1440×900 | **Measured (solid) vs Predicted (dashed)** with legend + caption (audit C-3) | Predictions never rendered like measurements |
| 15 | `15-recommendations.png` | Resource Intelligence · Recommendations | 1440×900 | Cards with risk + "Requires your approval" chips, benefit lines, Apply/Dismiss | Apply opens confirm dialog |
| 16 | `16-anomaly-detection.png` | Resource Intelligence · Anomalies | 1440×900 | Alert card: severity chip, resource, when, what, **suggested next step** | Status-edge card styling |
| 17 | `17-adaptive-optimization.png` | Adaptive Optimization (was Optimization Center) | 1440×900 | Mode cards that say **what each mode is allowed to change**; activity table with statuses; safety-model card; manual actions with risk | Mode change asks via confirm dialog |
| 18 | `18-settings.png` | Settings | 1440×900 | Grouped sections; theme as toggle-group; unimplemented features honestly labeled **"Planned for a later phase"** (audit U-8); privacy card | Switches/selects with descriptions |
| 19 | `19-settings-light.png` | Settings · **Light theme** | 1440×900 | **Real curated light theme — no filter-inversion hack** (audit V-1); toast notification visible | "Theme set to 'light'" toast (audit I-2) |
| 20 | `20-confirm-dialog.png` | Memory action confirm dialog | 1440×900 | Unified ConfirmDialog with What happens / Benefit / Risk (replaces `window.confirm` + double-click patterns, audit I-1) | Esc/cancel supported; preview explicitly says nothing real changes |
| 21 | `21-dashboard-critical.png` | Dashboard · **Critical scenario** | 1440×900 | Critical hero + per-resource Critical chips; red status-edge insights with next-step links | Scenario switchable in preview toolbar |
| 22 | `22-dashboard-empty.png` | Dashboard · **No data scenario** | 1440×900 | Honest empty state with explanation + next action ("Nothing is estimated in the meantime") | — |
| 23 | `23-dashboard-1280.png` | Dashboard · smaller desktop | 1280×720 | Grid reflow (metric cards wrap; columns collapse) — fluid layout, no scrollbars/overlap | — |
| 24 | `24-cpu-narrow-1000.png` | CPU · narrow layout | 1000×768 | Single-column reflow at small desktop width; all controls remain reachable | — |
| 25 | `25-acceleration-center.png` | GPU Acceleration · Acceleration Center | 1440×900 | Execution mode toggle with consequence line; resource limits; provider priority | Advanced caps behind labeled fields |

### Preview limitations (documented honestly)

1. **Sample data, not live hardware.** A browser cannot host the Tauri collector. All values are deterministic fabricated samples; the floating preview toolbar marks this. At integration, pages swap to the real Zustand stores/Tauri services (see `UI_REDESIGN_PROPOSAL.md`).
2. **Actions are simulated.** Optimization actions, benchmarks, mode changes and settings show preview toasts ("simulated — no real system change"). No control writes to your PC.
3. **Not captured here:** loading-skeleton states (they appear only during real backend latency) and a 390 px mobile layout — FluxCore is a desktop app with a 1050 px minimum window width, so responsive targets are 1440 / 1280 / ~1000 px. Both points carry into the integration plan.
4. Chart data shape (e.g., forecast curvature) is illustrative; real chart components are identical to the ones integration will use (Recharts with token styling).

### Screenshot dimensions policy

Primary: **1440×900** (desktop, matches the audit requirement). Smaller desktop: **1280×720**. Narrow: **1000×768** — the user-requested 390 px width is not relevant for a Tauri desktop app whose window cannot go below 1050 px (`tauri.conf.json`); this decision is recorded above and in the proposal.
