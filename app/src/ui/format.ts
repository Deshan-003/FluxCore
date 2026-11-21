/* FluxCore redesign preview — pure formatters & status logic (UI_DESIGN_SYSTEM.md §6).
 * Mirrors the semantics of the production lib/format.ts: an unavailable sensor
 * renders as "Unavailable"/"—", never as a fabricated zero. */

export type RxStatus = 'healthy' | 'normal' | 'attention' | 'critical' | 'unavailable'

export const fmtBytes = (value: number | null | undefined): string => {
  if (value === null || value === undefined) return 'Unavailable'
  const units = ['B', 'KB', 'MB', 'GB', 'TB']
  let n = value; let i = 0
  while (n >= 1024 && i < units.length - 1) { n /= 1024; i++ }
  return `${n.toFixed(i === 0 ? 0 : 1)} ${units[i]}`
}

export const fmtRate = (v: number | null | undefined) => (v == null ? 'Unavailable' : `${fmtBytes(v)}/s`)

export const pct = (v: number | null | undefined) => (v == null ? '—' : `${Math.round(v)}%`)

export const fmtTime = (t: number) => new Date(t).toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' })

export const ageOf = (t: number) => {
  const s = Math.max(0, Math.round((Date.now() - t) / 1000))
  return s < 5 ? 'just now' : s < 60 ? `${s}s ago` : `${Math.floor(s / 60)}m ago`
}

/** Documented thresholds (UI_DESIGN_SYSTEM.md §2.4): <60 normal, ≥60 attention, ≥85 critical. */
export const statusForPercent = (v: number | null | undefined): RxStatus =>
  v == null ? 'unavailable' : v >= 85 ? 'critical' : v >= 60 ? 'attention' : v >= 30 ? 'normal' : 'healthy'
