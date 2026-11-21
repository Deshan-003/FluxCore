import { useEffect, useState } from 'react'
import { CartesianGrid, ComposedChart, Legend, Line, ResponsiveContainer, Tooltip, XAxis, YAxis } from 'recharts'
import {
  Bell, BrainCircuit, ChartNoAxesCombined, Lightbulb, ShieldCheck, SlidersHorizontal, Zap,
} from 'lucide-react'
import {
  Button, Card, ChartFrame, ConfirmDialog, DetailList, Disclose, EmptyState, PageHeader, RXTooltip,
  StatusChip, Tabs, type ConfirmSpec,
} from '../ui/kit'
import { fmtBytes, fmtRate } from '../lib/format'
import { rxAxis, rxGrid, rxTick } from '../ui/chart-config'
import { getWorkloadRecommendations } from '../services/gpu'
import {
  applyResourcePolicy, configureResourceIntelligence, respondToResourceRecommendation,
  revertResourcePolicy,
} from '../services/intelligence'
import { useResourceStore } from '../stores/intelligence'
import type { SuitabilityAssessment } from '../types/gpu'
import type {
  Forecast, ResourceHistory, ResourceMode, ResourcePolicy, ResourceState,
} from '../types/intelligence'
import { toast } from '../ui/toast-store'

const TABS = [
  { id: 'overview', label: 'Overview' },
  { id: 'forecasts', label: 'Forecasts' },
  { id: 'recommendations', label: 'Recommendations' },
  { id: 'anomalies', label: 'Anomalies' },
  { id: 'history', label: 'History' },
  { id: 'controls', label: 'Controls' },
] as const
type TabId = typeof TABS[number]['id']

const MODES: Record<ResourceMode, string> = { analysisOnly: 'Analysis Only', smartAssist: 'Smart Assist', adaptive: 'Adaptive Optimization', manual: 'Manual Control' }
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
const readable = (value: string) => value.replace(/([a-z])([A-Z])/g, '$1 $2').replace(/^\w/, (c) => c.toUpperCase())
const severityChip = (severity: string) => severity === 'critical' || severity === 'high'
  ? <StatusChip status="critical" label={readable(severity)} />
  : severity === 'medium' || severity === 'low' ? <StatusChip status="attention" label={readable(severity)} />
  : <StatusChip status="normal" label={readable(severity)} />

