import { useEffect, useState } from 'react'
import { Activity, BrainCircuit, ChartNoAxesCombined, History, Lightbulb, ShieldCheck, SlidersHorizontal, Zap } from 'lucide-react'
import { CartesianGrid, Legend, Line, LineChart, ResponsiveContainer, Tooltip, XAxis, YAxis } from 'recharts'
import { Detail, EmptyState, Panel, SensorNotice, SettingRow } from '../components/monitoring'
import { fmtBytes, fmtRate, titleCase } from '../lib/format'
import { getWorkloadRecommendations } from '../services/gpu'
import { applyResourcePolicy, configureResourceIntelligence, respondToResourceRecommendation, revertResourcePolicy } from '../services/intelligence'
import { useResourceStore } from '../stores/intelligence'
import type { SuitabilityAssessment } from '../types/gpu'
import type { ResourceHistory, ResourceMode, ResourcePolicy, ResourceState } from '../types/intelligence'

const sections = ['System health', 'Forecasts', 'Recommendations', 'Adaptive controls', 'Anomalies', 'History'] as const
const modes: Record<ResourceMode, string> = { analysisOnly: 'Analysis Only', smartAssist: 'Smart Assist', adaptive: 'Adaptive Optimization', manual: 'Manual Control' }
const defaultPolicy: ResourcePolicy = { mode: 'analysisOnly', paused: false, scanIntervalMs: 15000, maxScanIntervalMs: 30000, cooldownSeconds: 300, horizonMinutes: [1, 5, 15] }
const labels: Record<string, string> = { cpu: 'CPU utilization', cpuMHz: 'CPU frequency', cpuTemp: 'CPU temperature', corePeak: 'Busiest logical CPU', ram: 'Physical RAM use', ramAvailable: 'Available physical RAM', ramUsed: 'Used physical RAM', ramTotal: 'Installed physical RAM', commit: 'Commit limit used', commitBytes: 'Committed memory', pageReads: 'Physical page reads', pagefileBytes: 'Pagefile used', gpu: 'GPU utilization', vram: 'Dedicated VRAM use', vramBytes: 'Dedicated VRAM used', gpuShared: 'Shared GPU memory used', gpuTemp: 'GPU temperature', disk: 'Storage capacity used', diskFree: 'Storage free', diskRead: 'Disk read', diskWrite: 'Disk write', processCount: 'Observed process count', fluxCpu: 'FluxCore process CPU', fluxRam: 'FluxCore process memory', scansPerMinute: 'Detailed scans per minute', scanMsPerMinute: 'Scan wall time per minute' }
const units = (key: string) => key.split(':')[0]
const metricLabel = (key: string) => `${labels[units(key)] ?? key}${key.includes(':') ? ` · ${key.slice(key.indexOf(':') + 1)}` : ''}`
function reading(key: string, value: number | null | undefined) {
  if (value == null) return 'Unavailable'
  const unit = units(key)
  if (['ramAvailable', 'ramUsed', 'ramTotal', 'commitBytes', 'pagefileBytes', 'vramBytes', 'gpuShared', 'diskFree', 'fluxRam'].includes(unit)) return fmtBytes(value)
  if (unit === 'diskRead' || unit === 'diskWrite') return fmtRate(value)
  if (unit === 'cpuMHz') return `${Math.round(value)} MHz`
  if (unit.endsWith('Temp')) return `${value.toFixed(1)} °C`
  if (unit === 'pageReads') return `${value.toFixed(1)}/s`
