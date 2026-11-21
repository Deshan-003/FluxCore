import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { fireEvent, render, screen } from '@testing-library/react'
import type { ResourceState } from '../types/intelligence'

const mocks = vi.hoisted(() => ({
  getResourceIntelligence: vi.fn(),
  getResourceHistory: vi.fn(),
  configureResourceIntelligence: vi.fn(),
  respondToResourceRecommendation: vi.fn(),
  applyResourcePolicy: vi.fn(),
  revertResourcePolicy: vi.fn(),
  getWorkloadRecommendations: vi.fn(),
}))

vi.mock('../services/intelligence', () => ({
  getResourceIntelligence: mocks.getResourceIntelligence,
  getResourceHistory: mocks.getResourceHistory,
  configureResourceIntelligence: mocks.configureResourceIntelligence,
  respondToResourceRecommendation: mocks.respondToResourceRecommendation,
  applyResourcePolicy: mocks.applyResourcePolicy,
  revertResourcePolicy: mocks.revertResourcePolicy,
}))

vi.mock('../services/gpu', () => ({
  getWorkloadRecommendations: mocks.getWorkloadRecommendations,
}))

import ResourceIntelligencePage from './ResourceIntelligence'
import { useResourceStore } from '../stores/intelligence'

function resourceState(overrides: Partial<ResourceState> = {}): ResourceState {
  return {
    policy: { mode: 'smartAssist', paused: false, scanIntervalMs: 15000, maxScanIntervalMs: 30000, cooldownSeconds: 300, horizonMinutes: [1, 5, 15] },
    monitoring: true,
    historyEnabled: true,
    latest: { at: Math.floor(Date.now() / 1000), values: { cpu: 42.4, ram: 55.1 }, pressure: 'normal' },
    health: 'Normal',
    insights: ['CPU and memory are within their normal ranges.'],
    status: null,
    activeIntervalMs: null,
    recommendations: [{
      id: 'rec-1', createdAt: Date.now(), resource: 'memory',
      problem: 'Two Chromium processes hold most of the RAM',
      evidence: ['chrome.exe holds 4.8 GB across two processes'],
      action: 'Close or restart the background browser tabs',
      benefit: 'Frees roughly 4 GB immediately',
      risk: 'Low risk when the apps are idle',
      confidence: 'high', requiresApproval: true, applicable: true,
      severity: 'medium', status: 'pending',
    }],
    forecasts: [],
    anomalies: [],
    ...overrides,
  }
}

beforeEach(() => {
  vi.spyOn(window, 'confirm').mockReturnValue(false)
  mocks.getResourceIntelligence.mockResolvedValue(resourceState())
  mocks.getResourceHistory.mockResolvedValue({ observations: [], comparisons: [], actions: [], decisions: [] })
  mocks.configureResourceIntelligence.mockResolvedValue(undefined)
  mocks.respondToResourceRecommendation.mockResolvedValue(undefined)
  useResourceStore.setState({ data: null, history: null, error: null, busy: false })
})

afterEach(() => {
  vi.restoreAllMocks()
  vi.clearAllMocks()
})

describe('ResourceIntelligencePage', () => {
  it('shows measured system health and live readings without estimates', async () => {
    useResourceStore.setState({ data: resourceState() })
    render(<ResourceIntelligencePage />)
    expect(await screen.findByText('System health: Normal')).toBeTruthy()
    expect(screen.getByText('CPU utilization')).toBeTruthy()
    expect(screen.getByText('42.4%')).toBeTruthy()
    expect(screen.getByText(/Local statistical analysis of live measurements/)).toBeTruthy()
  })

  it('routes recommendation approval through a dialog, never window.confirm', async () => {
    useResourceStore.setState({ data: resourceState() })
    render(<ResourceIntelligencePage />)
    fireEvent.click(await screen.findByText('Recommendations'))
    const approve = await screen.findByText('Approve & apply')
    fireEvent.click(approve)
    expect(screen.getByText('Apply this temporary FluxCore scan interval?')).toBeTruthy()
    expect(window.confirm).not.toHaveBeenCalled()
  })

  it('disables approval when a scan override is already active', async () => {
    // The page refreshes from the service on mount — the override must come
    // from the mocked service, not only from the initial store state.
    mocks.getResourceIntelligence.mockResolvedValue(resourceState({ activeIntervalMs: 30000 }))
    useResourceStore.setState({
      data: resourceState({ activeIntervalMs: 30000 }),
    })
    render(<ResourceIntelligencePage />)
    fireEvent.click(await screen.findByText('Recommendations'))
    const approve = await screen.findByText('Approve & apply') as HTMLButtonElement
    expect(approve.disabled).toBe(true)
  })

  it('surfaces the monitoring-paused state instead of inventing readings', async () => {
    useResourceStore.setState({ data: resourceState({ monitoring: false }) })
    render(<ResourceIntelligencePage />)
    expect(await screen.findByText(/Monitoring is paused or still initializing/)).toBeTruthy()
  })
})
