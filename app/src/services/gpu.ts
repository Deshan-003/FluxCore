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

export async function getTaskHistory(limit?: number, workloadId?: string): Promise<GpuTask[]> {
  return invoke<GpuTask[]>('get_task_history', { limit, workloadId })
}

export async function runBenchmark(request: BenchmarkRequest): Promise<BenchmarkRecord> {
  return invoke<BenchmarkRecord>('run_benchmark', { request })
}

export async function getBenchmarkHistory(limit?: number, workloadId?: string): Promise<BenchmarkRecord[]> {
  return invoke<BenchmarkRecord[]>('get_benchmark_history', { limit, workloadId })
}

export async function clearBenchmarkHistory(): Promise<number> {
  return invoke<number>('clear_benchmark_history')
}

export async function getGpuEnvironment(): Promise<AccelerationEnvironment> {
  return invoke<AccelerationEnvironment>('get_gpu_environment')
}

export function onGpuTaskEvent(handler: (task: GpuTask) => void): Promise<UnlistenFn> {
  return listen<GpuTask>('fluxcore-gpu-task', (event) => handler(event.payload))
}

export function onGpuCapabilitiesEvent(handler: (providers: ProviderStatus[]) => void): Promise<UnlistenFn> {
  return listen<ProviderStatus[]>('fluxcore-gpu-capabilities', (event) => handler(event.payload))
}