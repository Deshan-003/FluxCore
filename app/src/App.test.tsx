import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { render, screen } from '@testing-library/react'
import type { SystemSnapshot } from './types/monitoring'
import { useMonitoringStore, stopMonitoringSubscriptions } from './stores/monitoring'

const mocks = vi.hoisted(() => ({
  getSnapshot: vi.fn(),
  getSettings: vi.fn(),
  saveSettings: vi.fn(),
  getHistory: vi.fn(),
  isDesktopRuntime: vi.fn(),
  listen: vi.fn(),
}))

vi.mock('./services/monitoring', () => ({
  DEFAULT_SETTINGS: {
    refreshIntervalMs: 1000,
    historyDurationMinutes: 10,
    persistHistory: false,
    monitoringEnabled: true,
    startOnLogin: false,
    minimizeToTray: false,
    theme: 'dark',
  },
  getSnapshot: mocks.getSnapshot,
  getSettings: mocks.getSettings,
  saveSettings: mocks.saveSettings,
  getHistory: mocks.getHistory,
  isDesktopRuntime: mocks.isDesktopRuntime,
  checkHardware: vi.fn(),
}))

vi.mock('@tauri-apps/api/event', () => ({ listen: mocks.listen }))

vi.mock('@tauri-apps/api/core', () => ({
  invoke: vi.fn(async () => { throw new Error('no Tauri backend in frontend tests') }),
  isTauri: () => true,
}))

import App from './App'

const GIB = 1024 ** 3

function desktopSnapshot(): SystemSnapshot {
  return {
    collectedAt: new Date().toISOString(),
    monitoring: true,
    cpu: {
      name: 'AMD Ryzen 5 5500H', vendor: 'AMD', architecture: 'x86-64',
      physicalCores: 6, logicalProcessors: 12, usagePercent: 18.3,
      frequencyMhz: 3600, maxFrequencyMhz: 4200, temperatureC: null,
      cores: [],
    },
    memory: {
      totalBytes: 8 * GIB, usedBytes: 5 * GIB, availableBytes: 3 * GIB,
      freeBytes: 1.2 * GIB, usagePercent: 62.5, cachedBytes: 1.5 * GIB,
      committedBytes: 9 * GIB, commitLimitBytes: 16 * GIB,
      pagefileUsedBytes: 1 * GIB, pagefileTotalBytes: 8 * GIB,
    },
    gpus: [{
      name: 'NVIDIA GeForce RTX 2050', vendor: 'NVIDIA', provider: 'NVML',
      utilizationPercent: 37, dedicatedUsedBytes: 2 * GIB, dedicatedTotalBytes: 4 * GIB,
      dedicatedAvailableBytes: 2 * GIB, sharedUsedBytes: 100 * 1024 ** 2,
      vramUsagePercent: 50, memoryUtilizationPercent: 12, temperatureC: 61,
      coreClockMhz: 1500, memoryClockMhz: 6001, powerWatts: 28.4, fanPercent: 34,
      driverVersion: '555.99', gpuClass: 'Dedicated',
    }],
    storage: [{
      name: 'C:', fileSystem: 'NTFS', kind: 'NVMe SSD',
      totalBytes: 512 * GIB, availableBytes: 200 * GIB, usedBytes: 312 * GIB,
      usagePercent: 61, readBytesPerSecond: 1024, writeBytesPerSecond: 512,
      temperatureC: null, health: null,
    }],
    processes: [],
    hardware: { osName: 'Windows 11 Home', osVersion: '10.0.22631', hostName: 'IDEAPAD', gpus: [] },
    errors: [],
  }
}

async function connectDesktop() {
  mocks.isDesktopRuntime.mockReturnValue(true)
  mocks.getSettings.mockResolvedValue({
    refreshIntervalMs: 1000, historyDurationMinutes: 10, persistHistory: false,
    monitoringEnabled: true, startOnLogin: false, minimizeToTray: false, theme: 'dark',
  })
  mocks.getSnapshot.mockResolvedValue(desktopSnapshot())
  render(<App />)
  expect(await screen.findByText('Monitoring active', {}, { timeout: 4000 })).toBeTruthy()
}

