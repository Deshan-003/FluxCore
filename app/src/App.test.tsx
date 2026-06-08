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
    snapshot: null, history: [], status: 'loading', error: null, page: 'Overview',
  })
})

afterEach(() => {
  stopMonitoringSubscriptions()