function ForecastView({ data, history, metric, setMetric, horizon, setHorizon }: {
  data: ResourceState; history: ResourceHistory | null; metric: string; setMetric: (v: string) => void
  horizon: number; setHorizon: (v: number) => void
}) {
  const options = Array.from(new Set(['cpu', 'ram', 'commit', ...data.forecasts.map((f) => f.metric)]))
  const forecast = data.forecasts.find((f) => f.metric === metric && f.horizonMinutes === horizon)
  const rows: { at: number; measured: number | null; estimate: number | null }[] = (history?.observations ?? []).map((row) => ({
    at: row.at, measured: row.values[metric] ?? null,
    estimate: row.at === forecast?.observedAt ? row.values[metric] ?? null : null,
  }))
  if (forecast?.value != null && rows.some((row) => row.at === forecast.observedAt && row.estimate != null)) {
    rows.push({ at: forecast.targetAt, measured: null, estimate: forecast.value })
    rows.sort((a, b) => a.at - b.at)
  }
  const comparisons = history?.comparisons.slice(-10) ?? []
  return <Card title="Performance forecasts" icon={ChartNoAxesCombined}
    sub="Predictions are statistical estimates from local history — never measurements.">
    <div style={{ display: 'flex', gap: 10, flexWrap: 'wrap', marginBottom: 12 }}>
      <label style={{ display: 'flex', alignItems: 'center', gap: 7, fontSize: 12.5 }}>Metric
        <select className="rx-select" value={metric} aria-label="Forecast metric" onChange={(e) => setMetric(e.target.value)}>
          {!options.includes(metric) && <option value={metric}>{metricLabel(metric)}</option>}
          {options.map((key) => <option key={key} value={key}>{metricLabel(key)}</option>)}
        </select></label>
      <label style={{ display: 'flex', alignItems: 'center', gap: 7, fontSize: 12.5 }}>Horizon
        <select className="rx-select" value={horizon} aria-label="Forecast horizon" onChange={(e) => setHorizon(Number(e.target.value))}>
          {data.policy.horizonMinutes.map((value) => <option key={value} value={value}>{value} min</option>)}
        </select></label>
    </div>
    {forecast?.value != null ? <>
      <DetailList rows={[
        { label: `Estimate at ${localTime(forecast.targetAt)}`, value: reading(metric, forecast.value) },
        { label: 'Indicative uncertainty range', value: `${reading(metric, forecast.low)} – ${reading(metric, forecast.high)}` },
        { label: 'Confidence / method', value: `${readable(forecast.confidence)} · ${readable(forecast.method)}` },
        { label: 'Historical data quality', value: `${forecast.samples} samples · ${Math.round(forecast.observationSeconds / 60)} min · ${Math.round(forecast.coverage * 100)}% coverage` },
      ]} />
      <p className="rx-chart-caption">{forecast.explanation}</p>
    </> : <EmptyState title="Forecast unavailable" detail={forecast?.explanation ?? 'No recent measurements exist for this metric.'} />}
    {rows.filter((row) => row.measured != null).length > 1 && <ChartFrame ariaLabel="Measured history and dashed forecast" height="tall"
      caption="Solid: measured so far. Dashed: predicted by FluxCore from the recent trend.">
      <ResponsiveContainer width="100%" height="100%">
        <ComposedChart data={rows} margin={{ top: 8, right: 8, bottom: 0, left: -8 }}>
          <CartesianGrid {...rxGrid} />
          <XAxis type="number" dataKey="at" domain={['dataMin', 'dataMax']} {...rxAxis} tick={rxTick} tickFormatter={(value) => localTime(Number(value))} minTickGap={45} />
          <YAxis {...rxAxis} tick={rxTick} tickFormatter={(value: number) => reading(metric, value)} />
          <Tooltip content={<RXTooltip formatter={(v) => `${reading(metric, Number(v))}`} />} labelFormatter={(value) => localTime(Number(value))} />
          <Legend wrapperStyle={{ color: 'var(--rx-text-muted)', fontSize: 12 }} />
          <Line name="Measured" dataKey="measured" stroke="var(--rx-c-memory)" strokeWidth={2} dot={false} isAnimationActive={false} connectNulls />
          <Line name="Estimate" dataKey="estimate" stroke="var(--rx-c-other)" strokeDasharray="6 4" strokeWidth={2} connectNulls dot={false} isAnimationActive={false} />
        </ComposedChart>
      </ResponsiveContainer>
    </ChartFrame>}
    <h3 style={{ fontSize: 13.5, fontWeight: 600, margin: '14px 0 8px' }}>Past estimates vs. measurements</h3>
    {comparisons.length
      ? <div className="rx-table-wrap"><table className="rx-table">
          <thead><tr><th>Target time</th><th>Estimate</th><th>Measured</th><th>Absolute error</th></tr></thead>
          <tbody>
            {comparisons.map((item: Forecast) => <tr key={item.targetAt}>
              <td>{localDate(item.targetAt)}</td><td>{reading(metric, item.value)}</td><td>{reading(metric, item.actual)}</td>
              <td>{item.actual == null || item.value == null ? '—' : reading(metric, Math.abs(item.actual - item.value))}</td>
            </tr>)}
          </tbody>
        </table></div>
      : <p className="rx-chart-caption">No eligible target-time measurements yet. Results are recorded only when local history is enabled.</p>}
  </Card>
}

