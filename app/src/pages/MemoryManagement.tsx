import { useEffect, useState } from 'react'
import { Area, AreaChart, CartesianGrid, ResponsiveContainer, Tooltip, XAxis, YAxis } from 'recharts'
import {
  Activity, ArrowDownRight, ArrowRight, ArrowUpRight, Brain, CheckCircle2, Minus, Search,
  TriangleAlert,
} from 'lucide-react'
import { useMemoryStore } from '../stores/memory'
import { useMonitoringStore } from '../stores/monitoring'
import {
  Button, Card, ChartFrame, ConfirmDialog, DetailList, Disclose, EmptyState, InfoTip, PageHeader,
  RXTooltip, StatusChip, Tabs, type ConfirmSpec,
} from '../ui/kit'
import { fmtBytes, fmtTime, statusForPercent, type RxStatus } from '../lib/format'
import { rxAxis, rxGrid, rxTick } from '../ui/chart-config'
import { toast } from '../ui/toast-store'
import type { PressureLevel, Recommendation, TrendDirection } from '../types/memory'

const SERIES = [
  { id: 'ram', label: 'RAM in use' },
  { id: 'committed', label: 'Committed' },
  { id: 'compressed', label: 'Compressed' },
  { id: 'pagefile', label: 'Pagefile' },
] as const
type SeriesId = typeof SERIES[number]['id']

const pressureStatus = (level: PressureLevel): RxStatus =>
  level === 'normal' ? 'healthy' : level === 'elevated' ? 'attention' : 'critical'
const pressureLabel = (level: PressureLevel) =>
  level === 'normal' ? 'Normal' : level === 'elevated' ? 'Elevated' : level === 'high' ? 'High' : 'Critical'
const mbPerMin = (value: number) => `${value >= 0 ? '+' : ''}${(value / 1024 ** 2).toFixed(1)} MB/min`

const verdictFor = (level: PressureLevel) =>
  level === 'normal' ? 'Memory is handling the current workload comfortably'
    : level === 'elevated' ? 'Memory usage is elevated but the system is still coping'
    : level === 'high' ? 'Memory pressure is high — the system is leaning on the pagefile'
    : 'Memory is critical — performance is affected now'

function TrendIcon({ direction }: { direction: TrendDirection }) {
  if (direction === 'rising') return <span style={{ color: 'var(--rx-warning)', display: 'inline-flex', alignItems: 'center', gap: 3 }}><ArrowUpRight size={13} />rising</span>
  if (direction === 'falling') return <span style={{ color: 'var(--rx-success)', display: 'inline-flex', alignItems: 'center', gap: 3 }}><ArrowDownRight size={13} />falling</span>
  return <span style={{ color: 'var(--rx-text-muted)', display: 'inline-flex', alignItems: 'center', gap: 3 }}><Minus size={13} />stable</span>
}

function MetricTile({ name, value, caption, percent, status, tip }: {
  name: string; value: string; caption: string; percent: number | null; status: RxStatus; tip?: string
}) {
  return <div className="rx-card">
    <div className="rx-metric-top">
      <span className="rx-metric-name">{name}{tip && <InfoTip text={tip} />}</span>
      <StatusChip status={status} />
    </div>
    <div className="rx-metric-value-row"><span className="rx-metric-value">{value}</span></div>
    <div className="rx-metric-caption">{caption}</div>
    <div className="rx-metric-bar"><span style={{ width: `${Math.min(100, Math.max(0, percent ?? 0))}%` }} /></div>
  </div>
}

