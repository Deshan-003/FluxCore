import { useEffect, useState } from 'react'
import { Activity, BrainCircuit, ChartNoAxesCombined, History, Lightbulb, ShieldCheck, SlidersHorizontal, Zap } from 'lucide-react'
import { CartesianGrid, Legend, Line, LineChart, ResponsiveContainer, Tooltip, XAxis, YAxis } from 'recharts'
import { Detail, EmptyState, Panel, SensorNotice, SettingRow } from '../components/monitoring'
import { fmtBytes, fmtRate, titleCase } from '../lib/format'
import { getWorkloadRecommendations } from '../services/gpu'
import { applyResourcePolicy, configureResourceIntelligence, respondToResourceRecommendation, revertResourcePolicy } from '../services/intelligence'
import { useResourceStore } from '../stores/intelligence'
import type { SuitabilityAssessment } from '../types/gpu'
import type { ResourceHistory, ResourceMode, ResourcePolicy, ResourceState } from '../types/intelligence'

const sections = ['System health', 'Forecasts', 'Recommendations', 'Adaptive controls', 'Anomalies', 'History'] as const
const modes: Record<ResourceMode, string> = { analysisOnly: 'Analysis Only', smartAssist: 'Smart Assist', adaptive: 'Adaptive Optimization', manual: 'Manual Control' }
const defaultPolicy: ResourcePolicy = { mode: 'analysisOnly', paused: false, scanIntervalMs: 15000, maxScanIntervalMs: 30000, cooldownSeconds: 300, horizonMinutes: [1, 5, 15] }
const labels: Record<string, string> = { cpu: 'CPU utilization', cpuMHz: 'CPU frequency', cpuTemp: 'CPU temperature', corePeak: 'Busiest logical CPU', ram: 'Physical RAM use', ramAvailable: 'Available physical RAM', ramUsed: 'Used physical RAM', ramTotal: 'Installed physical RAM', commit: 'Commit limit used', commitBytes: 'Committed memory', pageReads: 'Physical page reads', pagefileBytes: 'Pagefile used', gpu: 'GPU utilization', vram: 'Dedicated VRAM use', vramBytes: 'Dedicated VRAM used', gpuShared: 'Shared GPU memory used', gpuTemp: 'GPU temperature', disk: 'Storage capacity used', diskFree: 'Storage free', diskRead: 'Disk read', diskWrite: 'Disk write', processCount: 'Observed process count', fluxCpu: 'FluxCore process CPU', fluxRam: 'FluxCore process memory', scansPerMinute: 'Detailed scans per minute', scanMsPerMinute: 'Scan wall time per minute' }
const units = (key: string) => key.split(':')[0]
const metricLabel = (key: string) => `${labels[units(key)] ?? key}${key.includes(':') ? ` · ${key.slice(key.indexOf(':') + 1)}` : ''}`
function reading(key: string, value: number | null | undefined) {
  if (value == null) return 'Unavailable'
  const unit = units(key)
  if (['ramAvailable', 'ramUsed', 'ramTotal', 'commitBytes', 'pagefileBytes', 'vramBytes', 'gpuShared', 'diskFree', 'fluxRam'].includes(unit)) return fmtBytes(value)
  if (unit === 'diskRead' || unit === 'diskWrite') return fmtRate(value)
  if (unit === 'cpuMHz') return `${Math.round(value)} MHz`
  if (unit.endsWith('Temp')) return `${value.toFixed(1)} °C`
  if (unit === 'pageReads') return `${value.toFixed(1)}/s`
  if (unit === 'scansPerMinute' || unit === 'processCount') return value.toFixed(1)
  if (unit === 'scanMsPerMinute') return `${value.toFixed(2)} ms/min`
  return `${value.toFixed(1)}%`
}
const localTime = (at: number) => new Date(at * 1000).toLocaleTimeString()
const localDate = (at: number) => new Date(at * 1000).toLocaleString()
const readable = (value: string) => titleCase(value.replace(/([a-z])([A-Z])/g, '$1 $2'))