function PolicyView({ data, busy, change, confirm }: {
  data: ResourceState; busy: boolean
  change: (operation: () => Promise<void>) => Promise<boolean>
  confirm: (spec: ConfirmSpec) => void
}) {
  const [draft, setDraft] = useState<Partial<ResourcePolicy>>({})
  const [horizons, setHorizons] = useState<string | null>(null)
  const policy = { ...data.policy, ...draft }
  const disabled = busy || !data.monitoring || !data.historyEnabled || !!data.status
  const update = async (next: ResourcePolicy) => {
    if (await change(() => configureResourceIntelligence(next, next.mode === 'adaptive'))) {
      setDraft({}); setHorizons(null); toast('Resource policy saved', 'success')
    }
  }
  const numberField = (label: string, help: string, value: number, onChange: (value: number) => void, min: number, max: number) =>
    <div className="rx-field">
      <label>{label}</label>
      <input className="rx-input rx-number-input" type="number" min={min} max={max} required value={value} aria-label={label}
        onChange={(event) => onChange(Number(event.target.value))} />
      <span className="help">{help}</span>
    </div>
  return <Card title="Adaptive controls" icon={SlidersHorizontal}
    sub="These controls manage the Phase-2 scan-interval policy. An override expires after three minutes; high memory pressure takes priority. CPU scheduling, GPU mode, concurrency and VRAM budgets are not changed here — GPU Acceleration keeps its own controls.">
    <DetailList rows={[
      { label: 'Active mode', value: MODES[data.policy.mode] },
      { label: 'Policy status', value: data.policy.paused ? 'Paused' : data.activeIntervalMs ? `Temporary scan minimum: ${data.activeIntervalMs / 1000} s` : disabled ? 'Actions unavailable' : 'No active override' },
    ]} />
    <div className="rx-detail-grid" style={{ marginTop: 12 }}>
      <div className="rx-field">
        <label>Optimization mode</label>
        <select className="rx-select" aria-label="Optimization mode" value={policy.mode}
          onChange={(e) => setDraft({ ...draft, mode: e.target.value as ResourceMode })}>
          {Object.entries(MODES).map(([value, name]) => <option value={value} key={value}>{name}</option>)}
        </select>
        <span className="help">Adaptive Optimization must be explicitly enabled. Analysis continues in all modes.</span>
      </div>
      {numberField('Minimum scan interval (s)', 'Affects detailed FluxCore memory-process scans only; it does not slow hardware collection.', policy.scanIntervalMs / 1000, (value) => setDraft({ ...draft, scanIntervalMs: value * 1000 }), 5, 60)}
      {numberField('Maximum scan interval (s)', 'Upper limit for the temporary policy.', policy.maxScanIntervalMs / 1000, (value) => setDraft({ ...draft, maxScanIntervalMs: value * 1000 }), 5, 60)}
      {numberField('Cooldown (s)', 'Minimum time before another change may be applied.', policy.cooldownSeconds, (value) => setDraft({ ...draft, cooldownSeconds: value }), 180, 3600)}
      <div className="rx-field">
        <label>Forecast horizons (min)</label>
        <input className="rx-input" aria-label="Forecast horizons in minutes" value={horizons ?? policy.horizonMinutes.join(', ')}
          onChange={(e) => setHorizons(e.target.value)} />
        <span className="help">Up to three ascending comma-separated values from 1 to 15.</span>
      </div>
    </div>
    <div className="rx-rec-row" style={{ marginTop: 12 }}>
      <Button variant="primary" disabled={busy}
        onClick={() => void update({ ...policy, horizonMinutes: (horizons ?? policy.horizonMinutes.join(',')).split(',').map((n) => Number(n.trim())) })}>
        {policy.mode === 'adaptive' && data.policy.mode !== 'adaptive' ? 'Enable Adaptive Optimization' : 'Save policy'}
      </Button>
      <Button disabled={busy} onClick={() => void update({ ...data.policy, paused: !data.policy.paused })}>{data.policy.paused ? 'Resume policy' : 'Pause policy'}</Button>
      <Button disabled={busy} onClick={() => void update(defaultPolicy)}>Reset policy</Button>
      {data.policy.mode === 'manual' && <Button variant="primary" disabled={disabled || data.policy.paused || !!data.activeIntervalMs}
        onClick={() => confirm({
          title: 'Apply your saved scan interval?',
          body: 'The minimum scan interval is applied for up to three minutes, then reverts automatically.',
          confirmLabel: 'Apply interval',
          onConfirm: () => void change(applyResourcePolicy).then((ok) => { if (ok) toast('Scan interval override applied', 'success') }),
          onCancel: () => undefined,
        })}>Apply saved scan interval</Button>}
    </div>
  </Card>
}

