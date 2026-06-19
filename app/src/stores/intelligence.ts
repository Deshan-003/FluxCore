import { create } from 'zustand'
import { getResourceHistory, getResourceIntelligence } from '../services/intelligence'
import type { ResourceHistory, ResourceState } from '../types/intelligence'

interface ResourceStore {
  data: ResourceState | null
  history: { key: string; data: ResourceHistory } | null
  error: string | null
  busy: boolean
  refresh: (metric?: string, horizon?: number) => Promise<void>
  change: (operation: () => Promise<void>) => Promise<boolean>
}

let request = 0
export const useResourceStore = create<ResourceStore>((set, get) => ({
  data: null, history: null, error: null, busy: false,
  refresh: async (metric, horizon) => {
    if (get().busy) return
    const id = ++request
    const [state, history] = await Promise.allSettled([
      getResourceIntelligence(),
      metric !== undefined && horizon !== undefined ? getResourceHistory(metric, horizon) : Promise.resolve(null),
    ])
    if (id !== request) return
    if (state.status === 'fulfilled') {
      set({ data: state.value, error: history.status === 'rejected' ? String(history.reason) : null,
        ...(history.status === 'fulfilled' && history.value ? { history: { key: `${metric}@${horizon}`, data: history.value } } : {}),
      })
    } else set({ error: String(state.reason) })
  },
  change: async (operation) => {
    if (get().busy) return false
    ++request
    set({ busy: true, error: null })
