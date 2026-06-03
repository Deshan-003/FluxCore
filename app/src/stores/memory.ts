import { create } from 'zustand'
import {
  executeOptimizationAction, getMemoryHistorySeries, getMemoryIntelligence, getMemoryPolicyConfig,
  getOptimizationActions, getOptimizationMode, setDashboardActive, setOptimizationMode,
  updateMemoryPolicyConfig,
} from '../services/memory'
import { isDesktopRuntime } from '../services/monitoring'
import type {
  ActionDescriptor, ActionResult, MemoryHistorySeries, MemoryIntelligenceSnapshot,
  OptimizationMode, PolicyConfig,
} from '../types/memory'

interface MemoryState {
  intelligence: MemoryIntelligenceSnapshot | null
  series: MemoryHistorySeries | null
  actions: ActionDescriptor[]
  mode: OptimizationMode
  policyConfig: PolicyConfig | null
  lastResult: ActionResult | null
  error: string | null
  refresh: () => Promise<void>
  refreshSeries: () => Promise<void>
  changeMode: (mode: OptimizationMode) => Promise<void>
  runAction: (actionId: string, confirmed: boolean) => Promise<ActionResult | null>
  savePolicyConfig: (config: PolicyConfig) => Promise<string | null>
  reportActive: (active: boolean) => Promise<void>
}

export const useMemoryStore = create<MemoryState>((set, get) => ({
  intelligence: null,
  series: null,
  actions: [],
  mode: 'analysisOnly',
  policyConfig: null,
  lastResult: null,
  error: null,
  refresh: async () => {
    if (!isDesktopRuntime()) return
    try {
      const [intelligence, mode, actions, policyConfig] = await Promise.all([
        getMemoryIntelligence(),
        getOptimizationMode(),
        get().actions.length ? Promise.resolve(get().actions) : getOptimizationActions(),
        get().policyConfig ? Promise.resolve(get().policyConfig) : getMemoryPolicyConfig(),
      ])
      set({ intelligence, mode, actions, policyConfig, error: null })
    } catch (error) {
      set({ error: String(error) })
    }
  },
  refreshSeries: async () => {
    if (!isDesktopRuntime()) return
    try {
      const series = await getMemoryHistorySeries(720)
      set({ series })
    } catch {
      // History charts stay empty until SQLite has samples; not fatal.
    }
  },
  changeMode: async (mode) => {
    set({ mode })
    if (!isDesktopRuntime()) return
    try {
      await setOptimizationMode(mode)
      await get().refresh()
    } catch (error) {
      set({ error: String(error) })
    }
  },
  runAction: async (actionId, confirmed) => {
    if (!isDesktopRuntime()) return null
    try {
      const result = await executeOptimizationAction(actionId, confirmed)
      set({ lastResult: result })
      await get().refresh()
      return result
    } catch (error) {
      set({ error: String(error) })
      return null
    }
  },
  savePolicyConfig: async (config) => {
    if (!isDesktopRuntime()) return null
    try {
      await updateMemoryPolicyConfig(config)
      set({ policyConfig: config })
      return null
    } catch (error) {
      return String(error)
    }
  },
  reportActive: async (active) => {
    if (!isDesktopRuntime()) return
    try {
      await setDashboardActive(active)
    } catch {
      // Idle policy hint only; ignore transport failures.
    }
  },
}))
