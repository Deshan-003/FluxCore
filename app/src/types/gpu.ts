// Phase 3 TS mirrors of the Rust serde camelCase contract
// (PHASE3_ARCHITECTURE.md §5/§6 + PHASE3_CONTRACT_FREEZE.md). Nullable numbers
// are `number | null`; the UI renders null as "Unavailable", never a guessed 0.

export type ProviderKind = 'cpu' | 'cuda' | 'wgpu' | 'other'
export type AccelerationMode = 'cpuOnly' | 'gpuAcceleration' | 'intelligentAuto' | 'manual'
export type BackendKind = 'cpu' | 'cuda' | 'wgpu'
export type Suitability =
  | 'gpuRecommended'
  | 'cpuRecommended'
  | 'gpuAvailableNotBeneficial'
  | 'insufficientGpuResources'
  | 'unsupportedWorkload'
  | 'accelerationUnavailable'
export type TaskStatus =
  | 'queued' | 'running' | 'succeeded' | 'failed' | 'cancelled' | 'timedOut' | 'rejected'
export type TaskPriority = 'low' | 'normal' | 'high'
export type SafetyLevel = 'ok' | 'caution' | 'blocked'
export type EstimateSource = 'model' | 'measured' | 'mixed'

export interface GpuDeviceInfo {
  deviceId: string
  name: string
  vendor: string
  backend: string
  monitorGpuIndex: number | null
  dedicatedTotalBytes: number | null
  dedicatedFreeBytes: number | null
  computeCapability: string | null
  driverVersion: string | null
  maxWorkgroupSize: number | null
  maxStorageBufferBytes: number | null
  isDiscrete: boolean
  notes: string[]
}

export interface ProviderCapabilities {
  computeReady: boolean
  unavailableReason: string | null
  maxThreadsPerWorkgroup: number | null
  maxWorkgroupSizeX: number | null
  maxWorkgroupSizeY: number | null
  maxWorkgroupSizeZ: number | null
  maxBufferBytes: number | null
  allocationAlignment: number
  backendNotes: string[]
}

export interface WorkloadSupport {
  workloadId: string
  maxElements: number
  notes: string[]
}

export interface GpuMemoryInfo {
  totalBytes: number | null
  freeBytes: number | null
  usedBytes: number | null
  allocatedBytes: number
}

export interface ProviderStatus {
  id: string
  label: string
  kind: ProviderKind
  available: boolean
  computeReady: boolean
  detail: string
  devices: GpuDeviceInfo[]
  capabilities: ProviderCapabilities
  supportedWorkloads: WorkloadSupport[]
  selectedDevice: string
  lastError: string | null
}

export interface AccelerationEnvironment {
  osName: string
  osVersion: string
  cpuName: string
