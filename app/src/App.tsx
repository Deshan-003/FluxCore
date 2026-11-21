import { useEffect, useMemo, useState, type ComponentType } from 'react'
import {
  Brain, BrainCircuit, Cpu, HardDrive, LayoutDashboard, MemoryStick, Microchip,
  MonitorCog, RefreshCw, Settings as SettingsIcon, ShieldCheck, Sparkles, SquareActivity, Zap,
} from 'lucide-react'
import { useMonitoringStore } from './stores/monitoring'
import { useMemoryStore } from './stores/memory'
import { useResourceStore } from './stores/intelligence'
import { IconButton, StatusChip, ToastHost } from './ui/kit'
import { ageOf } from './lib/format'
import Dashboard from './pages/Dashboard'
import CpuPage from './pages/Cpu'
import MemoryPage from './pages/Memory'
import GpuPage from './pages/Gpu'
import StoragePage from './pages/Storage'
import ProcessesPage from './pages/Processes'
import HardwarePage from './pages/Hardware'
import MemoryManagementPage from './pages/MemoryManagement'
import AdaptiveOptimizationPage from './pages/AdaptiveOptimization'
import GpuAccelerationPage from './pages/GpuAcceleration'
import ResourceIntelligencePage from './pages/ResourceIntelligence'
import SettingsPage from './pages/Settings'
import './ui/design-system.css'

// Stable page ids are stored in the monitoring store; labels are display-only
// so renaming navigation never breaks state or deep links.
interface NavEntry { id: string; label: string; icon: ComponentType<{ size?: number | string }> }
const NAV: { group: string; items: NavEntry[] }[] = [
  { group: 'Overview', items: [{ id: 'dashboard', label: 'Dashboard', icon: LayoutDashboard }] },
  {
    group: 'Monitoring', items: [
      { id: 'cpu', label: 'CPU', icon: Cpu },
      { id: 'memory', label: 'Memory', icon: MemoryStick },
      { id: 'gpu', label: 'GPU', icon: MonitorCog },
      { id: 'storage', label: 'Storage', icon: HardDrive },
      { id: 'processes', label: 'Processes', icon: SquareActivity },
      { id: 'hardware', label: 'Hardware Info', icon: Microchip },
    ],
  },
  {
    group: 'Optimization', items: [
      { id: 'memory-management', label: 'Memory Management', icon: Brain },
      { id: 'adaptive-optimization', label: 'Adaptive Optimization', icon: Sparkles },
      { id: 'gpu-acceleration', label: 'GPU Acceleration', icon: Zap },
    ],
  },
  { group: 'Intelligence', items: [{ id: 'resource-intelligence', label: 'Resource Intelligence', icon: BrainCircuit }] },
  { group: 'System', items: [{ id: 'settings', label: 'Settings', icon: SettingsIcon }] },
]

const PAGE_META: Record<string, { group: string; label: string }> = {}
for (const group of NAV) for (const item of group.items) PAGE_META[item.id] = { group: group.group, label: item.label }

