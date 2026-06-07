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
    committedGb: gb(point.committedBytes),
    compressedMb: point.compressedBytes == null ? null : Number((point.compressedBytes / 1024 ** 2).toFixed(1)),
    pagefileGb: gb(point.pagefileUsedBytes),
  })), [series])
  const chart = (dataKey: string, name: string, color: string, unit: string) => rows.length < 2
    ? <div className="empty-state"><strong>Collecting memory history</strong><span>Samples are persisted every few seconds; charts appear once history exists.</span></div>
    : <div className="mi-chart"><ResponsiveContainer width="100%" height="100%"><AreaChart data={rows} margin={{ top: 8, right: 6, bottom: 0, left: -18 }}>
        <defs><linearGradient id={`mi-${dataKey}`} x1="0" y1="0" x2="0" y2="1"><stop offset="0%" stopColor={color} stopOpacity={0.25} /><stop offset="100%" stopColor={color} stopOpacity={0} /></linearGradient></defs>
        <CartesianGrid stroke="#202b3b" strokeDasharray="3 5" vertical={false} />
        <XAxis dataKey="label" tick={{ fill: '#778399', fontSize: 9 }} tickLine={false} axisLine={false} minTickGap={44} />
        <YAxis tick={{ fill: '#778399', fontSize: 9 }} tickLine={false} axisLine={false} tickFormatter={(value) => `${value}${unit}`} />
        <Tooltip contentStyle={{ background: '#101a29', border: '1px solid #2b3749', borderRadius: 9, color: '#e8eef7', fontSize: 11 }} labelStyle={{ color: '#92a1b5' }} />
        <Area dataKey={dataKey} name={name} type="monotone" stroke={color} strokeWidth={1.8} fill={`url(#mi-${dataKey})`} isAnimationActive={false} connectNulls />
      </AreaChart></ResponsiveContainer></div>
  return <div className="mi-charts">
    <Panel title="RAM consumption & availability" icon={MemoryStick}>{chart('usedGb', 'Used GB', '#a78bfa', ' GB')}</Panel>
    <Panel title="Committed memory" icon={Database}>{chart('committedGb', 'Committed GB', '#56c7e8', ' GB')}</Panel>
    <Panel title="Compressed memory" icon={Sparkles}>{chart('compressedMb', 'Compressed MB', '#f4b75e', ' MB')}</Panel>
    <Panel title="Pagefile usage" icon={Gauge}>{chart('pagefileGb', 'Pagefile GB', '#f186ab', ' GB')}</Panel>
  </div>
}

function RecommendationCard({ recommendation }: { recommendation: Recommendation }) {
  const { mode, runAction, lastResult } = useMemoryStore()
  const [confirming, setConfirming] = useState(false)
  const [detail, setDetail] = useState<string | null>(null)
  const executable = recommendation.actionId !== null && mode !== 'analysisOnly'
  const act = async () => {
    if (!recommendation.actionId) return
    if (recommendation.requiresApproval && !confirming) { setConfirming(true); return }
    const result = await runAction(recommendation.actionId, true)
    setConfirming(false)
    if (result) setDetail(`${result.status}: ${result.detail}`)
  }
  const shown = detail ?? (lastResult && recommendation.actionId === lastResult.actionId ? `${lastResult.status}: ${lastResult.detail}` : null)
  return <div className={`rec-card risk-${recommendation.risk}`}>
    <div className="rec-head"><strong>{recommendation.title}</strong><span className={`risk-badge ${recommendation.risk}`}>{recommendation.risk} risk</span></div>
    <p className="rec-reason">{recommendation.reason}</p>
    <p className="rec-benefit"><CheckCircle2 size={13} />{recommendation.expectedBenefit}</p>
    <div className="rec-foot">
      <span className="rec-category">{recommendation.category}</span>
      {executable && <button className={`rec-action ${confirming ? 'confirming' : ''}`} onClick={() => void act()}>{confirming ? 'Click again to confirm' : recommendation.requiresApproval ? 'Approve & run' : 'Run'}</button>}
      {!executable && recommendation.actionId && <span className="rec-note">Switch to Smart Assist or Manual mode to run</span>}
    </div>
    {shown && <p className="rec-result">{shown}</p>}
  </div>
}

