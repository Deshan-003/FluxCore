import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { fireEvent, render, screen, waitFor } from '@testing-library/react'
import ResourceIntelligencePage from './ResourceIntelligence'
import { useResourceStore } from '../stores/intelligence'
import { getResourceIntelligence, respondToResourceRecommendation } from '../services/intelligence'

vi.mock('../services/intelligence', () => ({
  applyResourcePolicy: vi.fn(async () => undefined),
  configureResourceIntelligence: vi.fn(async () => undefined),
  getResourceHistory: vi.fn(async () => ({ observations: [], comparisons: [], actions: [], decisions: [] })),
  getResourceIntelligence: vi.fn(),
  respondToResourceRecommendation: vi.fn(async () => undefined),
  revertResourcePolicy: vi.fn(async () => undefined),
}))
vi.mock('../services/gpu', () => ({ getWorkloadRecommendations: vi.fn(async () => []) }))

const state = {
  policy: { mode: 'smartAssist' as const, paused: false, scanIntervalMs: 15_000, maxScanIntervalMs: 30_000, cooldownSeconds: 300, horizonMinutes: [1, 5, 15] },
  monitoring: true, historyEnabled: true, latest: { at: 1_800_000_000, values: { cpu: 94, ram: 55 }, pressure: 'normal' },
  health: 'Elevated resource activity', insights: [], status: null, activeIntervalMs: null,
  recommendations: [{ id: 'scan@1800000000', createdAt: 1_800_000_000, resource: 'Monitoring', problem: 'Optional detailed process scans compete for CPU time', evidence: ['System CPU remained high'], action: 'Reduce the detailed scan frequency', benefit: 'May reduce FluxCore scan overhead', risk: 'Process details update less often', confidence: 'moderate', requiresApproval: true, applicable: true, severity: 'low' as const, status: 'pending' }],
  forecasts: [], anomalies: [],
}

beforeEach(() => {
