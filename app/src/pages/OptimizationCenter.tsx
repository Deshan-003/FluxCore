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
