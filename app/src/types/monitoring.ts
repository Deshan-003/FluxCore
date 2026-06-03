export type SensorValue = number | null

export interface CpuCore {
  id: number
  usagePercent: number
  frequencyMhz: number | null
}

export interface CpuMetrics {
  name: string
  vendor: string
  architecture: string | null
  physicalCores: number | null
  logicalProcessors: number
  usagePercent: SensorValue
  frequencyMhz: number | null
  maxFrequencyMhz: number | null
  temperatureC: SensorValue
  cores: CpuCore[]
}

export interface MemoryMetrics {
  totalBytes: number
  usedBytes: number
  availableBytes: number
  freeBytes: SensorValue
  usagePercent: SensorValue
  cachedBytes: SensorValue
  committedBytes: SensorValue
  commitLimitBytes: SensorValue
  pagefileUsedBytes: SensorValue
  pagefileTotalBytes: SensorValue
}

export interface GpuMetrics {
  name: string
  vendor: string
  provider: string
  utilizationPercent: SensorValue
  dedicatedUsedBytes: SensorValue
  dedicatedTotalBytes: SensorValue
  dedicatedAvailableBytes: SensorValue
  sharedUsedBytes: SensorValue
  vramUsagePercent: SensorValue
  memoryUtilizationPercent: SensorValue
  temperatureC: SensorValue
  coreClockMhz: SensorValue
  memoryClockMhz: SensorValue
  powerWatts: SensorValue
  fanPercent: SensorValue
  driverVersion: string | null
  gpuClass: string | null
}

export interface StorageVolume {
  name: string
  fileSystem: string
  kind: string
  totalBytes: number
  availableBytes: number
  usedBytes: number
  usagePercent: SensorValue
  readBytesPerSecond: SensorValue
  writeBytesPerSecond: SensorValue
  temperatureC: SensorValue
  health: string | null
}

export interface ProcessMetrics {
  pid: number
  name: string
  cpuPercent: number | null
  memoryBytes: number
  memoryPercent: SensorValue
  gpuPercent: SensorValue
  gpuMemoryBytes: SensorValue
  startedAt: string | null
}

export interface HardwareInfo {
  osName: string
  osVersion: string
  hostName: string
  gpus: GpuMetrics[]
}

export interface SystemSnapshot {
  collectedAt: string
  monitoring: boolean
  cpu: CpuMetrics
  memory: MemoryMetrics
  gpus: GpuMetrics[]
  storage: StorageVolume[]
  processes: ProcessMetrics[]
  hardware: HardwareInfo
  errors: string[]
}

export interface HardwareCheckItem {
  component: string
  status: 'available' | 'partial' | 'unavailable'
  provider: string
  detail: string
}

export interface HardwareCheckReport {
  checkedAt: string
  checks: HardwareCheckItem[]
}

export interface MonitoringSettings {
  refreshIntervalMs: number
  historyDurationMinutes: number
  persistHistory: boolean
  monitoringEnabled: boolean
  startOnLogin: boolean
  minimizeToTray: boolean
  theme: 'dark' | 'light' | 'system'
}

export interface HistoryPoint {
  timestamp: string
  cpuPercent: SensorValue
  memoryPercent: SensorValue
  gpuPercent: number | null
  vramUsedBytes: number | null
  diskReadBytesPerSecond: number | null
  diskWriteBytesPerSecond: number | null
}
