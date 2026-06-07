import { useEffect, useMemo, useState } from 'react'
import {
  Activity, AlertTriangle, ArrowDownRight, ArrowUpRight, BrainCircuit, CheckCircle2, CircleSlash,
  Clock, Cpu, Database, Gauge, MemoryStick, Minus, Search, ShieldCheck, Sparkles,
} from 'lucide-react'
import { Area, AreaChart, CartesianGrid, ResponsiveContainer, Tooltip, XAxis, YAxis } from 'recharts'
import { useMemoryStore } from '../stores/memory'
import { useMonitoringStore } from '../stores/monitoring'
import { fmtBytes } from '../lib/format'
import type { PressureLevel, Recommendation } from '../types/memory'

const gb = (value: number | null | undefined) => value == null ? null : Number((value / 1024 ** 3).toFixed(2))
const mbPerMin = (value: number) => `${value >= 0 ? '+' : ''}${(value / 1024 ** 2).toFixed(1)} MB/min`
const levelClass = (level: PressureLevel) => level === 'normal' ? 'healthy' : level

function Panel({ title, icon: Icon, action, children }: { title: string; icon: React.ElementType; action?: React.ReactNode; children: React.ReactNode }) {
  return <section className="panel"><div className="panel-head"><div className="panel-title"><Icon size={16} /><h2>{title}</h2></div>{action}</div>{children}</section>
}

function OverviewTile({ label, value, sub, tone = '' }: { label: string; value: string; sub: string; tone?: string }) {
  return <div className={`mi-tile ${tone}`}><span className="mi-tile-label">{label}</span><strong>{value}</strong><small>{sub}</small></div>
}

function PressureIndicator() {
  const intelligence = useMemoryStore((state) => state.intelligence)
  if (!intelligence) return null
  const { pressure } = intelligence
  return <Panel title="Memory pressure" icon={Gauge} action={<span className={`pressure-badge ${levelClass(pressure.level)}`}>{pressure.level}</span>}>
    <div className="pressure-reasons">
      {pressure.reasons.map((reason, index) => <div className="pressure-reason" key={index}><CheckCircle2 size={13} /><span>{reason}</span></div>)}
    </div>
    <div className="pressure-signals">
      <span>Available <b>{pressure.signals.availablePercent.toFixed(0)}%</b></span>
      <span>Commit <b>{pressure.signals.commitPercent == null ? '—' : `${pressure.signals.commitPercent.toFixed(0)}%`}</b></span>
      <span>Pagefile <b>{pressure.signals.pagefilePercent == null ? '—' : `${pressure.signals.pagefilePercent.toFixed(0)}%`}</b></span>
      <span>Hard faults <b>{pressure.signals.hardFaultsPerSec == null ? '—' : `${pressure.signals.hardFaultsPerSec.toFixed(0)}/s`}</b></span>
      <span>Avail. trend <b>{mbPerMin(pressure.signals.availableTrendBytesPerMin)}</b></span>
      <span className="pressure-since"><Clock size={12} />since {new Date(pressure.levelSince).toLocaleTimeString()}</span>
    </div>
  </Panel>
}

function PredictionCard() {
  const intelligence = useMemoryStore((state) => state.intelligence)
  if (!intelligence) return null
  const prediction = intelligence.prediction
  return <Panel title="Memory pressure forecast" icon={Sparkles} action={<span className="chart-period">{Math.round(prediction.confidence * 100)}% confidence · {prediction.observationMinutes.toFixed(0)} min observed</span>}>
    <p className="prediction-summary">{prediction.summary}</p>
    <div className="pressure-signals">
      <span>Horizon <b>{prediction.horizonMinutes} min</b></span>
      <span>Available slope <b>{mbPerMin(prediction.availableForecast.slopeBytesPerMin)}</b></span>
      <span>Projected available <b>{fmtBytes(prediction.availableForecast.projectedBytes == null ? null : Math.max(0, prediction.availableForecast.projectedBytes))}</b></span>
      <span>Commit slope <b>{mbPerMin(prediction.commitForecast.slopeBytesPerMin)}</b></span>
      <span>Time to high pressure <b>{prediction.timeToHighPressureMinutes == null ? 'not projected' : `${prediction.timeToHighPressureMinutes.toFixed(0)} min`}</b></span>
    </div>
    {prediction.growingProcesses.length > 0 && <div className="growth-list">{prediction.growingProcesses.slice(0, 4).map((process) => <div className="growth-row" key={process.pid}><ArrowUpRight size={13} /><span>{process.name} · PID {process.pid}</span><b>{mbPerMin(process.growthBytesPerMin)}</b><small>{Math.round(process.confidence * 100)}%</small></div>)}</div>}
    <p className="muted-note">{prediction.disclaimer}</p>
  </Panel>
}

