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
  logicalProcessors: number
  systemRamBytes: number
  gpuName: string
  gpuVramBytes: number | null
  driverVersion: string | null
  cudaToolkitPresent: boolean
  cudaRuntimeApiPresent: boolean
  cudaComputePath: string
  wgpuVersion: string
  wgpuBackend: string | null
  vulkanPresent: boolean
  dx12Present: boolean
  recordedAt: string
}

export interface ResourceBudgetStatus {
  totalVramBytes: number | null
  freeVramBytes: number | null
  budgetBytes: number
  reservedBytes: number
  allocatedBytes: number
  remainingBudgetBytes: number
  runningGpuTasks: number
  queuedGpuTasks: number
  maxConcurrentGpuTasks: number
  gpuUtilizationPercent: number | null
  temperatureC: number | null
  powerWatts: number | null
  safetyLevel: SafetyLevel
  gates: string[]
  systemRamAvailablePercent: number | null
}

export type WorkloadParams =
  | { kind: 'vectorAxpy'; n: number; a: number }
  | { kind: 'vectorScale'; n: number; a: number }
  | { kind: 'matmul'; m: number; k: number; n: number }
  | { kind: 'rowSum'; rows: number; cols: number }

export interface TimingBreakdown {
  setupNs: number
  transferInNs: number
  computeNs: number
  transferOutNs: number
  validationNs: number
  totalNs: number
  transferredBytes: number
}

export interface ExecutionRecord {
  backend: BackendKind
  providerId: string
  deviceId: string
  timing: TimingBreakdown
  peakVramBytes: number
  maxAbsError: number | null
  validated: boolean
  notes: string[]
}

export interface SuitabilityAssessment {
  workloadId: string
  inputSize: number
  secondarySize: number | null
  classification: Suitability
  reason: string
  estimatedCpuMs: number | null
  estimatedGpuMs: number | null
  estimatedGpuTotalMs: number | null
  transferOverheadMs: number | null
  transferBytes: number
  estimatedVramBytes: number
  requiredVramBytes: number
  availableVramBytes: number | null
  gpuBudgetBytes: number
  estimateSource: EstimateSource
  selectedBackend: BackendKind
  gpuEnabled: boolean
  blockers: string[]
}

export interface BenchmarkSummary {
  id: number
  createdAt: string
  workloadId: string
  inputSize: number
  backend: BackendKind
  cpuMedianMs: number
  gpuMedianMs: number | null
  transferMs: number | null
  speedup: number | null
  verdict: string
}

export interface WorkloadView {
  id: string
  title: string
  description: string
  category: string
  sizes: number[]
  selectedSize: number
  mode: AccelerationMode
  pinned: boolean
  assessment: SuitabilityAssessment
  lastBenchmark: BenchmarkSummary | null
}

export interface GpuTask {
  id: number
  clientRef: string | null
  workloadId: string
  title: string
  params: WorkloadParams
  status: TaskStatus
  priority: TaskPriority
  requestedBackend: BackendKind | null
  selectedBackend: BackendKind | null
  assessment: SuitabilityAssessment | null
  queuedAt: string
  startedAt: string | null
  finishedAt: string | null
  durationMs: number | null
  queueMs: number | null
  progressPercent: number | null
  cpuTimeMs: number | null
  gpuTimeMs: number | null
  transferTimeMs: number | null
  totalTimeMs: number | null
  speedup: number | null
  vramUsedBytes: number | null
  maxAbsError: number | null
  validated: boolean
  fellBack: boolean
  error: string | null
  warnings: string[]
}

export interface WorkloadSetting {
  workloadId: string
  mode: AccelerationMode
  backend: BackendKind
  enabled: boolean
  selectedSize: number
}

export interface DevicePreference {
  providerId: string
  deviceId: string
}

export interface AccelerationConfig {
  mode: AccelerationMode
  workloadSettings: WorkloadSetting[]
  providerPriority: string[]
  preferredDevices: DevicePreference[]
  vramBudgetPercent: number
  vramBudgetCapMb: number