beforeEach(() => {
  vi.stubGlobal('ResizeObserver', class {
    observe() {}
    unobserve() {}
    disconnect() {}
  })
  localStorage.clear()
  mocks.getHistory.mockResolvedValue([])
  mocks.saveSettings.mockResolvedValue(undefined)
  mocks.listen.mockResolvedValue(() => {})
  useMonitoringStore.setState({
    snapshot: null, history: [], status: 'loading', error: null, page: 'dashboard',
  })
})

afterEach(() => {
  stopMonitoringSubscriptions()
  vi.unstubAllGlobals()
})

describe('App in browser preview', () => {
  it('shows the desktop-required state and fabricates no metrics', async () => {
    mocks.isDesktopRuntime.mockReturnValue(false)
    render(<App />)
    expect(await screen.findByText('Open FluxCore for Windows')).toBeTruthy()
    expect(screen.getByText('Desktop app required')).toBeTruthy()
    expect(screen.getByText(/No sample metrics are shown in browser preview/)).toBeTruthy()
    expect(screen.getByText('Waiting for data')).toBeTruthy()
    expect(screen.queryByText('NVIDIA GeForce RTX 2050')).toBeNull()
  })
})

describe('App GPU page', () => {
  it('presents dedicated VRAM separately from system RAM', async () => {
    await connectDesktop()
    useMonitoringStore.setState({ page: 'gpu' })

    expect(await screen.findByText('NVIDIA GeForce RTX 2050')).toBeTruthy()

    // The dedicated-VRAM card shows used/total from the GPU contract, not system RAM.
    expect(screen.getByText('Dedicated VRAM')).toBeTruthy()
    // Used and Available rows both read "2.0 GB of 4.0 GB" on this symmetric fixture.
    expect(screen.getAllByText('2.0 GB of 4.0 GB').length).toBeGreaterThanOrEqual(2)
    expect(screen.getByText(/VRAM is the graphics card's own memory/)).toBeTruthy()
    // Stated in both the page description and the dedicated-VRAM card.
    expect(screen.getAllByText(/separate from system RAM/).length).toBeGreaterThanOrEqual(1)

    const controllerRow = screen.getByText('Memory controller').closest('.rx-bar-row') as HTMLElement
    expect(controllerRow.textContent).toContain('12%')

    const driverRow = screen.getByText('Driver version').closest('.rx-detail-row') as HTMLElement
    expect(driverRow.querySelector('dd')?.textContent).toBe('555.99')

    // The 62.5% system-RAM figure must not appear anywhere on the GPU page.
    expect(screen.queryByText('63%')).toBeNull()
  })

  it('renders unavailable sensors as Unavailable, never as zero', async () => {
    mocks.isDesktopRuntime.mockReturnValue(true)
    mocks.getSettings.mockResolvedValue({
      refreshIntervalMs: 1000, historyDurationMinutes: 10, persistHistory: false,
      monitoringEnabled: true, startOnLogin: false, minimizeToTray: false, theme: 'dark',
    })
    const base = desktopSnapshot()
    mocks.getSnapshot.mockResolvedValue({
      ...base,
      gpus: [{
        ...base.gpus[0],
        provider: 'WMI',
        utilizationPercent: null, dedicatedUsedBytes: null, dedicatedTotalBytes: null,
        dedicatedAvailableBytes: null, sharedUsedBytes: null, vramUsagePercent: null,
        memoryUtilizationPercent: null, temperatureC: null, coreClockMhz: null,
        memoryClockMhz: null, powerWatts: null, fanPercent: null,
        driverVersion: null, gpuClass: null,
      }],
    })
    render(<App />)
    expect(await screen.findByText('Monitoring active', {}, { timeout: 4000 })).toBeTruthy()
    useMonitoringStore.setState({ page: 'gpu' })

    expect(await screen.findByText('Core clock')).toBeTruthy()
    const clockRow = screen.getByText('Core clock').closest('.rx-detail-row') as HTMLElement
    expect(clockRow.querySelector('dd')?.textContent).toBe('Unavailable')
    const powerRow = screen.getByText('Power draw').closest('.rx-detail-row') as HTMLElement
    expect(powerRow.querySelector('dd')?.textContent).toBe('Unavailable')
    expect(screen.queryByText('0 MHz')).toBeNull()
    expect(screen.queryByText('0 W')).toBeNull()
  })
})
