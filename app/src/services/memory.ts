import { invoke } from '@tauri-apps/api/core'
import type {
  ActionDescriptor, ActionResult, MemoryHistorySeries, MemoryIntelligenceSnapshot,
  OptimizationMode, PolicyConfig,
} from '../types/memory'

export async function getMemoryIntelligence(): Promise<MemoryIntelligenceSnapshot> {
  return invoke<MemoryIntelligenceSnapshot>('get_memory_intelligence')
}

export async function getMemoryHistorySeries(limit?: number): Promise<MemoryHistorySeries> {
  return invoke<MemoryHistorySeries>('get_memory_history_series', { limit })
}

export async function getOptimizationMode(): Promise<OptimizationMode> {
  return invoke<OptimizationMode>('get_optimization_mode')
}

export async function setOptimizationMode(mode: OptimizationMode): Promise<void> {
  await invoke('set_optimization_mode', { mode })
}

export async function getOptimizationActions(): Promise<ActionDescriptor[]> {
  return invoke<ActionDescriptor[]>('get_optimization_actions')
}

export async function executeOptimizationAction(actionId: string, confirmed: boolean): Promise<ActionResult> {
  return invoke<ActionResult>('execute_optimization_action', { actionId, confirmed })
}

export async function getMemoryPolicyConfig(): Promise<PolicyConfig> {
  return invoke<PolicyConfig>('get_memory_policy_config')
}

export async function updateMemoryPolicyConfig(config: PolicyConfig): Promise<void> {
  await invoke('update_memory_policy_config', { config })
}

export async function setDashboardActive(active: boolean): Promise<void> {
  await invoke('set_dashboard_active', { active })
}
