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
