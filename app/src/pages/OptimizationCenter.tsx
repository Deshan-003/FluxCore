import { useEffect, useState } from 'react'
import {
  AlertTriangle, Ban, ClipboardList, Eye, FileClock, Hand, History, Save, Settings2, ShieldCheck,
} from 'lucide-react'
import { useMemoryStore } from '../stores/memory'
import type { OptimizationMode, PolicyConfig } from '../types/memory'

const MODES: { id: OptimizationMode; title: string; icon: React.ElementType; description: string }[] = [
  { id: 'analysisOnly', title: 'Analysis Only', icon: Eye, description: 'Monitors, detects pressure, analyzes applications and generates recommendations. Performs no actions at all.' },
  { id: 'smartAssist', title: 'Smart Assist', icon: ShieldCheck, description: 'Watches pressure continuously and suggests safe actions. Anything affecting another application or a system setting always asks first.' },
  { id: 'manual', title: 'Manual Optimization', icon: Hand, description: 'You review every available action and explicitly choose which ones run, with confirmation for each.' },
]

function ModeSelector() {
  const { mode, changeMode } = useMemoryStore()
  return <section className="panel"><div className="panel-head"><div className="panel-title"><Settings2 size={16} /><h2>Operating mode</h2></div></div>
    <div className="mode-grid">{MODES.map((entry) => <button key={entry.id} className={`mode-card ${mode === entry.id ? 'active' : ''}`} onClick={() => void changeMode(entry.id)}>
      <entry.icon size={17} /><strong>{entry.title}</strong><span>{entry.description}</span>{mode === entry.id && <i className="mode-active">ACTIVE</i>}
    </button>)}</div>
  </section>
}

function PolicyPanel() {
  const intelligence = useMemoryStore((state) => state.intelligence)
  const policyConfig = useMemoryStore((state) => state.policyConfig)
  const savePolicyConfig = useMemoryStore((state) => state.savePolicyConfig)
  const [draft, setDraft] = useState<PolicyConfig | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [saved, setSaved] = useState(false)
  const config = draft ?? policyConfig
  if (!config) return null
  const set = (patch: Partial<PolicyConfig>) => { setDraft({ ...config, ...patch }); setSaved(false) }
  const setThreshold = (patch: Partial<PolicyConfig['thresholds']>) => { setDraft({ ...config, thresholds: { ...config.thresholds, ...patch } }); setSaved(false) }
  const save = async () => {
    const failure = await savePolicyConfig(config)
    setError(failure)
    if (!failure) { setDraft(null); setSaved(true) }
  }
  const numberField = (label: string, value: number, onChange: (value: number) => void, step = 1) =>
    <label className="policy-field"><span>{label}</span><input type="number" value={value} step={step} onChange={(event) => onChange(Number(event.target.value))} /></label>
  return <section className="panel"><div className="panel-head"><div className="panel-title"><ClipboardList size={16} /><h2>Active policies</h2></div>{intelligence && <span className="chart-period">{intelligence.policies.dashboardActive ? 'dashboard active' : 'idle policy engaged'}</span>}</div>
    <div className="policy-list">{(intelligence?.policies.active ?? []).map((policy) => <div className="policy-row" key={policy.id}><strong>{policy.title}</strong><span>{policy.detail}</span></div>)}</div>
    <div className="panel-head sub"><div className="panel-title"><Settings2 size={15} /><h2>Policy configuration</h2></div>{saved && <span className="chart-period">saved</span>}</div>
    <div className="policy-grid">
      {numberField('Elevated ≤ available %', config.thresholds.elevatedAvailablePercent, (value) => setThreshold({ elevatedAvailablePercent: value }))}
      {numberField('High ≤ available %', config.thresholds.highAvailablePercent, (value) => setThreshold({ highAvailablePercent: value }))}
      {numberField('Critical ≤ available %', config.thresholds.criticalAvailablePercent, (value) => setThreshold({ criticalAvailablePercent: value }))}
      {numberField('Elevated ≥ commit %', config.thresholds.elevatedCommitPercent, (value) => setThreshold({ elevatedCommitPercent: value }))}
      {numberField('High ≥ commit %', config.thresholds.highCommitPercent, (value) => setThreshold({ highCommitPercent: value }))}
      {numberField('Critical ≥ commit %', config.thresholds.criticalCommitPercent, (value) => setThreshold({ criticalCommitPercent: value }))}
      {numberField('Hard-fault threshold /s', config.thresholds.hardFaultsHighPerSec, (value) => setThreshold({ hardFaultsHighPerSec: value }), 50)}
      {numberField('Upgrade hysteresis (s)', config.upgradeMinSeconds, (value) => set({ upgradeMinSeconds: value }))}
      {numberField('Downgrade hysteresis (s)', config.downgradeMinSeconds, (value) => set({ downgradeMinSeconds: value }))}
      {numberField('Process scan (ms)', config.processScanIntervalMs, (value) => set({ processScanIntervalMs: value }), 500)}
      {numberField('Persist interval (ms)', config.persistIntervalMs, (value) => set({ persistIntervalMs: value }), 1000)}
      {numberField('Process persist (ms)', config.processPersistIntervalMs, (value) => set({ processPersistIntervalMs: value }), 5000)}
      {numberField('Idle after (s)', config.idleAfterSeconds, (value) => set({ idleAfterSeconds: value }))}
      {numberField('Idle refresh (ms)', config.idleRefreshIntervalMs, (value) => set({ idleRefreshIntervalMs: value }), 1000)}
      {numberField('Prediction window (min)', config.predictionWindowMinutes, (value) => set({ predictionWindowMinutes: value }))}
      {numberField('Retention (days)', config.retentionDays, (value) => set({ retentionDays: value }))}
    </div>
    {error && <div className="inline-warning"><AlertTriangle size={15} />{error}</div>}
    <div className="policy-actions"><button className="diagnostic-button" onClick={() => void save()}><Save size={14} />Save configuration</button><span className="muted-note">Hysteresis and minimum-duration rules prevent state flapping; aggressive decisions are never made from a single measurement.</span></div>
  </section>
}

