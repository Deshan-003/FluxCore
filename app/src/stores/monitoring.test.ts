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