function App() {
  const snapshot = useMonitoringStore((state) => state.snapshot)
  const settings = useMonitoringStore((state) => state.settings)
  const status = useMonitoringStore((state) => state.status)
  const error = useMonitoringStore((state) => state.error)
  const page = useMonitoringStore((state) => state.page)
  const setPage = useMonitoringStore((state) => state.setPage)
  const refresh = useMonitoringStore((state) => state.refresh)
  const initialize = useMonitoringStore((state) => state.initialize)
  const memoryRefresh = useMemoryStore((state) => state.refresh)
  const reportActive = useMemoryStore((state) => state.reportActive)
  const [collapsed, setCollapsed] = useState(false)
  const [now, setNow] = useState(() => Date.now())

  useEffect(() => { void initialize() }, [initialize])

  // Resource Intelligence analyses only while its page is visible and connected.
  useEffect(() => {
    if (status === 'connected') void useResourceStore.getState().refresh()
  }, [status])

  useEffect(() => {
    const visible = page === 'resource-intelligence' && !document.hidden
    if (!visible) return
    const timer = window.setInterval(() => {
      if (!document.hidden) void useResourceStore.getState().refresh()
    }, 15000)
    return () => window.clearInterval(timer)
  }, [page])

  // Memory intelligence ticks with the monitoring cadence while connected.
  useEffect(() => {
    if (status !== 'connected') return
    const timer = window.setInterval(() => void memoryRefresh(), Math.max(2000, settings.refreshIntervalMs))
    void memoryRefresh()
    return () => window.clearInterval(timer)
  }, [status, settings.refreshIntervalMs, memoryRefresh])

  // Idle-policy heartbeat: the memory engine slows down when the optimization
  // pages are not being looked at.
  const intelligenceVisible = page === 'memory-management' || page === 'adaptive-optimization'
  useEffect(() => {
    const active = intelligenceVisible && !document.hidden
    void reportActive(active)
    if (!active) return
    const heartbeat = window.setInterval(() => void reportActive(true), 30000)
    const onVisibility = () => void reportActive(intelligenceVisible && !document.hidden)
    document.addEventListener('visibilitychange', onVisibility)
    return () => { window.clearInterval(heartbeat); document.removeEventListener('visibilitychange', onVisibility) }
  }, [intelligenceVisible, reportActive])

  // Data-age caption ticks so "Updated 5s ago" stays truthful without re-rendering charts.
  useEffect(() => {
    const timer = window.setInterval(() => setNow(Date.now()), 5000)
    return () => window.clearInterval(timer)
  }, [])

  // Real curated themes (no filter hack): dark / light / system.
  const theme = useMemo(() => {
    if (settings.theme !== 'system') return settings.theme
    return window.matchMedia?.('(prefers-color-scheme: light)').matches ? 'light' : 'dark'
  }, [settings.theme])
  useEffect(() => {
    const media = window.matchMedia?.('(prefers-color-scheme: light)')
    const onChange = () => setNow(Date.now())
    media?.addEventListener('change', onChange)
    return () => media?.removeEventListener('change', onChange)
  }, [])

  const meta = PAGE_META[page] ?? PAGE_META.dashboard
  const statusLabel = status === 'connected'
    ? snapshot?.monitoring && settings.monitoringEnabled ? 'Monitoring active' : 'Monitoring paused'
    : status === 'web-preview' ? 'Desktop app required'
    : status === 'error' ? 'Connection issue'
    : 'Starting engine'
  const statusKind = status === 'connected' && snapshot?.monitoring && settings.monitoringEnabled ? 'healthy'
    : status === 'error' ? 'critical' : status === 'web-preview' ? 'attention' : 'unavailable'

  const pageContent = () => {
    if (!snapshot) {
      return <div className="rx-card"><div className="rx-empty">
        <ShieldCheck size={30} />
        <h3>{status === 'web-preview' ? 'Open FluxCore for Windows' : status === 'error' ? 'Monitoring engine unavailable' : 'Connecting to Windows monitoring'}</h3>
        <p>{error ?? 'Waiting for the first hardware sample. No sample metrics are shown outside the desktop app — nothing is estimated.'}</p>
      </div></div>
    }
    switch (page) {
      case 'cpu': return <CpuPage />
      case 'memory': return <MemoryPage />
      case 'gpu': return <GpuPage />
      case 'storage': return <StoragePage />
      case 'memory-management': return <MemoryManagementPage />
      case 'adaptive-optimization': return <AdaptiveOptimizationPage />
      case 'gpu-acceleration': return <GpuAccelerationPage />
      case 'resource-intelligence': return <ResourceIntelligencePage />
      case 'processes': return <ProcessesPage />
      case 'hardware': return <HardwarePage />
      case 'settings': return <SettingsPage />
      default: return <Dashboard onNavigate={setPage} />
    }
  }

  return <div className="rx-root" data-theme={theme}>
    <aside className={`rx-sidebar ${collapsed ? 'collapsed' : ''}`}>
      <div className="rx-brand">
        <div className="rx-brand-mark">FX</div>
        {!collapsed && <div className="rx-brand-name"><strong>FluxCore</strong><span>for Windows</span></div>}
        <button className="rx-collapse" aria-label={collapsed ? 'Expand sidebar' : 'Collapse sidebar'} onClick={() => setCollapsed(!collapsed)}>
          <svg width="14" height="14" viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.6" style={{ transform: collapsed ? 'rotate(180deg)' : 'none' }}>
            <path d="M10 3 5 8l5 5" />
          </svg>
        </button>
      </div>
      <nav className="rx-nav" aria-label="Primary">
        {NAV.map((group) => <div className="rx-nav-group" key={group.group}>
          {!collapsed && <div className="rx-nav-label">{group.group}</div>}
          {group.items.map((item) => {
            const Icon = item.icon
            const active = page === item.id
            return <button key={item.id} className={`rx-nav-item ${active ? 'active' : ''}`}
              onClick={() => setPage(item.id)} title={collapsed ? item.label : undefined} aria-current={active ? 'page' : undefined}>
              <Icon size={16} />
              {!collapsed && <span>{item.label}</span>}
              {!collapsed && item.id === 'processes' && snapshot && snapshot.processes.length > 0 && (
                <span className="rx-nav-count">{snapshot.processes.length}</span>
              )}
            </button>
          })}
        </div>)}
      </nav>
      <div className="rx-sidebar-foot">{collapsed ? '1.0' : 'FluxCore 1.0.0 · Windows'}</div>
    </aside>

    <div className="rx-main">
      <header className="rx-topbar">
        <span className="rx-topbar-context">{meta.group} · <strong>{meta.label}</strong></span>
        <div className="rx-topbar-right">
          <span data-testid="collector-state"><StatusChip status={statusKind} label={statusLabel} /></span>
          <span className="rx-data-age">{snapshot ? `Updated ${ageOf(snapshot.collectedAt, now)}` : 'Waiting for data'}</span>
          <IconButton icon={RefreshCw} label="Refresh metrics" onClick={() => void refresh()} />
        </div>
      </header>

      <main className="rx-content" id="rx-content">
        <div style={{ maxWidth: 1280, margin: '0 auto', display: 'flex', flexDirection: 'column', gap: 20 }}>
          {error && status === 'connected' && <div className="rx-card status-edge warning" role="alert">
            <div className="rx-card-head"><h2>Collector notice</h2></div>
            <p className="rx-card-sub" style={{ margin: 0 }}>{error}</p>
          </div>}
          {pageContent()}
        </div>
      </main>

      <footer className="rx-statusbar">
        <span>Local collection · No cloud upload</span>
        <span>Unavailable sensors are never estimated</span>
        <span>FluxCore 1.0.0</span>
      </footer>
    </div>

    <ToastHost />
  </div>
}

export default App
