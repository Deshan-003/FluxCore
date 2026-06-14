import { create } from 'zustand'
import type {
  AccelerationConfig, AccelerationMode, BenchmarkRecord, BenchmarkRequest,
  GpuAccelerationState, GpuTask, SubmitTaskRequest,
} from '../types/gpu'
import {
  cancelTask, clearBenchmarkHistory, clearFinishedTasks, getBenchmarkHistory,
  getGpuAccelerationState, getGpuResourceConfig, getTaskHistory, probeGpuProviders,
  runBenchmark, setAccelerationMode, submitWorkloadTask, updateGpuResourceConfig,
} from '../services/gpu'

const PREFS_KEY = 'fluxcore.gpu.preferences.v1'

export interface GpuPreferences {
  lastTab: string
  selectedSizes: Record<string, number>
  autoRefresh: boolean
}

const DEFAULT_PREFS: GpuPreferences = { lastTab: 'overview', selectedSizes: {}, autoRefresh: true }

function loadPrefs(): GpuPreferences {
  try {
    const raw = localStorage.getItem(PREFS_KEY)
    if (!raw) return DEFAULT_PREFS
    return { ...DEFAULT_PREFS, ...(JSON.parse(raw) as Partial<GpuPreferences>) }
  } catch {
    return DEFAULT_PREFS
  }
}

function savePrefs(prefs: GpuPreferences) {
  try {
    localStorage.setItem(PREFS_KEY, JSON.stringify(prefs))
  } catch {
    // Web preview / private mode: preferences are best-effort only.
  }
}

interface GpuStore {
  state: GpuAccelerationState | null
  config: AccelerationConfig | null
  benchmarks: BenchmarkRecord[]
  history: GpuTask[]
  loading: boolean
  benchmarkBusy: boolean
  error: string | null
  prefs: GpuPreferences
  refresh: () => Promise<void>
  setMode: (mode: AccelerationMode) => Promise<void>
  saveConfig: (config: AccelerationConfig) => Promise<void>
  submit: (request: SubmitTaskRequest) => Promise<GpuTask | null>
  cancel: (taskId: number) => Promise<void>
  clearFinished: () => Promise<void>
  runBenchmark: (request: BenchmarkRequest) => Promise<BenchmarkRecord | null>
  probe: (force: boolean) => Promise<void>
  loadHistory: () => Promise<void>
  loadBenchmarks: () => Promise<void>
  clearBenchmarks: () => Promise<void>
  setPrefs: (patch: Partial<GpuPreferences>) => void
  applyTaskEvent: (task: GpuTask) => void
}

function message(error: unknown): string {
  return error instanceof Error ? error.message : String(error)
}

export const useGpuStore = create<GpuStore>()((set, get) => ({
  state: null,
  config: null,
  benchmarks: [],
  history: [],
  loading: false,
  benchmarkBusy: false,
  error: null,
  prefs: loadPrefs(),

  refresh: async () => {
    set({ loading: true })
    try {
      const [state, config] = await Promise.all([getGpuAccelerationState(), getGpuResourceConfig()])
      set({ state, config, error: null })
    } catch (error) {
      set({ error: message(error) })
    } finally {
      set({ loading: false })
    }
  },

  setMode: async (mode) => {
    try {
      const state = await setAccelerationMode(mode)
      set({ state, error: null })
      await get().refresh()
    } catch (error) {
      set({ error: message(error) })
    }
  },

  saveConfig: async (config) => {
    try {
      const stored = await updateGpuResourceConfig(config)
      set({ config: stored, error: null })
      await get().refresh()
    } catch (error) {
      set({ error: message(error) })
    }
  },

  submit: async (request) => {
    try {
      const task = await submitWorkloadTask(request)
      set({ error: null })
      await get().refresh()
      return task
    } catch (error) {
      set({ error: message(error) })
      return null
    }
  },

  cancel: async (taskId) => {
    try {
      await cancelTask(taskId)
      set({ error: null })
      await get().refresh()
    } catch (error) {
      set({ error: message(error) })
    }
  },

  clearFinished: async () => {
    try {
      await clearFinishedTasks()
      await get().refresh()
    } catch (error) {
      set({ error: message(error) })
    }
  },

  runBenchmark: async (request) => {
    set({ benchmarkBusy: true })
    try {
      const record = await runBenchmark(request)
      set({ error: null })
      await get().loadBenchmarks()
      await get().refresh()
      return record
    } catch (error) {
      set({ error: message(error) })
      return null
    } finally {
      set({ benchmarkBusy: false })
    }
