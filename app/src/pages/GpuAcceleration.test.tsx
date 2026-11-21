import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { fireEvent, render, screen } from '@testing-library/react'
import type { AccelerationConfig, GpuAccelerationState, ProviderStatus } from '../types/gpu'

const mocks = vi.hoisted(() => ({
  getGpuAccelerationState: vi.fn(),
  getGpuResourceConfig: vi.fn(),
  getTaskHistory: vi.fn(),
  getBenchmarkHistory: vi.fn(),
  cancelTask: vi.fn(),
  onGpuTaskEvent: vi.fn(),
  onGpuCapabilitiesEvent: vi.fn(),
}))

vi.mock('../services/gpu', () => ({
  getGpuAccelerationState: mocks.getGpuAccelerationState,
  setAccelerationMode: vi.fn(),
  getGpuResourceConfig: mocks.getGpuResourceConfig,
  updateGpuResourceConfig: vi.fn(),
  listGpuProviders: vi.fn(),
  probeGpuProviders: vi.fn(),
  listSupportedWorkloads: vi.fn(),
  getWorkloadRecommendations: vi.fn(),
  submitWorkloadTask: vi.fn(),
  cancelTask: mocks.cancelTask,
  listTasks: vi.fn(),
  clearFinishedTasks: vi.fn(),
  getTaskHistory: mocks.getTaskHistory,
  runBenchmark: vi.fn(),
  getBenchmarkHistory: mocks.getBenchmarkHistory,
  clearBenchmarkHistory: vi.fn(),
  getGpuEnvironment: vi.fn(),
  onGpuTaskEvent: mocks.onGpuTaskEvent,
  onGpuCapabilitiesEvent: mocks.onGpuCapabilitiesEvent,
}))

import GpuAccelerationPage from './GpuAcceleration'
import { useGpuStore } from '../stores/gpu'

const GIB = 1024 ** 3

function provider(partial: Partial<ProviderStatus> = {}): ProviderStatus {
  return {
    id: 'cuda', label: 'CUDA (NVIDIA)', kind: 'cuda', available: true, computeReady: true,
    detail: 'Ready — RTX 2050, driver 555.99.', devices: [], capabilities: {
      computeReady: true, unavailableReason: null, maxThreadsPerWorkgroup: null,
      maxWorkgroupSizeX: null, maxWorkgroupSizeY: null, maxWorkgroupSizeZ: null,
      maxBufferBytes: null, allocationAlignment: 256, backendNotes: [],
    },
    supportedWorkloads: [], selectedDevice: '0', lastError: null,
    ...partial,
  }
}

function gpuState(): GpuAccelerationState {
  return {
    collectedAt: new Date().toISOString(),
    mode: 'intelligentAuto',
    available: true,
    unavailableReason: null,
    providers: [provider()],
    workloads: [{
      id: 'vector_axpy', title: 'Vector AXPY', description: 'Vector arithmetic', category: 'vector',
      sizes: [1048576, 4194304], selectedSize: 1048576, mode: 'intelligentAuto', pinned: false,
      assessment: {
        workloadId: 'vector_axpy', inputSize: 1048576, secondarySize: null,
        classification: 'gpuRecommended', reason: 'The workload fits in VRAM with headroom.',
        estimatedCpuMs: 12.5, estimatedGpuMs: 1.2, estimatedGpuTotalMs: 2.4,
        transferOverheadMs: 1.1, transferBytes: 8 * GIB / 1024, estimatedVramBytes: 32 * 1024 ** 2,
        requiredVramBytes: 32 * 1024 ** 2, availableVramBytes: 2 * GIB, gpuBudgetBytes: 1 * GIB,
        estimateSource: 'measured', selectedBackend: 'cuda', gpuEnabled: true, blockers: [],
      },
      lastBenchmark: null,
    }],
    resources: {
      totalVramBytes: 4 * GIB, freeVramBytes: 3 * GIB, budgetBytes: 1 * GIB,
      reservedBytes: 256 * 1024 ** 2, allocatedBytes: 0, remainingBudgetBytes: 768 * 1024 ** 2,
      runningGpuTasks: 1, queuedGpuTasks: 0, maxConcurrentGpuTasks: 2,
      gpuUtilizationPercent: 22, temperatureC: 54, powerWatts: 28, safetyLevel: 'ok',
      gates: [], systemRamAvailablePercent: 62,
    },
    tasks: [{
      id: 7, clientRef: null, workloadId: 'vector_axpy', title: 'Vector AXPY',
      params: { kind: 'vectorAxpy', n: 1048576, a: 1.5 }, status: 'running', priority: 'normal',
      requestedBackend: 'cuda', selectedBackend: 'cuda', assessment: null,
      queuedAt: new Date().toISOString(), startedAt: new Date().toISOString(), finishedAt: null,
      durationMs: null, queueMs: null, progressPercent: 45, cpuTimeMs: null, gpuTimeMs: null,
      transferTimeMs: null, totalTimeMs: null, speedup: null, vramUsedBytes: null,
      maxAbsError: null, validated: false, fellBack: false, error: null, warnings: [],
    }],
    safetyNotes: [],
    environment: {
      osName: 'Windows 11', osVersion: '10.0', cpuName: 'Test CPU', logicalProcessors: 8,
      systemRamBytes: 16 * GIB, gpuName: 'RTX 2050', gpuVramBytes: 4 * GIB, driverVersion: '555.99',
      cudaToolkitPresent: true, cudaRuntimeApiPresent: true, cudaComputePath: 'sm_75',
      wgpuVersion: '25', wgpuBackend: 'vulkan', vulkanPresent: true, dx12Present: true,
      recordedAt: new Date().toISOString(),
    },
  }
}

