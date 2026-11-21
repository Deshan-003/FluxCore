// Shared display helpers for live metrics. A sensor the collector could not
// read is rendered as an explicit placeholder — never as a fabricated zero.

export const fmtBytes = (value: number | null | undefined): string => {
  if (value === null || value === undefined) return 'Unavailable'
  const units = ['B', 'KB', 'MB', 'GB', 'TB']
  let n = value; let index = 0
  while (n >= 1024 && index < units.length - 1) { n /= 1024; index++ }
  return `${n.toFixed(index === 0 ? 0 : 1)} ${units[index]}`
}

export const fmtRate = (value: number | null | undefined): string =>
  value == null ? 'Unavailable' : `${fmtBytes(value)}/s`

export const pct = (value: number | null | undefined): string =>
  value == null ? '—' : `${Math.round(value)}%`

export type HealthStatus = 'unknown' | 'critical' | 'elevated' | 'healthy'

export const statusFor = (value: number | null | undefined): HealthStatus =>
  value == null ? 'unknown' : value >= 90 ? 'critical' : value >= 75 ? 'elevated' : 'healthy'

export const titleCase = (page: string): string =>
  page === 'Overview' ? 'System overview'
    : page === 'Hardware' ? 'Hardware information'
    : page === 'Settings' ? 'Settings & preferences'
    : page === 'Memory Intelligence' ? 'Memory intelligence'
    : page === 'Optimization Center' ? 'Optimization center'
    : `${page} monitor`

/* ---------- redesign status language (UI_DESIGN_SYSTEM.md §2.4) ---------- */

export type RxStatus = 'healthy' | 'normal' | 'attention' | 'critical' | 'unavailable'

/** Documented thresholds: <30 healthy, <60 normal, <85 needs attention, ≥85 critical. */
export const statusForPercent = (value: number | null | undefined): RxStatus =>
  value == null ? 'unavailable' : value >= 85 ? 'critical' : value >= 60 ? 'attention' : value >= 30 ? 'normal' : 'healthy'

export const fmtTime = (timestamp: string | number): string =>
  new Date(timestamp).toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' })

export const ageOf = (timestamp: string | number, now = Date.now()): string => {
  const seconds = Math.max(0, Math.round((now - new Date(timestamp).getTime()) / 1000))
  return seconds < 5 ? 'just now' : seconds < 60 ? `${seconds}s ago` : `${Math.floor(seconds / 60)}m ago`
}

export type MetricSeriesKey = 'cpuPercent' | 'memoryPercent' | 'gpuPercent' | 'vramUsedBytes'

// The VRAM tile charts dedicated GPU memory in bytes; it must never fall back
// to the system-RAM percentage series.
export const metricSeriesKey = (title: string): MetricSeriesKey =>
  title === 'CPU' ? 'cpuPercent'
    : title === 'GPU' ? 'gpuPercent'
    : title === 'VRAM' ? 'vramUsedBytes'
    : 'memoryPercent'