function ProcessRanking() {
  const intelligence = useMemoryStore((state) => state.intelligence)
  const [search, setSearch] = useState('')
  const [sortBy, setSortBy] = useState<'private' | 'working' | 'trend'>('private')
  const [filter, setFilter] = useState<'all' | 'rising' | 'leak'>('all')
  const rows = useMemo(() => {
    if (!intelligence) return []
    return intelligence.processes
      .filter((process) => process.name.toLowerCase().includes(search.toLowerCase()) || String(process.pid).includes(search))
      .filter((process) => filter === 'all' ? true : filter === 'rising' ? process.trendDirection === 'rising' : process.leak !== null)
      .sort((a, b) => sortBy === 'private' ? b.privateBytes - a.privateBytes : sortBy === 'working' ? b.workingSetBytes - a.workingSetBytes : b.trendBytesPerMin - a.trendBytesPerMin)
      .slice(0, 60)
  }, [intelligence, search, sortBy, filter])
  return <Panel title={`Application memory ranking · ${intelligence?.processes.length ?? 0} analyzed`} icon={Cpu} action={<label className="search-box"><Search size={15} /><input value={search} onChange={(event) => setSearch(event.target.value)} placeholder="Search name or PID" /></label>}>
    <div className="process-controls">
      <label>Filter <select value={filter} onChange={(event) => setFilter(event.target.value as 'all' | 'rising' | 'leak')}><option value="all">All processes</option><option value="rising">Rising memory</option><option value="leak">Suspected growth pattern</option></select></label>
      <label>Sort by <select value={sortBy} onChange={(event) => setSortBy(event.target.value as 'private' | 'working' | 'trend')}><option value="private">Private bytes</option><option value="working">Working set</option><option value="trend">Growth rate</option></select></label>
    </div>
    <div className="process-table-wrap"><table className="process-table"><thead><tr><th>APPLICATION</th><th>PID</th><th>MEMORY (WS)</th><th>PRIVATE</th><th>TREND</th><th>STATUS</th></tr></thead>
      <tbody>{rows.map((process) => <tr key={process.pid}>
        <td><span className="process-name"><span className="process-dot" />{process.name}</span></td>
        <td>{process.pid}</td>
        <td>{fmtBytes(process.workingSetBytes)}</td>
        <td>{fmtBytes(process.privateBytes)} <small className="share-label">{process.sharePercent.toFixed(1)}%</small></td>
        <td className={process.trendDirection === 'rising' ? 'trend-rising' : process.trendDirection === 'falling' ? 'trend-falling' : ''}>{process.trendDirection === 'rising' ? <ArrowUpRight size={12} className="trend-icon" /> : process.trendDirection === 'falling' ? <ArrowDownRight size={12} className="trend-icon" /> : <Minus size={12} className="trend-icon" />}{mbPerMin(process.trendBytesPerMin)}</td>
        <td>{process.leak ? <span className={`leak-badge ${process.leak.confidence}`}>growth {process.leak.confidence}</span> : process.samples < 8 ? <span className="leak-badge observing">observing</span> : <span className="leak-badge stable">stable</span>}</td>
      </tr>)}</tbody></table>
      {rows.length === 0 && <div className="empty-state"><strong>No matching processes</strong><span>Adjust the search or filter, or wait for the next process scan.</span></div>}
    </div>
    <p className="muted-note">A large working set is not inefficiency: private bytes, shared pages and commit charge are reported separately. Growth labels require sustained observations, never a single snapshot.</p>
  </Panel>
}

function HistoryCharts() {
  const series = useMemoryStore((state) => state.series)
  const rows = useMemo(() => (series?.samples ?? []).slice(-240).map((point) => ({
    label: new Date(point.timestamp).toLocaleTimeString([], { hour: '2-digit', minute: '2-digit', second: '2-digit' }),
    usedGb: gb(point.usedBytes),
    availableGb: gb(point.availableBytes),