function gpuConfig(): AccelerationConfig {
  return {
    mode: 'intelligentAuto', workloadSettings: [], providerPriority: ['cuda', 'wgpu', 'cpu'],
    preferredDevices: [], vramBudgetPercent: 25, vramBudgetCapMb: 1024,
    maxGpuUtilizationPercent: 85, maxTemperatureC: 90, maxConcurrentGpuTasks: 2,
    maxConcurrentCpuTasks: 2, maxQueueLength: 64, taskTimeoutMs: 60000,
    minSystemRamAvailablePercent: 15, persistHistory: true, retentionDays: 14,
    autoBenchmark: false, validateResults: true, emitEvents: true,
  }
}

beforeEach(() => {
  localStorage.clear()
  mocks.getTaskHistory.mockResolvedValue([])
  mocks.getBenchmarkHistory.mockResolvedValue([])
  mocks.cancelTask.mockResolvedValue(undefined)
  mocks.onGpuTaskEvent.mockResolvedValue(() => {})
  mocks.onGpuCapabilitiesEvent.mockResolvedValue(() => {})
  useGpuStore.setState({
    state: gpuState(), config: gpuConfig(), benchmarks: [], history: [],
    loading: false, benchmarkBusy: false, error: null,
    prefs: { lastTab: 'overview', selectedSizes: {}, autoRefresh: false },
  })
})

afterEach(() => {
  vi.clearAllMocks()
})

describe('GpuAccelerationPage', () => {
  it('states that VRAM is never used as system RAM and shows the budget honestly', () => {
    render(<GpuAccelerationPage />)
    expect(screen.getByText(/never used as system RAM/)).toBeTruthy()
    expect(screen.getByText('256.0 MB of 1.0 GB')).toBeTruthy()
    expect(screen.getByText('Remaining budget')).toBeTruthy()
  })

  it('explains workload recommendations with the reason and evidence source', () => {
    useGpuStore.setState({ prefs: { lastTab: 'workloads', selectedSizes: {}, autoRefresh: false } })
    render(<GpuAccelerationPage />)
    expect(screen.getByText('Vector AXPY')).toBeTruthy()
    expect(screen.getByText('GPU recommended')).toBeTruthy()
    expect(screen.getByText(/The workload fits in VRAM with headroom\./)).toBeTruthy()
    expect(screen.getByText('measured evidence')).toBeTruthy()
  })

  it('shows running tasks with a status chip and cancels through a confirm dialog', () => {
    useGpuStore.setState({ prefs: { lastTab: 'tasks', selectedSizes: {}, autoRefresh: false } })
    render(<GpuAccelerationPage />)
    expect(screen.getByText('Running')).toBeTruthy()
    fireEvent.click(screen.getByText('Cancel'))
    expect(screen.getByText('Cancel this task?')).toBeTruthy()
    // Confirming calls the real store action (mocked service underneath).
    fireEvent.click(screen.getByText('Cancel task'))
    expect(mocks.cancelTask).toHaveBeenCalledWith(7)
  })

  it('shows an honest unavailable state with the backend reason', () => {
    const current = gpuState()
    useGpuStore.setState({
      state: { ...current, available: false, unavailableReason: 'No CUDA or DX12 compute adapter found.' },
      prefs: { lastTab: 'overview', selectedSizes: {}, autoRefresh: false },
    })
    render(<GpuAccelerationPage />)
    expect(screen.getByText('GPU acceleration is not available on this system')).toBeTruthy()
    expect(screen.getByText(/No CUDA or DX12 compute adapter found\./)).toBeTruthy()
  })
})
