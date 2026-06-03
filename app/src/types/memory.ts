export type PressureLevel = 'normal' | 'elevated' | 'high' | 'critical'
export type TrendDirection = 'rising' | 'falling' | 'stable'
export type LeakConfidence = 'low' | 'medium' | 'high'
export type RiskLevel = 'none' | 'low' | 'medium'
export type OptimizationMode = 'analysisOnly' | 'smartAssist' | 'manual'
export type ActionStatus = 'success' | 'failed' | 'rejected' | 'cancelled'

export interface MemoryAnalysis {
  collectedAt: string
  totalBytes: number
  usedBytes: number
  availableBytes: number
  freeBytes: number | null
  usagePercent: number
  availablePercent: number
  cachedBytes: number | null
  standbyBytes: number | null
  modifiedBytes: number | null
  compressedBytes: number | null
  committedBytes: number | null
  commitLimitBytes: number | null
  commitPercent: number | null
  pagefileUsedBytes: number | null
  pagefileTotalBytes: number | null
  pagefilePercent: number | null
  hardFaultsPerSec: number | null
  pageFaultsPerSec: number | null
  memoryLoadPercent: number | null
  sources: string[]
  limitations: string[]
}

export interface PressureSignals {
  availablePercent: number
  commitPercent: number | null
  pagefilePercent: number | null
  hardFaultsPerSec: number | null
  availableTrendBytesPerMin: number
}

export interface PressureReport {
  level: PressureLevel
  levelSince: string
  reasons: string[]
  signals: PressureSignals
}

export interface PressureEvent {
  id: number
  startedAt: string
  endedAt: string | null
  level: PressureLevel
  reason: string
  minAvailableBytes: number
  peakCommitPercent: number | null
}

export interface LeakIndicator {
  confidence: LeakConfidence
  growthBytesPerMin: number
  reason: string
}

export interface ProcessMemoryInfo {
  pid: number
  name: string
  privateBytes: number
  workingSetBytes: number
  privateWorkingSetBytes: number | null
  peakWorkingSetBytes: number
  virtualBytes: number | null
  cpuPercent: number | null
  sharePercent: number
  trendBytesPerMin: number
  trendDirection: TrendDirection
  samples: number
  observationSeconds: number
  leak: LeakIndicator | null
  startedAt: string | null
}

export interface CompressionInfo {
  available: boolean
  compressedBytes: number | null
  compressionProcessBytes: number | null
  pressureRelationship: string
  cpuImpactNote: string
  limitation: string | null
}

export interface PagefileConfigEntry {
  path: string
  systemManaged: boolean
  initialSizeMb: number | null
  maximumSizeMb: number | null
}

export interface VirtualMemoryInfo {
  commitChargeBytes: number | null
  commitLimitBytes: number | null
  commitPercent: number | null
  availableCommitBytes: number | null
  pagefilePercentUsage: number | null
  pagefileConfig: PagefileConfigEntry[]
  notes: string[]
  limitations: string[]
}

export interface Recommendation {
  id: string
  title: string
  reason: string
  expectedBenefit: string
  risk: RiskLevel
  category: string
  actionId: string | null
  requiresApproval: boolean
}

export interface ActionDescriptor {
  id: string
  title: string
  description: string
  sideEffects: string
  risk: RiskLevel
  requiresConfirmation: boolean
  changesSystem: boolean
  availableInModes: OptimizationMode[]
}

export interface ActionResult {
  actionId: string
  status: ActionStatus
  detail: string
  executedAt: string
  durationMs: number
}

export interface AuditEntry {
  id: number
  executedAt: string
  mode: OptimizationMode
  actionId: string
  confirmed: boolean
