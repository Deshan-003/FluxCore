import { invoke } from '@tauri-apps/api/core'
import type { Decision, ResourceHistory, ResourcePolicy, ResourceState } from '../types/intelligence'

export const getResourceIntelligence = () => invoke<ResourceState>('get_resource_intelligence')
export const getResourceHistory = (metric: string, horizon: number) =>
  invoke<ResourceHistory>('get_resource_history', { metric, horizon })
export const configureResourceIntelligence = (policy: ResourcePolicy, confirmed: boolean) =>
  invoke<void>('configure_resource_intelligence', { policy, confirmed })
export const respondToResourceRecommendation = (id: string, decision: Decision, confirmed = false) =>
  invoke<void>('respond_to_resource_recommendation', { id, decision, confirmed })
export const applyResourcePolicy = () => invoke<void>('apply_resource_policy', { confirmed: true })
export const revertResourcePolicy = (id: string) => invoke<void>('revert_resource_policy', { id })