function RecommendationCard({ recommendation }: { recommendation: Recommendation }) {
  const mode = useMemoryStore((state) => state.mode)
  const runAction = useMemoryStore((state) => state.runAction)
  const lastResult = useMemoryStore((state) => state.lastResult)
  const [confirm, setConfirm] = useState<ConfirmSpec | null>(null)
  const [busy, setBusy] = useState(false)

  const executable = recommendation.actionId !== null && mode !== 'analysisOnly'
  const result = lastResult?.actionId === recommendation.actionId ? lastResult : null
  const run = async () => {
    if (!recommendation.actionId) return
    setBusy(true)
    const outcome = await runAction(recommendation.actionId, true)
    setBusy(false)
    setConfirm(null)
    if (outcome) toast(`"${recommendation.title}": ${outcome.status} — ${outcome.detail}`, outcome.status === 'success' ? 'success' : 'error')
  }
  return <div className="rx-rec-card">
    <h3>{recommendation.title}</h3>
    <div className="rx-rec-meta">
      {recommendation.risk !== 'none' && <span className={`rx-chip ${recommendation.risk === 'low' ? 'success' : 'warning'}`}>{recommendation.risk} risk</span>}
      {recommendation.requiresApproval && executable && <span className="rx-chip info">Requires your approval</span>}
    </div>
    <p>{recommendation.reason}</p>
    <p><strong>Expected benefit:</strong> {recommendation.expectedBenefit}</p>
    <div className="rx-rec-row">
      {executable && <Button variant="primary" loading={busy} onClick={() => setConfirm({
        title: `Run "${recommendation.title}"?`,
        body: recommendation.reason,
        facts: [
          { kind: 'what', text: 'FluxCore executes this optimization now and audits the result.' },
          { kind: 'benefit', text: recommendation.expectedBenefit },
          { kind: 'risk', text: `${recommendation.risk} risk — FluxCore never terminates applications or changes system settings.` },
        ],
        confirmLabel: 'Run action',
        onConfirm: () => void run(),
        onCancel: () => setConfirm(null),
      })}>Run action</Button>}
      {!executable && recommendation.actionId && <span className="rx-chart-caption">Switch to Smart Assist or Manual mode (Adaptive Optimization) to run this.</span>}
    </div>
    {result && <p className="rx-chart-caption" role="status">{result.status}: {result.detail}</p>}
    <ConfirmDialog spec={confirm} />
  </div>
}

