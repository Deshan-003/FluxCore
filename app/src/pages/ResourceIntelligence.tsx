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
  const disabled = busy || !data.monitoring || !data.historyEnabled || !!data.status
  const update = async (next: ResourcePolicy) => { if (await onChange(next)) { setDraft({}); setHorizons(null) } }
  return <Panel title="Adaptive optimization" icon={SlidersHorizontal}>
    <Detail label="Active mode" value={modes[data.policy.mode]} />
    <Detail label="Policy status" value={data.policy.paused ? 'Paused' : data.activeIntervalMs ? `Temporary scan minimum: ${data.activeIntervalMs / 1000} s` : disabled ? 'Actions unavailable' : 'No active override'} />
    <form onSubmit={(e) => { e.preventDefault(); void update({ ...policy, horizonMinutes: (horizons ?? policy.horizonMinutes.join(',')).split(',').map((n) => Number(n.trim())) }) }}>
      <SettingRow title="Optimization mode" description="Adaptive Optimization must be explicitly enabled. Analysis continues in all modes."><select aria-label="Optimization mode" className="setting-select" value={policy.mode} onChange={(e) => setDraft({ ...draft, mode: e.target.value as ResourceMode })}>{Object.entries(modes).map(([value, name]) => <option value={value} key={value}>{name}</option>)}</select></SettingRow>
      <SettingRow title="Minimum scan interval" description="Affects detailed FluxCore memory-process scans only; it does not slow hardware collection."><input className="setting-select" aria-label="Minimum scan interval in seconds" type="number" min={5} max={60} required value={policy.scanIntervalMs / 1000} onChange={(e) => setDraft({ ...draft, scanIntervalMs: Number(e.target.value) * 1000 })} /></SettingRow>
      <SettingRow title="Maximum scan interval" description="Upper limit for the temporary policy (60 seconds maximum)."><input className="setting-select" aria-label="Maximum scan interval in seconds" type="number" min={5} max={60} required value={policy.maxScanIntervalMs / 1000} onChange={(e) => setDraft({ ...draft, maxScanIntervalMs: Number(e.target.value) * 1000 })} /></SettingRow>
      <SettingRow title="Cooldown" description="Minimum time before another change may be applied."><input className="setting-select" aria-label="Cooldown in seconds" type="number" min={180} max={3600} required value={policy.cooldownSeconds} onChange={(e) => setDraft({ ...draft, cooldownSeconds: Number(e.target.value) })} /></SettingRow>
      <SettingRow title="Forecast horizons" description="Up to three ascending comma-separated values from 1 to 15 minutes."><input className="setting-select" aria-label="Forecast horizons in minutes" value={horizons ?? policy.horizonMinutes.join(', ')} onChange={(e) => setHorizons(e.target.value)} /></SettingRow>
      <div className="intelligence-controls"><button className="diagnostic-button" disabled={busy}>{policy.mode === 'adaptive' && data.policy.mode !== 'adaptive' ? 'Enable Adaptive Optimization' : 'Save policy'}</button>
        <button className="diagnostic-button" type="button" disabled={busy} onClick={() => void update({ ...data.policy, paused: !data.policy.paused })}>{data.policy.paused ? 'Resume policy' : 'Pause policy'}</button>
        <button className="diagnostic-button" type="button" disabled={busy} onClick={() => void update(defaultPolicy)}>Reset policy</button>
        {data.policy.mode === 'manual' && <button className="diagnostic-button" type="button" disabled={disabled || data.policy.paused || !!data.activeIntervalMs} onClick={onManual}>Apply saved scan interval</button>}
      </div>
    </form>
    <SensorNotice text="An override expires after three minutes. High memory pressure takes priority. CPU scheduling, GPU mode, concurrency, and VRAM budgets are not changed here; GPU Acceleration retains its own controls." />
  </Panel>
}

