/* FluxCore redesign preview — shared Recharts styling (UI_DESIGN_SYSTEM.md §5.13).
 * Token-based grid/axes so charts match the design system in dark and light. */

export const rxGrid = {
  stroke: 'var(--rx-border)',
  strokeDasharray: '3 5',
  vertical: false,
}

export const rxTick = { fill: 'var(--rx-text-muted)', fontSize: 12 }

export const rxAxis = { tickLine: false, axisLine: false }
