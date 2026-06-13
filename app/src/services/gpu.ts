import { invoke } from '@tauri-apps/api/core'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'
import type {
  AccelerationConfig, AccelerationEnvironment, AccelerationMode, BenchmarkRecord,
  BenchmarkRequest, GpuAccelerationState, GpuTask, ProviderStatus, SubmitTaskRequest,
  SuitabilityAssessment, TaskStatus, WorkloadView,
} from '../types/gpu'

export async function getGpuAccelerationState(): Promise<GpuAccelerationState> {
  return invoke<GpuAccelerationState>('get_gpu_acceleration_state')
}

export async function setAccelerationMode(mode: AccelerationMode): Promise<GpuAccelerationState> {
  return invoke<GpuAccelerationState>('set_acceleration_mode', { mode })
}

export async function getGpuResourceConfig(): Promise<AccelerationConfig> {
  return invoke<AccelerationConfig>('get_gpu_resource_config')
}

export async function updateGpuResourceConfig(config: AccelerationConfig): Promise<AccelerationConfig> {
  return invoke<AccelerationConfig>('update_gpu_resource_config', { config })
}

export async function listGpuProviders(): Promise<ProviderStatus[]> {
  return invoke<ProviderStatus[]>('list_gpu_providers')
}

