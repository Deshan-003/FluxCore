import { useEffect, useState } from 'react'
import { Eye, Hand, Save, ShieldCheck, TriangleAlert } from 'lucide-react'
import { useMemoryStore } from '../stores/memory'
import {
  Button, Card, ConfirmDialog, DetailList, Disclose, EmptyState, PageHeader, StatusChip, Tabs,
  type ConfirmSpec,
} from '../ui/kit'
import { toast } from '../ui/toast-store'
import type { OptimizationMode, PolicyConfig } from '../types/memory'

const MODES: { id: OptimizationMode; name: string; icon: typeof Eye; body: string; allowed: string }[] = [
  {
    id: 'analysisOnly', name: 'Analysis only', icon: Eye,
    body: 'Monitors, detects pressure, analyzes applications and generates recommendations. Performs no actions at all.',
    allowed: 'Reads metrics only. No memory or system changes.',
  },
  {
    id: 'smartAssist', name: 'Smart assist', icon: ShieldCheck,
    body: 'Watches pressure continuously and suggests safe actions. Anything affecting another application or a system setting always asks first.',
    allowed: 'May run low-risk actions on its own; everything else requires your approval.',
  },
  {
    id: 'manual', name: 'Manual', icon: Hand,
    body: 'You review every available action and explicitly choose which ones run, with confirmation for each.',
    allowed: 'No automatic changes at all.',
  },
]

const HISTORY_TABS = [
  { id: 'actions', label: 'Actions' },
  { id: 'events', label: 'Pressure events' },
  { id: 'recommendations', label: 'Recommendations' },
] as const

