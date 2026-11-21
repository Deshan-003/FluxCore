import { create } from 'zustand'
import { DEFAULT_SETTINGS, getHistory, getSettings, getSnapshot, isDesktopRuntime, saveSettings } from '../services/monitoring'
import type { HistoryPoint, MonitoringSettings, SystemSnapshot } from '../types/monitoring'

const STORE_KEY = 'fluxcore.preferences.v1'
const MAX_POINTS = 1800
const HISTORY_REFRESH_MS = 5000

type LoadStatus = 'loading' | 'connected' | 'error' | 'web-preview'

interface MonitoringState {
  snapshot: SystemSnapshot | null
  history: HistoryPoint[]
  settings: MonitoringSettings
  status: LoadStatus
  error: string | null
  page: string
  search: string
  sortBy: 'cpu' | 'memory'
  initialize: () => Promise<void>
  refresh: () => Promise<void>
  updateSettings: (settings: MonitoringSettings) => Promise<void>
  setPage: (page: string) => void
  setSearch: (search: string) => void
  setSortBy: (sort: 'cpu' | 'memory') => void
}

function readPreferences(): MonitoringSettings {
  try {
    const saved = localStorage.getItem(STORE_KEY)
    if (saved) return { ...DEFAULT_SETTINGS, ...JSON.parse(saved) as Partial<MonitoringSettings> }
  } catch {
    // Corrupt local preferences fall back to defaults.
  }
  return DEFAULT_SETTINGS
}

export function toHistory(snapshot: SystemSnapshot): HistoryPoint {
  return {
    timestamp: snapshot.collectedAt,
    cpuPercent: snapshot.cpu.usagePercent,
    memoryPercent: snapshot.memory.usagePercent,
    gpuPercent: snapshot.gpus.find((gpu) => gpu.utilizationPercent !== null)?.utilizationPercent ?? null,
    vramUsedBytes: snapshot.gpus.reduce<number | null>((sum, gpu) => {
      if (gpu.dedicatedUsedBytes === null) return sum
      return (sum ?? 0) + gpu.dedicatedUsedBytes
    }, null),
    diskReadBytesPerSecond: snapshot.storage.reduce<number | null>((sum, disk) => disk.readBytesPerSecond === null ? sum : (sum ?? 0) + disk.readBytesPerSecond, null),
    diskWriteBytesPerSecond: snapshot.storage.reduce<number | null>((sum, disk) => disk.writeBytesPerSecond === null ? sum : (sum ?? 0) + disk.writeBytesPerSecond, null),
  }
}

function applySnapshot(snapshot: SystemSnapshot, appendInMemory: boolean) {
  const current = useMonitoringStore.getState()
  const history = appendInMemory
    ? [...current.history, toHistory(snapshot)].slice(-MAX_POINTS)
    : current.history
  useMonitoringStore.setState({
    snapshot,
    history,
    status: 'connected',
    error: snapshot.errors.length ? snapshot.errors.join(' ') : null,
  })
}

let eventUnlisten: (() => void) | null = null
let historyTimer: number | null = null
let pollTimer: number | null = null

async function subscribeToEvents() {
  const { listen } = await import('@tauri-apps/api/event')
  eventUnlisten = await listen<SystemSnapshot>('fluxcore-snapshot', (event) => {
    // With persistence on, SQLite rows are the single history source and are
    // re-read on a slow cadence; otherwise the live stream feeds the charts.
    applySnapshot(event.payload, !useMonitoringStore.getState().settings.persistHistory)
  })
  historyTimer = window.setInterval(async () => {
    const state = useMonitoringStore.getState()
    if (!state.settings.persistHistory || !state.settings.monitoringEnabled) return
    try {
      useMonitoringStore.setState({ history: await getHistory() })
    } catch {
      // SQLite hiccup: keep the history already in memory.
    }
  }, HISTORY_REFRESH_MS)
}

export function stopMonitoringSubscriptions() {
  eventUnlisten?.()
  eventUnlisten = null
  if (historyTimer !== null) {
    window.clearInterval(historyTimer)
    historyTimer = null
  }
  if (pollTimer !== null) {
    window.clearInterval(pollTimer)
    pollTimer = null
  }
}

export const useMonitoringStore = create<MonitoringState>((set, get) => ({
  snapshot: null,
  history: [],
  settings: readPreferences(),
  status: 'loading',
  error: null,
  page: 'Overview',
  search: '',
  sortBy: 'cpu',
  initialize: async () => {
    if (!isDesktopRuntime()) {
      set({ status: 'web-preview', error: 'Live system collection is available in the installed Windows desktop app. No sample metrics are shown in browser preview.' })
      return
    }
    try {
      const [settings, history] = await Promise.all([getSettings(), getHistory()])
      set({ settings, history, status: 'connected', error: null })
      await get().refresh()
      if (eventUnlisten === null) {
        try {
          await subscribeToEvents()
        } catch (error) {
          set({ error: `Snapshot events unavailable, falling back to polling: ${String(error)}` })
          pollTimer = window.setInterval(() => {
            const state = useMonitoringStore.getState()
            if (state.settings.monitoringEnabled) void state.refresh()
          }, get().settings.refreshIntervalMs)
        }
      }
    } catch (error) {
      set({ status: 'error', error: String(error) })
    }
  },
  refresh: async () => {
    try {
      const snapshot = await getSnapshot()
      applySnapshot(snapshot, !get().settings.persistHistory)
    } catch (error) {
      set({ status: 'error', error: String(error) })
    }
  },
  updateSettings: async (settings) => {
    const previous = get().settings
    set({ settings })
    localStorage.setItem(STORE_KEY, JSON.stringify(settings))
    if (isDesktopRuntime()) {
      try {
        await saveSettings(settings)
        await get().refresh()
        // The collector publishes a fresh snapshot on its next tick, so the value
        // just fetched can still carry the previous monitoring flag. Reflect the
        // saved setting immediately and let the next event confirm it.
        set((state) => ({
          error: null,
          snapshot: state.snapshot
            ? { ...state.snapshot, monitoring: settings.monitoringEnabled }
            : state.snapshot,
        }))
      } catch (error) {
        // The backend rejected the change: roll back to the stored values so
        // the UI never runs on settings the engine refused. Refresh first,
        // then surface the rejection so the snapshot cannot clear it.
        set({ settings: previous })
        localStorage.setItem(STORE_KEY, JSON.stringify(previous))
        await get().refresh()
        set({ error: `Settings change rejected: ${String(error)}` })
      }
    }
  },
  setPage: (page) => set({ page }),
  setSearch: (search) => set({ search }),
  setSortBy: (sortBy) => set({ sortBy }),
}))