function ManualActions() {
  const { actions, mode, runAction, lastResult } = useMemoryStore()
  const [pending, setPending] = useState<string | null>(null)
  const run = async (id: string, requiresConfirmation: boolean) => {
    if (requiresConfirmation && pending !== id) { setPending(id); return }
    setPending(null)
    await runAction(id, true)
  }
  return <section className="panel"><div className="panel-head"><div className="panel-title"><Hand size={16} /><h2>Optimization actions</h2></div><span className="chart-period">mode: {mode}</span></div>
    <div className="action-list">{actions.map((action) => {
      const available = action.availableInModes.includes(mode)
      const result = lastResult?.actionId === action.id ? lastResult : null
      return <div className={`action-row ${available ? '' : 'disabled'}`} key={action.id}>
        <div className="action-info"><strong>{action.title}</strong><span>{action.description}</span><small>Side effects: {action.sideEffects}</small></div>
        <div className="action-side">
          <span className={`risk-badge ${action.risk}`}>{action.risk} risk</span>
          <button className={`rec-action ${pending === action.id ? 'confirming' : ''}`} disabled={!available} onClick={() => void run(action.id, action.requiresConfirmation)} title={!available ? 'Not available in the current mode' : undefined}>
            {pending === action.id ? 'Confirm run' : action.requiresConfirmation ? 'Run with confirmation' : 'Run'}
          </button>
        </div>
        {result && <p className={`rec-result ${result.status}`}>{result.status}: {result.detail}</p>}
      </div>
    })}</div>
    <div className="sensor-notice"><Ban size={15} /><span>FluxCore never terminates applications, trims working sets as a RAM-cleaning routine, purges standby lists, resizes the pagefile or disables Windows memory compression. Those operations are not part of the action registry and are rejected by the safety engine.</span></div>
  </section>
}

function OptimizationHistory() {
  const series = useMemoryStore((state) => state.series)
  const refreshSeries = useMemoryStore((state) => state.refreshSeries)
  const [tab, setTab] = useState<'actions' | 'events' | 'recommendations'>('actions')
  useEffect(() => {
    void refreshSeries()
  }, [refreshSeries, tab])
  const actions = series?.actions ?? []
  const events = series?.pressureEvents ?? []
  const recommendations = series?.recommendations ?? []
  return <section className="panel"><div className="panel-head"><div className="panel-title"><History size={16} /><h2>Optimization history</h2></div>
    <div className="history-tabs">{(['actions', 'events', 'recommendations'] as const).map((key) => <button key={key} className={`history-tab ${tab === key ? 'active' : ''}`} onClick={() => setTab(key)}>{key}</button>)}</div></div>
    {tab === 'actions' && <div className="process-table-wrap"><table className="process-table"><thead><tr><th>WHEN</th><th>MODE</th><th>ACTION</th><th>CONFIRMED</th><th>STATUS</th><th>DETAIL</th></tr></thead>
      <tbody>{actions.map((entry) => <tr key={entry.id}><td>{new Date(entry.executedAt).toLocaleString()}</td><td>{entry.mode}</td><td>{entry.actionId}</td><td>{entry.confirmed ? 'yes' : 'no'}</td><td><span className={`status-chip ${entry.status}`}>{entry.status}</span></td><td className="audit-detail">{entry.detail}</td></tr>)}</tbody></table>
      {actions.length === 0 && <div className="empty-state"><strong>No actions recorded</strong><span>Every executed, rejected or failed action is audited here.</span></div>}</div>}
    {tab === 'events' && <div className="event-list">{events.map((event) => <div className={`event-row ${event.level}`} key={event.id}><FileClock size={14} /><div><strong>{event.level} pressure</strong><span>{event.reason}</span><small>{new Date(event.startedAt).toLocaleString()} → {event.endedAt ? new Date(event.endedAt).toLocaleString() : 'ongoing'} · min available {(event.minAvailableBytes / 1024 ** 3).toFixed(1)} GiB{event.peakCommitPercent == null ? '' : ` · peak commit ${event.peakCommitPercent.toFixed(0)}%`}</small></div></div>)}
      {events.length === 0 && <div className="empty-state"><strong>No pressure events yet</strong><span>Sustained elevated, high or critical pressure periods are recorded here.</span></div>}</div>}
    {tab === 'recommendations' && <div className="event-list">{recommendations.map((entry) => <div className="event-row stored" key={entry.id}><ClipboardList size={14} /><div><strong>{entry.title}</strong><span>{entry.reason}</span><small>{new Date(entry.createdAt).toLocaleString()} · {entry.category} · {entry.risk} risk</small></div></div>)}
      {recommendations.length === 0 && <div className="empty-state"><strong>No stored recommendations</strong><span>Generated recommendations are persisted for review.</span></div>}</div>}
  </section>
}

export default function OptimizationCenterPage() {
  return <div className="oc-page">
    <ModeSelector />
    <div className="mi-grid-2"><PolicyPanel /><ManualActions /></div>
    <OptimizationHistory />
  </div>
}
