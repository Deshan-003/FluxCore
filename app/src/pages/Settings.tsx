import { Moon, ShieldCheck, SlidersHorizontal, Monitor } from 'lucide-react'
import { useMonitoringStore } from '../stores/monitoring'
import {
  Button, Card, PageHeader, SettingRow, StatusChip, Switch, ToggleGroup,
} from '../ui/kit'
import { toast } from '../ui/toast-store'
import type { MonitoringSettings } from '../types/monitoring'

export default function SettingsPage() {
  const settings = useMonitoringStore((state) => state.settings)
  const updateSettings = useMonitoringStore((state) => state.updateSettings)
  const error = useMonitoringStore((state) => state.error)
  const patch = (values: Partial<MonitoringSettings>) => void updateSettings({ ...settings, ...values })

  return <>
    <PageHeader title="Settings" description="Tune how FluxCore collects and presents data. Changes apply immediately; the backend validates them and rejected values roll back automatically." />

    {error && <div className="rx-card status-edge critical" role="alert">
      <div className="rx-card-head"><h2>The last change was not applied</h2></div>
      <p className="rx-card-sub" style={{ margin: 0 }}>{error}</p>
    </div>}

    <Card title="Appearance" icon={Moon} sub="Dark is the default. Light and System use the same curated design tokens — no color-inversion tricks.">
      <SettingRow title="Theme" description="Choose the application color scheme."
        control={<ToggleGroup
          options={[{ id: 'dark', label: 'Dark' }, { id: 'light', label: 'Light' }, { id: 'system', label: 'System' }]}
          value={settings.theme}
          onChange={(id) => { patch({ theme: id as MonitoringSettings['theme'] }); toast(`Theme set to "${id}"`, 'info') }} />} />
    </Card>

    <Card title="Monitoring" icon={SlidersHorizontal}>
      <SettingRow title="Enable monitoring" description="Pause collection while keeping the application open. Nothing is measured while paused."
        tip="When paused, the status chip at the top shows “Monitoring paused” and charts stop receiving samples."
        control={<Switch checked={settings.monitoringEnabled} onChange={(checked) => patch({ monitoringEnabled: checked })} label="Enable monitoring" />} />
      <SettingRow title="Refresh interval" description="Controls live metric polling frequency (250 ms to 60 s)."
        control={<select className="rx-select" value={settings.refreshIntervalMs} aria-label="Refresh interval"
          onChange={(event) => patch({ refreshIntervalMs: Number(event.target.value) })}>
          <option value={500}>0.5 seconds</option>
          <option value={1000}>1 second</option>
          <option value={2000}>2 seconds</option>
          <option value={5000}>5 seconds</option>
          <option value={10000}>10 seconds</option>
        </select>} />
      <SettingRow title="Chart history window" description="How much time is visible in live charts (5 minutes to 6 hours)."
        control={<select className="rx-select" value={settings.historyDurationMinutes} aria-label="Chart history window"
          onChange={(event) => patch({ historyDurationMinutes: Number(event.target.value) })}>
          <option value={5}>5 minutes</option>
          <option value={10}>10 minutes</option>
          <option value={30}>30 minutes</option>
          <option value={60}>1 hour</option>
          <option value={360}>6 hours</option>
        </select>} />
      <SettingRow title="Persist history" description="Store a sample every 5 seconds in the local SQLite database (7-day retention) so charts survive restarts."
        control={<Switch checked={settings.persistHistory} onChange={(checked) => patch({ persistHistory: checked })} label="Persist history" />} />
    </Card>

    <Card title="Integration" icon={Monitor}>
      <SettingRow planned title="Start with Windows" description="Launch FluxCore automatically when you sign in. Startup registration is not enabled in this phase — the preference is stored for a future release."
        control={<Switch checked={settings.startOnLogin} onChange={(checked) => patch({ startOnLogin: checked })} label="Start with Windows" />} />
      <SettingRow planned title="Minimize to system tray" description="Keep FluxCore running in the tray with the window closed. Tray support is not configured in this phase."
        control={<Switch checked={settings.minimizeToTray} onChange={(checked) => patch({ minimizeToTray: checked })} label="Minimize to tray" />} />
    </Card>

    <Card title="About & privacy" icon={ShieldCheck}>
      <SettingRow title="Your data stays on this PC" description="FluxCore collects metrics locally and never uploads them. There is no cloud component, account or telemetry."
        control={<StatusChip status="healthy" label="Local only" />} />
      <SettingRow title="Version" description="FluxCore 1.0.0 · Windows · Tauri 2"
        control={<Button small onClick={() => toast('FluxCore is up to date', 'success')}>Check for updates</Button>} />
    </Card>
  </>
}