export default function ResourceIntelligencePage() {
  const { data, history, error, busy, refresh, change } = useResourceStore()
  const [section, setSection] = useState<(typeof sections)[number]>('System health')
  const [metric, setMetric] = useState('cpu')
  const [horizonChoice, setHorizon] = useState(1)
  const [workloads, setWorkloads] = useState<SuitabilityAssessment[] | null>(null)
  const [gpuError, setGpuError] = useState<string | null>(null)
  const [gpuBusy, setGpuBusy] = useState(false)
  const horizon = data?.policy.horizonMinutes.includes(horizonChoice) ? horizonChoice : data?.policy.horizonMinutes[0] ?? 1
  const loadHistory = section === 'History' || section === 'Forecasts'
  useEffect(() => {
    const read = () => { if (!document.hidden) void (loadHistory ? refresh(metric, horizon) : refresh()) }
    read()
    const timer = window.setInterval(read, 15_000)
    document.addEventListener('visibilitychange', read)
    return () => { window.clearInterval(timer); document.removeEventListener('visibilitychange', read) }
  }, [loadHistory, metric, horizon, refresh])
  const currentHistory = history?.key === `${metric}@${horizon}` ? history.data : null
  if (!data) return <Panel title="System intelligence" icon={BrainCircuit}><EmptyState title="Waiting for resource intelligence" detail={error ?? 'Open the desktop app and wait for the first sample.'} /></Panel>
  const values = data.latest?.values ?? {}
  const applyAllowed = data.monitoring && data.historyEnabled && !data.policy.paused && !data.activeIntervalMs && !data.status
  const gpuAdvice = async () => {
    setGpuBusy(true); setGpuError(null)
    try { setWorkloads(await getWorkloadRecommendations()) } catch (error) { setGpuError(String(error)) } finally { setGpuBusy(false) }
  }
  return <div className="page-main wide-page intelligence-page">
    <nav className="intelligence-controls" aria-label="Resource Intelligence sections">{sections.map((name) => <button className="diagnostic-button" aria-pressed={section === name} key={name} onClick={() => setSection(name)}>{name}</button>)}</nav>
    {(error || data.status) && <p className="inline-warning" role="alert">{error ?? data.status}</p>}
    {!data.monitoring && <SensorNotice text="Monitoring is paused or still initializing. Resource readings will resume with the next live collection." />}
    {data.monitoring && !data.historyEnabled && <SensorNotice text="Local history is off. Analysis uses only in-memory observations; optimization actions and persisted forecast comparisons are disabled." />}
    {section === 'System health' && <><Panel title={data.health} icon={BrainCircuit}>
      <p className="muted-note">{data.latest ? `Observed ${localTime(data.latest.at)} · Local statistical analysis` : 'No current sample'}</p>
      {data.insights.map((insight) => <p className="section-description" key={insight}>{insight}</p>)}
      <div className="intelligence-readings">{['cpu', 'corePeak', 'cpuMHz', 'cpuTemp', 'ram', 'ramAvailable', 'commit', 'pagefileBytes', 'pageReads'].map((key) => <Detail key={key} label={metricLabel(key)} value={reading(key, values[key])} />)}
      <Detail label="Memory pressure" value={data.latest?.pressure ? readable(data.latest.pressure) : 'Unavailable'} />
      {['gpu:', 'vram:', 'disk:'].filter((prefix) => !Object.keys(values).some((key) => key.startsWith(prefix))).map((prefix) => <Detail key={prefix} label={metricLabel(prefix.slice(0, -1))} value="Unavailable" />)}
      {Object.keys(values).filter((key) => key.includes(':')).map((key) => <Detail key={key} label={metricLabel(key)} value={reading(key, values[key])} />)}</div>
    </Panel><Panel title="Highlights" icon={Activity}>
      <Detail label="Active anomalies" value={String(data.anomalies.filter((a) => a.endedAt == null).length)} />
      <Detail label="Pending recommendations" value={String(data.recommendations.filter((r) => r.status === 'pending').length)} />
      {data.forecasts.filter((f) => f.value != null && f.horizonMinutes === data.policy.horizonMinutes[0] && ['cpu', 'ram', 'commit'].includes(f.metric)).map((f) => <Detail key={f.metric} label={`${metricLabel(f.metric)} · ${f.horizonMinutes} min estimate`} value={`${reading(f.metric, f.value)} (${f.confidence} confidence)`} />)}
    </Panel></>}
    {section === 'Forecasts' && <ForecastView data={data} history={currentHistory} metric={metric} setMetric={setMetric} horizon={horizon} setHorizon={setHorizon} />}
    {section === 'Recommendations' && <><Panel title="Optimization recommendations" icon={Lightbulb}>
      {!data.recommendations.length && <EmptyState title="No active recommendations" detail="FluxCore needs supporting measurements before it suggests a change." />}
      {data.recommendations.map((rec) => <article className="intelligence-record" key={rec.id}>
        <h3>{rec.problem}</h3><p className="muted-note">{rec.resource} · {readable(rec.severity)} · {rec.confidence} confidence · {readable(rec.status)}</p>
        <ul>{rec.evidence.map((fact, i) => <li key={i}>{fact}</li>)}</ul><p>{rec.action}</p>
        <details><summary>Benefit, risks and approval</summary><p>{rec.benefit} {rec.risk}</p><p>{rec.requiresApproval ? 'Smart Assist requires your approval; Adaptive can act only after opt-in.' : 'Guidance only; FluxCore will not act on this recommendation.'}</p></details>
        {rec.status === 'pending' && <div className="intelligence-controls">
          {rec.applicable && data.policy.mode === 'smartAssist' && <button className="diagnostic-button" disabled={busy || !applyAllowed} onClick={() => { if (window.confirm('Apply this temporary FluxCore scan interval?')) void change(() => respondToResourceRecommendation(rec.id, 'apply', true)) }}>Approve & apply</button>}
          <button className="diagnostic-button" disabled={busy} onClick={() => void change(() => respondToResourceRecommendation(rec.id, 'reject'))}>Reject</button>
          <button className="diagnostic-button" disabled={busy} onClick={() => void change(() => respondToResourceRecommendation(rec.id, 'dismiss'))}>Dismiss</button>
        </div>}
      </article>)}
    </Panel><Panel title="Supported CPU/GPU workloads" icon={Zap} action={<button className="diagnostic-button" disabled={gpuBusy} onClick={() => void gpuAdvice()}>{gpuBusy ? 'Checking…' : 'Assess workloads'}</button>}>
      <p className="muted-note">The existing workload profiler evaluates built-in workloads at their default sizes. Recommendations do not accelerate unrelated applications or start tasks.</p>
      {gpuError && <p role="alert">{gpuError}</p>}
      {workloads?.map((w) => <article className="intelligence-record" key={w.workloadId}><h3>{readable(w.workloadId.replaceAll('_', ' '))}</h3><p>{readable(w.classification)} · {w.inputSize.toLocaleString()} input size · {readable(w.estimateSource)} evidence</p><p>{w.reason}</p><Detail label="Required VRAM / available VRAM" value={`${fmtBytes(w.requiredVramBytes)} / ${fmtBytes(w.availableVramBytes)}`} /></article>)}
    </Panel></>}
    {section === 'Adaptive controls' && <PolicyView data={data} busy={busy} onChange={(policy) => change(() => configureResourceIntelligence(policy, policy.mode === 'adaptive'))} onManual={() => { if (window.confirm('Apply your saved scan interval for up to three minutes?')) void change(applyResourcePolicy) }} />}
    {section === 'Anomalies' && <Panel title="Resource anomalies" icon={ShieldCheck}>
      {!data.anomalies.length && <EmptyState title="No detected anomalies" detail="Brief spikes are filtered out. Missing measurements are not interpreted as recovery." />}
      {data.anomalies.map((a) => <article className="intelligence-record" key={a.id}><h3>{metricLabel(a.metric)}</h3><p className="muted-note">{readable(a.severity)} · {localDate(a.startedAt)} · {a.endedAt == null ? 'Active' : `Ended ${localDate(a.endedAt)}`}</p><p>{a.explanation} {a.resolution}</p><Detail label="Reading / detection threshold" value={`${reading(a.metric, a.value)} / ${reading(a.metric, a.threshold)}`} /><p>Review the associated workload and recommendations. This is not a security alert.</p></article>)}
    </Panel>}
    {section === 'History' && <><Panel title="Optimization history" icon={History} action={<button className="diagnostic-button" onClick={() => void refresh(metric, horizon)}>Refresh history</button>}>
      {!currentHistory?.actions.length && <EmptyState title={currentHistory ? 'No recorded optimizations' : 'Loading history'} detail="Real policy changes and their evaluations appear here." />}
      {currentHistory?.actions.map((action) => <article className="intelligence-record" key={action.id}>
        <h3>Detailed process scan interval · {readable(action.status)}</h3><p className="muted-note">{localDate(action.at)} · {modes[action.mode]}</p><p>{action.detail}</p>
        <Detail label="Phase 2 setting / temporary minimum" value={`${action.previousIntervalMs == null ? 'Unavailable' : `${action.previousIntervalMs / 1000} s`} / ${action.intervalMs / 1000} s`} />
        {action.outcome ? <><p><strong>{readable(action.outcome.verdict)}:</strong> {action.outcome.detail}</p><details><summary>Before and after averages</summary>{Object.keys({ ...action.outcome.before, ...action.outcome.after }).sort().map((key) => <Detail key={key} label={metricLabel(key)} value={`${reading(key, action.outcome?.before[key])} → ${reading(key, action.outcome?.after[key])}`} />)}</details></> : <p className="muted-note">{action.before.length} baseline observations · {action.endedAt == null ? 'Evaluation pending' : 'No comparable post-action measurement was recorded'}</p>}
        {action.endedAt == null && data.activeIntervalMs && <button className="diagnostic-button" disabled={busy} onClick={() => void change(() => revertResourcePolicy(action.id)).then((ok) => { if (ok) void refresh(metric, horizon) })}>Revert change</button>}
      </article>)}
    </Panel><Panel title="Recommendation decisions" icon={Lightbulb}>{(currentHistory?.decisions ?? []).filter((r) => r.status !== 'pending').map((r) => <Detail key={r.id} label={r.problem} value={readable(r.status)} />)}
      {!currentHistory?.decisions.some((r) => r.status !== 'pending') && <p className="muted-note">No recorded decisions.</p>}
    </Panel></>}
  </div>
}
