import { beforeEach, describe, expect, it, vi } from 'vitest'
import type { GpuMetrics, StorageVolume, SystemSnapshot } from '../types/monitoring'

const mocks = vi.hoisted(() => ({
  getSnapshot: vi.fn(),
  getSettings: vi.fn(),
  saveSettings: vi.fn(),
  getHistory: vi.fn(),
  isDesktopRuntime: vi.fn(),
}))

vi.mock('../services/monitoring', () => ({
  DEFAULT_SETTINGS: {
    refreshIntervalMs: 1000,
    historyDurationMinutes: 10,
    persistHistory: true,
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
}))

import { DEFAULT_SETTINGS } from '../services/monitoring'
import { toHistory, useMonitoringStore } from './monitoring'

function gpu(partial: Partial<GpuMetrics> = {}): GpuMetrics {
  return {
    name: 'Test GPU', vendor: 'NVIDIA', provider: 'NVML',
    utilizationPercent: null, dedicatedUsedBytes: null, dedicatedTotalBytes: null,
    dedicatedAvailableBytes: null, sharedUsedBytes: null, vramUsagePercent: null,
    memoryUtilizationPercent: null, temperatureC: null, coreClockMhz: null,
    memoryClockMhz: null, powerWatts: null, fanPercent: null,
    driverVersion: null, gpuClass: null,
    ...partial,
  }
}

function volume(partial: Partial<StorageVolume> = {}): StorageVolume {
  return {
    name: 'C:', fileSystem: 'NTFS', kind: 'NVMe SSD',
    totalBytes: 512 * 1024 ** 3, availableBytes: 256 * 1024 ** 3, usedBytes: 256 * 1024 ** 3,
    usagePercent: 50, readBytesPerSecond: null, writeBytesPerSecond: null,
    temperatureC: null, health: null,
    ...partial,
  }
}

function snapshot(overrides: Partial<SystemSnapshot> = {}): SystemSnapshot {
  return {
    collectedAt: '2026-11-11T12:00:00Z',
    monitoring: true,
    cpu: {
      name: 'Test CPU', vendor: 'Test', architecture: null, physicalCores: null,
      logicalProcessors: 4, usagePercent: null, frequencyMhz: null,
      maxFrequencyMhz: null, temperatureC: null, cores: [],
    },
    memory: {
      totalBytes: 0, usedBytes: 0, availableBytes: 0, freeBytes: null,
      usagePercent: null, cachedBytes: null, committedBytes: null,
      commitLimitBytes: null, pagefileUsedBytes: null, pagefileTotalBytes: null,
    },
    gpus: [],
    storage: [],
    processes: [],
    hardware: { osName: 'Windows', osVersion: '11', hostName: 'test-host', gpus: [] },
    errors: [],
    ...overrides,
  }
}

describe('toHistory', () => {
  it('preserves null percentages instead of coercing them to zero', () => {
    const point = toHistory(snapshot())
    expect(point.cpuPercent).toBeNull()
    expect(point.memoryPercent).toBeNull()
    expect(point.timestamp).toBe('2026-11-11T12:00:00Z')
  })

  it('carries real percentages through unchanged', () => {
    const base = snapshot()
    const point = toHistory(snapshot({
      ...base,
      cpu: { ...base.cpu, usagePercent: 12.5 },
      memory: { ...base.memory, usagePercent: 61.25 },
    }))
    expect(point.cpuPercent).toBe(12.5)
    expect(point.memoryPercent).toBe(61.25)
  })

  it('picks the first reporting GPU utilization and sums only dedicated VRAM', () => {
    const point = toHistory(snapshot({
      gpus: [
        gpu({ name: 'iGPU', utilizationPercent: null, dedicatedUsedBytes: null }),
        gpu({ name: 'dGPU', utilizationPercent: 37, dedicatedUsedBytes: 2 * 1024 ** 3 }),
      ],
    }))
    expect(point.gpuPercent).toBe(37)
    expect(point.vramUsedBytes).toBe(2 * 1024 ** 3)
  })

  it('reports null VRAM when no adapter exposes dedicated memory', () => {
    const point = toHistory(snapshot({ gpus: [gpu({ sharedUsedBytes: 512 * 1024 ** 2 })] }))
    expect(point.vramUsedBytes).toBeNull()
  })

  it('sums disk rates across volumes but stays null when no counter is provided', () => {
    expect(toHistory(snapshot({ storage: [volume(), volume()] })).diskReadBytesPerSecond).toBeNull()
    const point = toHistory(snapshot({
      storage: [
        volume({ name: 'C:', readBytesPerSecond: 100, writeBytesPerSecond: null }),
        volume({ name: 'D:', readBytesPerSecond: 50, writeBytesPerSecond: 25 }),
      ],
    }))
    expect(point.diskReadBytesPerSecond).toBe(150)
    expect(point.diskWriteBytesPerSecond).toBe(25)
  })
})

describe('updateSettings', () => {
  beforeEach(() => {
    localStorage.clear()
    mocks.getSnapshot.mockResolvedValue(snapshot())
    mocks.getHistory.mockResolvedValue([])
    mocks.isDesktopRuntime.mockReturnValue(true)
    mocks.saveSettings.mockResolvedValue(undefined)
    useMonitoringStore.setState({
      snapshot: null, history: [], settings: { ...DEFAULT_SETTINGS },
      status: 'loading', error: null, page: 'Overview', search: '', sortBy: 'cpu',
    })
  })

  it('persists accepted settings locally', async () => {
    const next = { ...DEFAULT_SETTINGS, refreshIntervalMs: 5000 }
    await useMonitoringStore.getState().updateSettings(next)
    expect(useMonitoringStore.getState().settings.refreshIntervalMs).toBe(5000)
    expect(useMonitoringStore.getState().error).toBeNull()
    expect(JSON.parse(localStorage.getItem('fluxcore.preferences.v1')!).refreshIntervalMs).toBe(5000)
  })

  it('rolls back and surfaces the rejection when the backend refuses the change', async () => {
    mocks.saveSettings.mockRejectedValue(new Error('refresh_interval_ms must be between 250 and 60000'))
    await useMonitoringStore.getState().updateSettings({ ...DEFAULT_SETTINGS, refreshIntervalMs: 10 })
    const state = useMonitoringStore.getState()
    expect(state.settings.refreshIntervalMs).toBe(DEFAULT_SETTINGS.refreshIntervalMs)
    expect(state.error).toContain('Settings change rejected')
    expect(state.error).toContain('between 250 and 60000')
    expect(JSON.parse(localStorage.getItem('fluxcore.preferences.v1')!).refreshIntervalMs)
      .toBe(DEFAULT_SETTINGS.refreshIntervalMs)
  })
})