export default function ResourceIntelligencePage() {
  const { data, history, error, busy, refresh, change } = useResourceStore()
  const [tab, setTab] = useState<TabId>('overview')
  const [metric, setMetric] = useState('cpu')
  const [horizonChoice, setHorizon] = useState(1)
  const [workloads, setWorkloads] = useState<SuitabilityAssessment[] | null>(null)
  const [gpuError, setGpuError] = useState<string | null>(null)
  const [gpuBusy, setGpuBusy] = useState(false)
  const [confirm, setConfirm] = useState<ConfirmSpec | null>(null)
  const horizon = data?.policy.horizonMinutes.includes(horizonChoice) ? horizonChoice : data?.policy.horizonMinutes[0] ?? 1
  const loadHistory = tab === 'history' || tab === 'forecasts'

  useEffect(() => {
    const read = () => { if (!document.hidden) void (loadHistory ? refresh(metric, horizon) : refresh()) }
    read()
    const timer = window.setInterval(read, 15_000)
    document.addEventListener('visibilitychange', read)
    return () => { window.clearInterval(timer); document.removeEventListener('visibilitychange', read) }
  }, [loadHistory, metric, horizon, refresh])

  const currentHistory = history?.key === `${metric}@${horizon}` ? history.data : null
  if (!data) {
    return <>
      <PageHeader title="Resource intelligence" description="FluxCore watches trends, predicts problems before they bite, and explains what to do — predictions are always labeled, never passed off as measurements." />
      <Card><EmptyState icon={BrainCircuit} title="Waiting for resource intelligence"
        detail={error ?? 'Open the desktop app and wait for the first sample.'} /></Card>
    </>
  }

  const values = data.latest?.values ?? {}
  const applyAllowed = data.monitoring && data.historyEnabled && !data.policy.paused && !data.activeIntervalMs && !data.status
  const gpuAdvice = async () => {
    setGpuBusy(true); setGpuError(null)
    try { setWorkloads(await getWorkloadRecommendations()) } catch (err) { setGpuError(String(err)) } finally { setGpuBusy(false) }
  }

  return <>
    <PageHeader title="Resource intelligence"
      description="FluxCore watches trends, predicts problems before they bite, and explains what to do — predictions are always labeled, never passed off as measurements." />

    <Tabs tabs={TABS.map((entry) => ({
      id: entry.id, label: entry.label,
      count: entry.id === 'anomalies' ? data.anomalies.filter((a) => a.endedAt == null).length
        : entry.id === 'recommendations' ? data.recommendations.filter((r) => r.status === 'pending').length
        : undefined,
    }))} active={tab} onChange={(id) => setTab(id as TabId)} />

    {(error || data.status) && <div className="rx-card status-edge critical" role="alert">
      <div className="rx-card-head"><h2>Analysis notice</h2></div>
      <p className="rx-card-sub" style={{ margin: 0 }}>{error ?? data.status}</p>
    </div>}
    {!data.monitoring && <p className="rx-chart-caption">Monitoring is paused or still initializing. Resource readings will resume with the next live collection.</p>}
    {data.monitoring && !data.historyEnabled && <p className="rx-chart-caption">Local history is off. Analysis uses only in-memory observations; optimization actions and persisted forecast comparisons are disabled.</p>}

    {tab === 'overview' && <>
      <div className="rx-hero">
        <span style={{ color: data.health.toLowerCase().includes('normal') || data.health.toLowerCase().includes('healthy') ? 'var(--rx-success)' : 'var(--rx-warning)' }}><ShieldCheck size={34} /></span>
        <div className="rx-hero-text">
          <h2>System health: {data.health}</h2>
          <p>{data.latest ? `Observed ${localTime(data.latest.at)} · Local statistical analysis of live measurements` : 'No current sample yet'}</p>
        </div>
      </div>
      {data.insights.length > 0 && <Card title="What the analysis found" icon={BrainCircuit}>
        <div className="rx-stack" style={{ gap: 10 }}>
          {data.insights.map((insight) => <p className="rx-card-sub" style={{ margin: 0 }} key={insight}>{insight}</p>)}
        </div>
      </Card>}
      <Card title="Latest readings" icon={BrainCircuit} note="Measured values only">
        <DetailList rows={[
          ...['cpu', 'corePeak', 'cpuMHz', 'cpuTemp', 'ram', 'ramAvailable', 'commit', 'pagefileBytes', 'pageReads'].map((key) => ({ label: metricLabel(key), value: reading(key, values[key]) })),
          { label: 'Memory pressure', value: data.latest?.pressure ? readable(data.latest.pressure) : 'Unavailable' },
          ...Object.keys(values).filter((key) => key.includes(':')).map((key) => ({ label: metricLabel(key), value: reading(key, values[key]) })),
          ...['gpu:', 'vram:', 'disk:'].filter((prefix) => !Object.keys(values).some((key) => key.startsWith(prefix)))
            .map((prefix) => ({ label: metricLabel(prefix.slice(0, -1)), value: 'Unavailable' })),
        ]} />
      </Card>
      <Card title="Highlights" icon={Zap}>
        <DetailList rows={[
          { label: 'Active anomalies', value: String(data.anomalies.filter((a) => a.endedAt == null).length) },
          { label: 'Pending recommendations', value: String(data.recommendations.filter((r) => r.status === 'pending').length) },
          ...data.forecasts.filter((f) => f.value != null && f.horizonMinutes === data.policy.horizonMinutes[0] && ['cpu', 'ram', 'commit'].includes(f.metric))
            .map((f) => ({ label: `${metricLabel(f.metric)} · ${f.horizonMinutes} min estimate`, value: `${reading(f.metric, f.value)} (${f.confidence} confidence)` })),
        ]} />
      </Card>
    </>}

    {tab === 'forecasts' && <ForecastView data={data} history={currentHistory} metric={metric} setMetric={setMetric} horizon={horizon} setHorizon={setHorizon} />}

    {tab === 'recommendations' && <>
      <Card title="Optimization recommendations" icon={Lightbulb}
        sub="Each card states the problem, the evidence, the suggested action, expected benefit and risk.">
        {data.recommendations.length === 0 && <EmptyState title="No active recommendations" detail="FluxCore needs supporting measurements before it suggests a change." />}
        <div className="rx-stack" style={{ gap: 12 }}>
          {data.recommendations.map((rec) => <div className="rx-rec-card" key={rec.id}>
            <h3>{rec.problem}</h3>
            <div className="rx-rec-meta">
              <span className="rx-chip neutral">{rec.resource}</span>
              {severityChip(rec.severity)}
              <span className="rx-chip neutral">{rec.confidence} confidence</span>
              <span className="rx-chip neutral">{readable(rec.status)}</span>
            </div>
            <ul style={{ margin: 0, paddingLeft: 18, fontSize: 12.5, color: 'var(--rx-text-secondary)' }}>
              {rec.evidence.map((fact, index) => <li key={index}>{fact}</li>)}
            </ul>
            <p><strong>Suggested action:</strong> {rec.action}</p>
            <p><strong>Benefit & risk:</strong> {rec.benefit} {rec.risk}</p>
            <p className="rx-chart-caption">{rec.requiresApproval ? 'Smart Assist requires your approval; Adaptive can act only after opt-in.' : 'Guidance only; FluxCore will not act on this recommendation.'}</p>
            {rec.status === 'pending' && <div className="rx-rec-row">
              {rec.applicable && data.policy.mode === 'smartAssist' && <Button variant="primary" disabled={busy || !applyAllowed}
                onClick={() => setConfirm({
                  title: 'Apply this temporary FluxCore scan interval?',
                  body: rec.action,
                  facts: [
                    { kind: 'benefit', text: rec.benefit },
                    { kind: 'risk', text: `${rec.risk} The override expires automatically after three minutes.` },
                  ],
                  confirmLabel: 'Approve & apply',
                  onConfirm: () => void change(() => respondToResourceRecommendation(rec.id, 'apply', true)).then((ok) => { if (ok) toast('Recommendation applied', 'success') }),
                  onCancel: () => undefined,
                })}>Approve & apply</Button>}
              <Button disabled={busy} onClick={() => void change(() => respondToResourceRecommendation(rec.id, 'reject')).then((ok) => { if (ok) toast('Recommendation rejected', 'info') })}>Reject</Button>
              <Button disabled={busy} onClick={() => void change(() => respondToResourceRecommendation(rec.id, 'dismiss')).then((ok) => { if (ok) toast('Recommendation dismissed', 'info') })}>Dismiss</Button>
            </div>}
          </div>)}
        </div>
      </Card>
      <Card title="Supported CPU/GPU workloads" icon={Zap}
        sub="The existing workload profiler evaluates built-in workloads at their default sizes. Recommendations do not accelerate unrelated applications or start tasks."
        right={<Button disabled={gpuBusy} loading={gpuBusy} onClick={() => void gpuAdvice()}>{gpuBusy ? 'Checking…' : 'Assess workloads'}</Button>}>
        {gpuError && <p className="rx-chart-caption" role="alert" style={{ color: 'var(--rx-danger)' }}>{gpuError}</p>}
        {!workloads && !gpuError && <EmptyState title="Not assessed yet" detail="Run an assessment to see which built-in workloads would benefit from GPU execution." />}
        {workloads?.map((w) => <div className="rx-rec-card" key={w.workloadId} style={{ marginBottom: 10 }}>
          <h3>{readable(w.workloadId.replaceAll('_', ' '))}</h3>
          <p>{readable(w.classification)} · {w.inputSize.toLocaleString()} input size · {readable(w.estimateSource)} evidence</p>
          <p>{w.reason}</p>
          <DetailList rows={[{ label: 'Required VRAM / available VRAM', value: `${fmtBytes(w.requiredVramBytes)} / ${fmtBytes(w.availableVramBytes)}` }]} />
        </div>)}
      </Card>
    </>}

    {tab === 'anomalies' && <Card title="Resource anomalies" icon={Bell}
      sub="Brief spikes are filtered out. Missing measurements are not interpreted as recovery. This is not a security alert.">
      {data.anomalies.length === 0 && <EmptyState title="No detected anomalies" detail="Nothing unusual in the analyzed window." />}
      <div className="rx-stack" style={{ gap: 12 }}>
        {data.anomalies.map((anomaly) => <div className={`rx-card status-edge ${anomaly.severity === 'critical' || anomaly.severity === 'high' ? 'critical' : 'warning'}`} key={anomaly.id}>
          <div className="rx-card-head">
            <Bell size={15} style={{ color: anomaly.severity === 'critical' || anomaly.severity === 'high' ? 'var(--rx-danger)' : 'var(--rx-warning)' }} />
            <h2>{metricLabel(anomaly.metric)}</h2>
            {severityChip(anomaly.severity)}
            <div className="rx-card-head-right"><span className="rx-data-age">{localDate(anomaly.startedAt)} · {anomaly.endedAt == null ? 'Active' : `Ended ${localDate(anomaly.endedAt)}`}</span></div>
          </div>
          <p style={{ margin: '0 0 8px', fontSize: 13 }}>{anomaly.explanation} {anomaly.resolution}</p>
          <DetailList rows={[{ label: 'Reading / detection threshold', value: `${reading(anomaly.metric, anomaly.value)} / ${reading(anomaly.metric, anomaly.threshold)}` }]} />
          <p className="rx-chart-caption">Review the associated workload and recommendations.</p>
        </div>)}
      </div>
    </Card>}

    {tab === 'history' && <Card title="Optimization history" icon={BrainCircuit}
      right={<Button small onClick={() => void refresh(metric, horizon)}>Refresh history</Button>}>
      {!currentHistory?.actions.length && <EmptyState title={currentHistory ? 'No recorded optimizations' : 'Loading history'} detail="Real policy changes and their evaluations appear here." />}
      <div className="rx-stack" style={{ gap: 12 }}>
        {currentHistory?.actions.map((action) => <div className="rx-card status-edge info" key={action.id}>
          <div className="rx-card-head"><h2>Detailed process scan interval</h2><StatusChip status={action.status === 'applied' ? 'healthy' : 'attention'} label={readable(action.status)} />
            <div className="rx-card-head-right"><span className="rx-data-age">{localDate(action.at)} · {MODES[action.mode]}</span></div></div>
          <p style={{ margin: '0 0 8px', fontSize: 13 }}>{action.detail}</p>
          <DetailList rows={[{ label: 'Phase 2 setting / temporary minimum', value: `${action.previousIntervalMs == null ? 'Unavailable' : `${action.previousIntervalMs / 1000} s`} / ${action.intervalMs / 1000} s` }]} />
          {action.outcome ? <>
            <p style={{ margin: '8px 0', fontSize: 13 }}><strong>{readable(action.outcome.verdict)}:</strong> {action.outcome.detail}</p>
            <Disclose summary="Before and after averages">
              <div className="rx-disclose-body">
                <DetailList rows={Object.keys({ ...action.outcome.before, ...action.outcome.after }).sort().map((key) => ({ label: metricLabel(key), value: `${reading(key, action.outcome?.before[key])} → ${reading(key, action.outcome?.after[key])}` }))} />
              </div>
            </Disclose>
          </> : <p className="rx-chart-caption">{action.before.length} baseline observations · {action.endedAt == null ? 'Evaluation pending' : 'No comparable post-action measurement was recorded'}</p>}
          {action.endedAt == null && data.activeIntervalMs && <Button disabled={busy}
            onClick={() => void change(() => revertResourcePolicy(action.id)).then((ok) => { if (ok) { toast('Change reverted', 'success'); void refresh(metric, horizon) } })}>Revert change</Button>}
        </div>)}
      </div>
      <h3 style={{ fontSize: 13.5, fontWeight: 600, margin: '14px 0 8px' }}>Recommendation decisions</h3>
      {(currentHistory?.decisions ?? []).filter((r) => r.status !== 'pending').length
        ? <DetailList rows={(currentHistory?.decisions ?? []).filter((r) => r.status !== 'pending').map((r) => ({ label: r.problem, value: readable(r.status) }))} />
        : <p className="rx-chart-caption">No recorded decisions.</p>}
    </Card>}

    {tab === 'controls' && <PolicyView data={data} busy={busy} change={change} confirm={setConfirm} />}

    <ConfirmDialog spec={confirm} />
  </>
}
