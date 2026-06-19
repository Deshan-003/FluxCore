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
  pressure: string | null
}

export interface Forecast {
  metric: string
  observedAt: number
  targetAt: number
  horizonMinutes: number
  value: number | null
  low: number | null
  high: number | null
  actual: number | null
  samples: number
  observationSeconds: number
  coverage: number
  confidence: string
  method: string
  explanation: string
}

export interface Anomaly {
  id: string
  metric: string
  startedAt: number
  endedAt: number | null
  resolution: string | null
  severity: Severity
  value: number
  threshold: number
  explanation: string
}

export interface Recommendation {
  id: string
  createdAt: number
  resource: string
  problem: string
  evidence: string[]
  action: string
  benefit: string
  risk: string
  confidence: string
  requiresApproval: boolean
  applicable: boolean
  severity: Severity
  status: string
}

export interface ResourceAction {
