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