export default function AdaptiveOptimizationPage() {
  const mode = useMemoryStore((state) => state.mode)
  const changeMode = useMemoryStore((state) => state.changeMode)
  const actions = useMemoryStore((state) => state.actions)
  const runAction = useMemoryStore((state) => state.runAction)
  const lastResult = useMemoryStore((state) => state.lastResult)
  const policyConfig = useMemoryStore((state) => state.policyConfig)
  const savePolicyConfig = useMemoryStore((state) => state.savePolicyConfig)
  const series = useMemoryStore((state) => state.series)
  const refreshSeries = useMemoryStore((state) => state.refreshSeries)

  const [confirm, setConfirm] = useState<ConfirmSpec | null>(null)
  const [draft, setDraft] = useState<PolicyConfig | null>(null)
  const [policyError, setPolicyError] = useState<string | null>(null)
  const [historyTab, setHistoryTab] = useState<typeof HISTORY_TABS[number]['id']>('actions')
  const [busyAction, setBusyAction] = useState<string | null>(null)

  useEffect(() => { void refreshSeries() }, [refreshSeries, historyTab])

  const requestMode = (id: OptimizationMode) => {
    if (id === mode) return
    const target = MODES.find((entry) => entry.id === id)!
    setConfirm({
      title: `Switch to "${target.name}"?`,
      body: target.body,
      facts: [{ kind: 'what', text: `The new mode applies immediately. Allowed to: ${target.allowed}` }],
      confirmLabel: 'Switch mode',
      onConfirm: () => {
        setConfirm(null)
        void changeMode(id).then(() => toast(`Optimization mode set to "${target.name}"`, 'success'))
      },
      onCancel: () => setConfirm(null),
    })
  }

  const runActionWithGuard = (id: string, requiresConfirmation: boolean) => {
    const descriptor = actions.find((action) => action.id === id)
    const start = async () => {
      setBusyAction(id)
      const result = await runAction(id, true)
      setBusyAction(null)
      if (result) toast(`"${descriptor?.title ?? id}": ${result.status} — ${result.detail}`, result.status === 'success' ? 'success' : 'error')
    }
    if (requiresConfirmation) {
      setConfirm({
        title: `Run "${descriptor?.title ?? id}"?`,
        body: descriptor?.description ?? '',
        facts: [
          { kind: 'what', text: descriptor?.sideEffects ?? 'The action runs immediately and is audited.' },
          { kind: 'risk', text: `${descriptor?.risk ?? 'low'} risk — FluxCore never terminates applications, purges standby lists, resizes the pagefile or disables Windows memory compression.` },
        ],
        confirmLabel: 'Run action',
        onConfirm: () => { setConfirm(null); void start() },
        onCancel: () => setConfirm(null),
      })
      return
    }
    void start()
  }

  const config = draft ?? policyConfig
  const saveConfig = async () => {
    if (!config) return
    const failure = await savePolicyConfig(config)
    setPolicyError(failure)
    if (!failure) { setDraft(null); toast('Policy configuration saved', 'success') }
  }
  const patchThreshold = (patch: Partial<PolicyConfig['thresholds']>) => {
    if (!config) return
    setDraft({ ...config, thresholds: { ...config.thresholds, ...patch } })
  }
  const patch = (value: Partial<PolicyConfig>) => { if (config) setDraft({ ...config, ...value }) }

  const numberField = (label: string, help: string, value: number, onChange: (value: number) => void, step = 1) =>
    <div className="rx-field" key={label}>
      <label>{label}</label>
      <input className="rx-input rx-number-input" type="number" step={step} value={value} aria-label={label}
        onChange={(event) => onChange(Number(event.target.value))} />
      <span className="help">{help}</span>
    </div>

  const actions_ = series?.actions ?? []
  const events = series?.pressureEvents ?? []
  const recommendations = series?.recommendations ?? []

  return <>
    <PageHeader title="Adaptive optimization"
      description="Choose how much FluxCore is allowed to do on its own — from watch-only to hands-on, always with a clear explanation." />

    <div className="rx-mode-grid">
      {MODES.map((entry) => {
        const Icon = entry.icon
        return <button key={entry.id} className={`rx-mode-card ${mode === entry.id ? 'active' : ''}`}
          onClick={() => requestMode(entry.id)} aria-pressed={mode === entry.id}>
          <h3><Icon size={15} />{entry.name}{mode === entry.id && <span className="rx-chip success" style={{ marginLeft: 'auto' }}>Active</span>}</h3>
          <p>{entry.body}</p>
          <p style={{ marginTop: 6, color: 'var(--rx-text-muted)' }}><strong>Allowed to:</strong> {entry.allowed}</p>
        </button>
      })}
    </div>

    <div className="rx-grid-side">
      <Card title="Recent optimization activity" icon={ShieldCheck}
        right={<Tabs tabs={HISTORY_TABS.map((entry) => ({ id: entry.id, label: entry.label }))} active={historyTab} onChange={(id) => setHistoryTab(id as typeof historyTab)} />}>
        {historyTab === 'actions' && <div className="rx-table-wrap"><table className="rx-table">
          <thead><tr><th>When</th><th>Mode</th><th>Action</th><th>Status</th><th>Detail</th></tr></thead>
          <tbody>
            {actions_.map((entry) => <tr key={entry.id}>
              <td style={{ whiteSpace: 'nowrap' }}>{new Date(entry.executedAt).toLocaleString()}</td>
              <td>{entry.mode}</td>
              <td><strong>{entry.actionId}</strong>{entry.confirmed ? '' : ' (auto)'}</td>
              <td><StatusChip status={entry.status === 'success' ? 'healthy' : entry.status === 'rejected' ? 'attention' : 'critical'} label={entry.status} /></td>
              <td style={{ color: 'var(--rx-text-secondary)' }}>{entry.detail}</td>
            </tr>)}
          </tbody>
        </table></div>}
        {historyTab === 'actions' && actions_.length === 0 && <EmptyState title="No actions recorded" detail="Every executed, rejected or failed action is audited here." />}
        {historyTab === 'events' && <div className="rx-stack" style={{ gap: 10 }}>
          {events.map((event) => <div className={`rx-card status-edge ${event.level === 'critical' || event.level === 'high' ? 'critical' : 'warning'}`} key={event.id}>
            <div className="rx-card-head"><h2>{pressureWord(event.level)} pressure</h2><div className="rx-card-head-right"><span className="rx-data-age">{new Date(event.startedAt).toLocaleString()} → {event.endedAt ? new Date(event.endedAt).toLocaleString() : 'ongoing'}</span></div></div>
            <p className="rx-card-sub" style={{ margin: 0 }}>{event.reason} · min available {(event.minAvailableBytes / 1024 ** 3).toFixed(1)} GB{event.peakCommitPercent == null ? '' : ` · peak commit ${event.peakCommitPercent.toFixed(0)}%`}</p>
          </div>)}
          {events.length === 0 && <EmptyState title="No pressure events yet" detail="Sustained elevated, high or critical pressure periods are recorded here." />}
        </div>}
        {historyTab === 'recommendations' && <div className="rx-stack" style={{ gap: 10 }}>
          {recommendations.map((entry) => <div className="rx-card status-edge info" key={entry.id}>
            <div className="rx-card-head"><h2>{entry.title}</h2><div className="rx-card-head-right"><span className="rx-data-age">{new Date(entry.createdAt).toLocaleString()}</span></div></div>
            <p className="rx-card-sub" style={{ margin: 0 }}>{entry.reason} · {entry.category} · {entry.risk} risk</p>
          </div>)}
          {recommendations.length === 0 && <EmptyState title="No stored recommendations" detail="Generated recommendations are persisted for review." />}
        </div>}
      </Card>

      <div className="rx-stack">
        <Card title="Safety model" icon={ShieldCheck}>
          <DetailList rows={[
            { label: 'Automatic scope', value: MODES.find((entry) => entry.id === mode)?.allowed ?? '' },
            { label: 'Approval required', value: 'Anything beyond low-risk actions', tip: 'Actions that affect other applications or system settings always ask first.' },
            { label: 'Never touches', value: 'Files, apps, processes', tip: 'FluxCore never terminates applications, trims working sets as a RAM-cleaning routine, purges standby lists, resizes the pagefile or disables Windows memory compression.' },
            { label: 'Full audit trail', value: 'Every action logged', tip: 'Each optimization is recorded with its mode, confirmation status and result.' },
          ]} />
        </Card>
        <Card title="Manual actions" icon={Hand}
          sub="Run an optimization yourself right now. Each action explains its effect and risk first.">
          {actions.length === 0 && <EmptyState title="Loading action registry" detail="The available actions appear after the memory engine responds." />}
          <div className="rx-stack" style={{ gap: 10 }}>
            {actions.map((action) => {
              const available = action.availableInModes.includes(mode)
              const result = lastResult?.actionId === action.id ? lastResult : null
              return <div key={action.id} style={{ display: 'flex', alignItems: 'flex-start', gap: 10, background: 'var(--rx-surface-2)', borderRadius: 8, padding: '10px 12px', opacity: available ? 1 : 0.6 }}>
                <TriangleAlert size={15} style={{ color: action.risk === 'low' ? 'var(--rx-success)' : 'var(--rx-warning)', marginTop: 2, flexShrink: 0 }} />
                <div style={{ flex: 1, minWidth: 0 }}>
                  <div style={{ fontSize: 12.5, fontWeight: 600 }}>{action.title}</div>
                  <div style={{ fontSize: 12, color: 'var(--rx-text-muted)' }}>{action.risk} risk · {action.requiresConfirmation ? 'asks for confirmation' : 'runs immediately'}{available ? '' : ' · not available in this mode'}</div>
                  {result && <div className="rx-chart-caption" role="status">{result.status}: {result.detail}</div>}
                </div>
                <Button small disabled={!available || busyAction === action.id} loading={busyAction === action.id}
                  onClick={() => runActionWithGuard(action.id, action.requiresConfirmation)}>Run</Button>
              </div>
            })}
          </div>
        </Card>
      </div>
    </div>

    <Card>
      <Disclose summary="Advanced: optimization thresholds">
        <div className="rx-disclose-body">
          <p className="rx-card-sub" style={{ margin: '0 0 12px' }}>
            These values control when the pressure engine escalates and how aggressively Smart assist acts. Defaults are safe for most systems.
            Hysteresis and minimum-duration rules prevent state flapping; aggressive decisions are never made from a single measurement.
          </p>
          {!config ? <EmptyState title="Policy configuration unavailable" detail="It appears once the memory engine has responded." /> : <>
            <div className="rx-detail-grid">
              {numberField('Elevated ≤ available %', 'First pressure level', config.thresholds.elevatedAvailablePercent, (value) => patchThreshold({ elevatedAvailablePercent: value }))}
              {numberField('High ≤ available %', 'Second pressure level', config.thresholds.highAvailablePercent, (value) => patchThreshold({ highAvailablePercent: value }))}
              {numberField('Critical ≤ available %', 'Highest pressure level', config.thresholds.criticalAvailablePercent, (value) => patchThreshold({ criticalAvailablePercent: value }))}
              {numberField('Elevated ≥ commit %', 'Commit-based trigger', config.thresholds.elevatedCommitPercent, (value) => patchThreshold({ elevatedCommitPercent: value }))}
              {numberField('High ≥ commit %', 'Commit-based trigger', config.thresholds.highCommitPercent, (value) => patchThreshold({ highCommitPercent: value }))}
              {numberField('Critical ≥ commit %', 'Commit-based trigger', config.thresholds.criticalCommitPercent, (value) => patchThreshold({ criticalCommitPercent: value }))}
              {numberField('Hard-fault threshold /s', 'Sustained paging signal', config.thresholds.hardFaultsHighPerSec, (value) => patchThreshold({ hardFaultsHighPerSec: value }), 50)}
              {numberField('Upgrade hysteresis (s)', 'Min time between upgrades', config.upgradeMinSeconds, (value) => patch({ upgradeMinSeconds: value }))}
              {numberField('Downgrade hysteresis (s)', 'Min time between downgrades', config.downgradeMinSeconds, (value) => patch({ downgradeMinSeconds: value }))}
              {numberField('Process scan (ms)', 'Detailed scan cadence', config.processScanIntervalMs, (value) => patch({ processScanIntervalMs: value }), 500)}
              {numberField('Idle after (s)', 'Idle policy trigger', config.idleAfterSeconds, (value) => patch({ idleAfterSeconds: value }))}
              {numberField('Prediction window (min)', 'Trend analysis window', config.predictionWindowMinutes, (value) => patch({ predictionWindowMinutes: value }))}
              {numberField('Retention (days)', 'History retention', config.retentionDays, (value) => patch({ retentionDays: value }))}
            </div>
            {policyError && <p className="rx-chart-caption" role="alert" style={{ color: 'var(--rx-danger)' }}>{policyError}</p>}
            <div style={{ marginTop: 12 }}>
              <Button variant="primary" icon={Save} onClick={() => void saveConfig()}>Save configuration</Button>
            </div>
          </>}
        </div>
      </Disclose>
    </Card>

    <ConfirmDialog spec={confirm} />
  </>
}

function pressureWord(level: string) {
  return level === 'normal' ? 'Normal' : level === 'elevated' ? 'Elevated' : level === 'high' ? 'High' : 'Critical'
}