export default function MemoryIntelligencePage() {
  const intelligence = useMemoryStore((state) => state.intelligence)
  const refreshSeries = useMemoryStore((state) => state.refreshSeries)
  const status = useMonitoringStore((state) => state.status)
  useEffect(() => { void refreshSeries() }, [refreshSeries, intelligence?.collectedAt])
  if (status !== 'connected') return <div className="empty-state"><div className="empty-icon"><BrainCircuit size={20} /></div><strong>Memory engine offline</strong><span>Open FluxCore in the Windows desktop app to collect live memory intelligence.</span></div>
  if (!intelligence) return <div className="empty-state"><div className="empty-icon"><Activity size={20} /></div><strong>Waiting for the memory engine</strong><span>The first analysis appears after a few monitoring ticks.</span></div>
  const { analysis, compression, virtualMemory } = intelligence
  return <div className="mi-page">
    <div className="mi-overview">
      <OverviewTile label="Physical RAM" value={fmtBytes(analysis.usedBytes)} sub={`of ${fmtBytes(analysis.totalBytes)} · ${analysis.usagePercent.toFixed(0)}% in use`} />
      <OverviewTile label="Available" value={fmtBytes(analysis.availableBytes)} sub={`${analysis.availablePercent.toFixed(0)}% · reclaimable cache excluded`} tone="mint" />
      <OverviewTile label="Committed" value={fmtBytes(analysis.committedBytes)} sub={`of ${fmtBytes(analysis.commitLimitBytes)} limit · ${analysis.commitPercent == null ? '—' : `${analysis.commitPercent.toFixed(0)}%`}`} />
      <OverviewTile label="Compressed" value={compression.available ? fmtBytes(compression.compressedBytes) : 'Unavailable'} sub={compression.available ? 'Windows memory compression' : 'not exposed by this build'} />
      <OverviewTile label="Pagefile" value={fmtBytes(analysis.pagefileUsedBytes)} sub={`${fmtBytes(analysis.pagefileTotalBytes)} capacity · ${analysis.pagefilePercent == null ? '—' : `${analysis.pagefilePercent.toFixed(0)}%`}`} />
      <OverviewTile label="Standby / modified" value={`${fmtBytes(analysis.standbyBytes)} / ${fmtBytes(analysis.modifiedBytes)}`} sub="reclaimable · not counted as used" tone="violet" />
      <OverviewTile label="Hard faults" value={analysis.hardFaultsPerSec == null ? '—' : `${analysis.hardFaultsPerSec.toFixed(0)}/s`} sub="physical page reads" />
      <OverviewTile label="Pressure" value={intelligence.pressure.level} sub={`since ${new Date(intelligence.pressure.levelSince).toLocaleTimeString()}`} tone={levelClass(intelligence.pressure.level)} />
    </div>
    <div className="mi-grid-2">
      <PressureIndicator />
      <PredictionCard />
    </div>
    <ProcessRanking />
    <HistoryCharts />
    <Panel title={`Optimization recommendations · ${intelligence.recommendations.length}`} icon={ShieldCheck}>
      {intelligence.recommendations.length ? <div className="rec-grid">{intelligence.recommendations.map((recommendation) => <RecommendationCard key={recommendation.id} recommendation={recommendation} />)}</div> : <div className="empty-state"><div className="empty-icon"><CircleSlash size={18} /></div><strong>No recommendations right now</strong><span>Recommendations are generated only from live measurements when pressure, commit charge or growth patterns justify them.</span></div>}
      {intelligence.limitations.length > 0 && <div className="sensor-notice"><AlertTriangle size={15} /><span>{intelligence.limitations.join(' ')}</span></div>}
      <div className="mi-sources"><span>Sources: {analysis.sources.join(' · ')}</span></div>
      <p className="muted-note">{compression.pressureRelationship} {compression.cpuImpactNote}</p>
      {virtualMemory.notes.map((note, index) => <p className="muted-note" key={index}>{note}</p>)}
    </Panel>
  </div>
}
