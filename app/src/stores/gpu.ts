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