export default function MemoryManagementPage() {
  const intelligence = useMemoryStore((state) => state.intelligence)
  const series = useMemoryStore((state) => state.series)
  const refreshSeries = useMemoryStore((state) => state.refreshSeries)
  const status = useMonitoringStore((state) => state.status)
  const [tab, setTab] = useState<SeriesId>('ram')
  const [limit] = useState(60)
  useEffect(() => { void refreshSeries() }, [refreshSeries])

  if (status !== 'connected') {
    return <>
      <PageHeader title="Memory management" description="Understand memory pressure and clean up safely — with an explanation before every action." />
      <Card><EmptyState icon={Brain} title="Memory engine offline"
        detail="Open FluxCore in the Windows desktop app to collect live memory intelligence." /></Card>
    </>
  }
  if (!intelligence) {
    return <>
      <PageHeader title="Memory management" description="Understand memory pressure and clean up safely — with an explanation before every action." />
      <Card><EmptyState icon={Activity} title="Waiting for the memory engine"
        detail="The first analysis appears after a few monitoring ticks." /></Card>
    </>
  }

  const { analysis, pressure, prediction, compression, virtualMemory } = intelligence
  const committedPercent = analysis.commitPercent
  const samples = series?.samples ?? []
  const rows = samples.slice(-limit).map((point) => ({
    label: fmtTime(point.timestamp),
    value:
      tab === 'ram' ? (analysis.totalBytes ? (point.usedBytes / analysis.totalBytes) * 100 : null)
      : tab === 'committed' ? (point.committedBytes == null || analysis.commitLimitBytes == null ? null : (point.committedBytes / analysis.commitLimitBytes) * 100)
      : tab === 'compressed' ? (point.compressedBytes == null ? null : point.compressedBytes / 1024 ** 2)
      : (point.pagefileUsedBytes == null ? null : point.pagefileUsedBytes / 1024 ** 3),
  }))
  const seriesMeta = SERIES.find((entry) => entry.id === tab)!
  const unit = tab === 'compressed' ? ' MB' : tab === 'pagefile' ? ' GB' : '%'
  const caption =
    tab === 'ram' ? 'Share of physical RAM in use. Cached/standby pages are inside "available" and reclaimable on demand.'
    : tab === 'committed' ? 'Committed memory as a share of the commit limit (RAM + pagefile). Sustained growth means apps are being promised more than RAM holds.'
    : tab === 'compressed' ? 'Memory Windows compressed to fit more into RAM. Growth is a sign of pressure.'
    : 'Pagefile use. Some use is normal; sustained growth under light load signals that RAM is too small for the workload.'
  const topConsumers = intelligence.processes.slice().sort((a, b) => b.workingSetBytes - a.workingSetBytes).slice(0, 6)

  return <>
    <PageHeader title="Memory management"
      description="Understand what is using your RAM and reclaim memory safely. Every action explains what it does before you run it." />

    <div className="rx-metric-row">
      <MetricTile name="In use" value={fmtBytes(analysis.usedBytes)} caption={`of ${fmtBytes(analysis.totalBytes)} physical RAM`}
        percent={analysis.usagePercent} status={statusForPercent(analysis.usagePercent)} />
      <MetricTile name="Available" value={fmtBytes(analysis.availableBytes)} caption="ready for new apps instantly"
        percent={analysis.availablePercent} status="healthy" />
      <MetricTile name="Committed" value={fmtBytes(analysis.committedBytes)} caption={`of ${fmtBytes(analysis.commitLimitBytes)} limit (RAM + pagefile)`}
        percent={committedPercent} status={statusForPercent(committedPercent)}
        tip="What Windows has promised to all running apps. It can exceed physical RAM because the pagefile covers the difference." />
      <MetricTile name="Pressure" value={pressureLabel(pressure.level)} caption={`since ${fmtTime(pressure.levelSince)}`}
        percent={pressure.signals.availablePercent != null ? 100 - pressure.signals.availablePercent : null}
        status={pressureStatus(pressure.level)} tip="How hard Windows works to keep apps in RAM. High pressure causes slowdowns even when some RAM is technically free." />
    </div>

    <div className="rx-grid-side">
      <div className="rx-stack">
        <Card title="What FluxCore found" icon={Search} note={`Based on ${prediction.observationMinutes.toFixed(0)} min of measurements`}>
          <div className="rx-insight">
            <span className={`rx-insight-icon ${pressure.level === 'normal' ? 'success' : pressure.level === 'elevated' ? 'warning' : 'critical'}`}>
              {pressure.level === 'normal' ? <CheckCircle2 size={16} /> : <TriangleAlert size={16} />}
            </span>
            <div>
              <h3>{verdictFor(pressure.level)}</h3>
              <ul style={{ margin: '6px 0 0', paddingLeft: 18, fontSize: 12.5, color: 'var(--rx-text-secondary)' }}>
                {pressure.reasons.map((reason) => <li key={reason} style={{ marginBottom: 4 }}>{reason}</li>)}
              </ul>
            </div>
          </div>
        </Card>

        <Card title="Memory history" icon={Brain} right={<Tabs tabs={SERIES.map((entry) => ({ id: entry.id, label: entry.label }))} active={tab} onChange={(id) => setTab(id as SeriesId)} />}>
          {rows.filter((row) => row.value != null).length < 2
            ? <EmptyState title="Collecting memory history" detail="Samples are persisted every few seconds; charts appear once history exists." />
            : <ChartFrame ariaLabel={`Memory history chart: ${seriesMeta.label}`} caption={caption}>
                <ResponsiveContainer width="100%" height="100%">
                  <AreaChart data={rows} margin={{ top: 8, right: 8, bottom: 0, left: -18 }}>
                    <defs><linearGradient id="rxMiFill" x1="0" y1="0" x2="0" y2="1">
                      <stop offset="0%" stopColor="var(--rx-c-memory)" stopOpacity={0.26} />
                      <stop offset="100%" stopColor="var(--rx-c-memory)" stopOpacity={0} />
                    </linearGradient></defs>
                    <CartesianGrid {...rxGrid} />
                    <XAxis dataKey="label" {...rxAxis} tick={rxTick} minTickGap={44} />
                    <YAxis domain={[0, 'auto']} {...rxAxis} tick={rxTick} tickFormatter={(v) => `${Math.round(Number(v))}${unit}`} />
                    <Tooltip content={<RXTooltip unit={unit} />} />
                    <Area dataKey="value" name={seriesMeta.label} type="monotone" stroke="var(--rx-c-memory)" strokeWidth={2} fill="url(#rxMiFill)" isAnimationActive={false} connectNulls />
                  </AreaChart>
                </ResponsiveContainer>
              </ChartFrame>}
        </Card>

        <Card title="Optimization recommendations" icon={CheckCircle2}
          sub="Generated only from live measurements when pressure, commit charge or growth patterns justify them.">
          {intelligence.recommendations.length === 0
            ? <EmptyState title="No recommendations right now"
                detail="Recommendations appear when the analysis finds something actionable." />
            : <div className="rx-grid-2">{intelligence.recommendations.map((recommendation) => <RecommendationCard key={recommendation.id} recommendation={recommendation} />)}</div>}
        </Card>
      </div>

      <div className="rx-stack">
        <Card title="Prediction" icon={ArrowRight} note={`${Math.round(prediction.confidence * 100)}% confidence · estimate, not a measurement`}>
          <div className="rx-insight">
            <span className={`rx-insight-icon ${prediction.timeToHighPressureMinutes == null ? 'info' : 'warning'}`}><ArrowRight size={16} /></span>
            <div><h3>{prediction.timeToHighPressureMinutes == null ? 'Stable trend' : `High pressure projected in ~${prediction.timeToHighPressureMinutes.toFixed(0)} min`}</h3>
              <p>{prediction.summary}</p></div>
          </div>
          <DetailList rows={[
            { label: 'Horizon', value: `${prediction.horizonMinutes} min` },
            { label: 'Projected available', value: fmtBytes(prediction.availableForecast.projectedBytes == null ? null : Math.max(0, prediction.availableForecast.projectedBytes)) },
            { label: 'Available slope', value: mbPerMin(prediction.availableForecast.slopeBytesPerMin) },
          ]} />
          <p className="rx-chart-caption">{prediction.disclaimer}</p>
        </Card>

        <Card title="Highest memory consumers" icon={Search} note="Working set">
          <div className="rx-table-wrap"><table className="rx-table">
            <thead><tr><th>Process</th><th className="num">RAM</th><th className="num">Trend</th></tr></thead>
            <tbody>
              {topConsumers.map((process) => <tr key={process.pid}>
                <td><span className="rx-process-name">
                  <span className="rx-process-dot" style={{ background: process.leak ? 'var(--rx-danger)' : 'var(--rx-c-memory)' }} />
                  {process.name}
                  {process.leak && <span className="rx-chip critical" style={{ marginLeft: 6 }}>growth {process.leak.confidence}<InfoTip text="Memory use keeps growing without being released. FluxCore flags this pattern but never ends processes automatically." /></span>}
                </span></td>
                <td className="num">{fmtBytes(process.workingSetBytes)}</td>
                <td className="num" style={{ fontSize: 12 }}><TrendIcon direction={process.trendDirection} /></td>
              </tr>)}
            </tbody>
          </table></div>
          <p className="rx-chart-caption">Growth labels require sustained observations, never a single snapshot.</p>
        </Card>
      </div>
    </div>

    <Card>
      <Disclose summary="Technical memory counters">
        <div className="rx-disclose-body">
          <DetailList rows={[
            { label: 'Standby (reclaimable)', value: fmtBytes(analysis.standbyBytes), tip: 'Cached data returned to apps the moment they request memory.' },
            { label: 'Modified pages', value: fmtBytes(analysis.modifiedBytes), tip: 'Changed data waiting to be written to disk.' },
            { label: 'Compressed', value: compression.available ? fmtBytes(compression.compressedBytes) : 'Unavailable' },
            { label: 'Pagefile in use / capacity', value: `${fmtBytes(analysis.pagefileUsedBytes)} / ${fmtBytes(analysis.pagefileTotalBytes)}` },
            { label: 'Hard faults', value: analysis.hardFaultsPerSec == null ? 'Unavailable' : `${analysis.hardFaultsPerSec.toFixed(0)}/s` },
            { label: 'Commit percent', value: analysis.commitPercent == null ? 'Unavailable' : `${analysis.commitPercent.toFixed(0)}%` },
            { label: 'Data sources', value: analysis.sources.join(' · ') },
          ]} />
          {virtualMemory.notes.map((note) => <p className="rx-chart-caption" key={note}>{note}</p>)}
          {intelligence.limitations.length > 0 && <p className="rx-chart-caption">{intelligence.limitations.join(' ')}</p>}
        </div>
      </Disclose>
    </Card>
  </>
}
