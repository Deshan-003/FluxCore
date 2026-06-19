export type ResourceMode = 'analysisOnly' | 'smartAssist' | 'adaptive' | 'manual'
export type Decision = 'apply' | 'reject' | 'dismiss'
export type Severity = 'information' | 'low' | 'medium' | 'high' | 'critical'

export interface ResourcePolicy {
  mode: ResourceMode
  paused: boolean
  scanIntervalMs: number
  maxScanIntervalMs: number
  cooldownSeconds: number
  horizonMinutes: number[]
}

export interface Observation {
  at: number
  values: Record<string, number>