function ForecastView({ data, history, metric, setMetric, horizon, setHorizon }: {
  data: ResourceState; history: ResourceHistory | null; metric: string; setMetric: (v: string) => void; horizon: number; setHorizon: (v: number) => void
}) {
  const options = Array.from(new Set(['cpu', 'ram', 'commit', ...data.forecasts.map((f) => f.metric)]))
  const forecast = data.forecasts.find((f) => f.metric === metric && f.horizonMinutes === horizon)
  const rows: { at: number; measured: number | null; estimate: number | null }[] = (history?.observations ?? []).map((row) => ({ at: row.at, measured: row.values[metric] ?? null, estimate: row.at === forecast?.observedAt ? row.values[metric] ?? null : null }))
  if (forecast?.value != null && rows.some((row) => row.at === forecast.observedAt && row.estimate != null)) {
    rows.push({ at: forecast.targetAt, measured: null, estimate: forecast.value })
    rows.sort((a, b) => a.at - b.at)
  }
  const comparisons = history?.comparisons.slice(-10) ?? []
  return <Panel title="Performance forecasts" icon={ChartNoAxesCombined}>
    <div className="intelligence-controls">
      <label>Metric <select value={metric} onChange={(e) => setMetric(e.target.value)}>{!options.includes(metric) && <option value={metric}>{metricLabel(metric)}</option>}{options.map((key) => <option key={key} value={key}>{metricLabel(key)}</option>)}</select></label>
      <label>Horizon <select value={horizon} onChange={(e) => setHorizon(Number(e.target.value))}>{data.policy.horizonMinutes.map((value) => <option key={value} value={value}>{value} min</option>)}</select></label>
    </div>
    {forecast?.value != null ? <>
      <Detail label={`Estimate at ${localTime(forecast.targetAt)}`} value={reading(metric, forecast.value)} />
      <Detail label="Indicative uncertainty range" value={`${reading(metric, forecast.low)} – ${reading(metric, forecast.high)}`} />
      <Detail label="Confidence / method" value={`${readable(forecast.confidence)} · ${readable(forecast.method)}`} />
      <p className="muted-note">{forecast.explanation}</p>
    </> : <EmptyState title="Forecast unavailable" detail={forecast?.explanation ?? 'No recent measurements exist for this metric.'} />}
    {forecast && <Detail label="Historical data quality" value={`${forecast.samples} samples · ${Math.round(forecast.observationSeconds / 60)} min · ${Math.round(forecast.coverage * 100)}% coverage`} />}
    {rows.filter((r) => r.measured != null).length > 1 && <div className="chart-wrap" aria-label="Measured history and dashed forecast"><ResponsiveContainer width="100%" height="100%"><LineChart data={rows} margin={{ top: 10, right: 22, bottom: 6, left: 8 }}>
      <CartesianGrid stroke="#344154" strokeDasharray="3 5" vertical={false} />
      <XAxis type="number" dataKey="at" domain={['dataMin', 'dataMax']} tickFormatter={localTime} minTickGap={45} />
      <YAxis tickFormatter={(value: number) => reading(metric, value)} />
      <Tooltip labelFormatter={(value) => localTime(Number(value))} formatter={(value: number, name: string) => [reading(metric, value), name]} contentStyle={{ background: '#101a29', borderColor: '#2b3749' }} />
      <Legend /><Line name="Measured" dataKey="measured" stroke="#56c7e8" strokeWidth={2} dot={false} isAnimationActive={false} />
      <Line name="Estimate" dataKey="estimate" stroke="#f4b75e" strokeDasharray="5 5" strokeWidth={2} connectNulls dot={false} isAnimationActive={false} />
    </LineChart></ResponsiveContainer></div>}
    <h3>Past estimates vs. measurements</h3>
    {comparisons.length ? <div className="process-table-wrap"><table className="process-table"><thead><tr><th>Target time</th><th>Estimate</th><th>Measured</th><th>Absolute error</th></tr></thead><tbody>
      {comparisons.map((item) => <tr key={item.targetAt}><td>{localDate(item.targetAt)}</td><td>{reading(metric, item.value)}</td><td>{reading(metric, item.actual)}</td><td>{item.actual == null || item.value == null ? '—' : reading(metric, Math.abs(item.actual - item.value))}</td></tr>)}
    </tbody></table></div> : <p className="muted-note">No eligible target-time measurements yet. Results are recorded only when local history is enabled.</p>}
  </Panel>
}

function PolicyView({ data, onChange, onManual, busy }: { data: ResourceState; onChange: (p: ResourcePolicy) => Promise<boolean>; onManual: () => void; busy: boolean }) {
  const [draft, setDraft] = useState<Partial<ResourcePolicy>>({})
  const [horizons, setHorizons] = useState<string | null>(null)
  const policy = { ...data.policy, ...draft }
