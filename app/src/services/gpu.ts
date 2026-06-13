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

export async function probeGpuProviders(force: boolean): Promise<ProviderStatus[]> {
  return invoke<ProviderStatus[]>('probe_gpu_providers', { force })
}

export async function listSupportedWorkloads(): Promise<WorkloadView[]> {
  return invoke<WorkloadView[]>('list_supported_workloads')
}

export async function getWorkloadRecommendations(
  size?: number, workloadId?: string,
): Promise<SuitabilityAssessment[]> {
  return invoke<SuitabilityAssessment[]>('get_workload_recommendations', { size, workloadId })
}

export async function submitWorkloadTask(request: SubmitTaskRequest): Promise<GpuTask> {
  return invoke<GpuTask>('submit_workload_task', { request })
}

export async function cancelTask(taskId: number): Promise<GpuTask> {
  return invoke<GpuTask>('cancel_task', { taskId })
}

export async function listTasks(limit?: number, status?: TaskStatus): Promise<GpuTask[]> {
  return invoke<GpuTask[]>('list_tasks', { limit, status })
}

export async function clearFinishedTasks(): Promise<number> {
  return invoke<number>('clear_finished_tasks')
}
