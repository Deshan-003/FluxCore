import { describe, expect, it, vi } from 'vitest'
import { fireEvent, render, screen } from '@testing-library/react'

// jsdom has no ResizeObserver; recharts' ResponsiveContainer requires one.
vi.stubGlobal('ResizeObserver', class {
  observe() {}
  unobserve() {}
  disconnect() {}
})
import BackendBadge from './BackendBadge'
import UnsupportedState from './UnsupportedState'
import ResourceBudgetPanel from './ResourceBudgetPanel'
import WorkloadRecommendationRow from './WorkloadRecommendationRow'
import TaskMonitorTable from './TaskMonitorTable'
import BenchmarkComparisonChart from './BenchmarkComparisonChart'
import type {
  BenchmarkRecord, GpuTask, ResourceBudgetStatus, SuitabilityAssessment, WorkloadView,
} from '../../types/gpu'

const assessment: SuitabilityAssessment = {
  workloadId: 'vector_axpy',
  inputSize: 1_048_576,
  secondarySize: null,
  classification: 'gpuRecommended',
  reason: 'GPU is ~3.4x faster at 1M elements (measured)',
  estimatedCpuMs: 4.2,
  estimatedGpuMs: 0.6,
  estimatedGpuTotalMs: 1.2,
  transferOverheadMs: 0.5,
  transferBytes: 12_582_912,
  estimatedVramBytes: 12_582_912,
  requiredVramBytes: 12_582_912,
  availableVramBytes: 3_000_000_000,
  gpuBudgetBytes: 2_147_483_648,
  estimateSource: 'measured',
  selectedBackend: 'cuda',
  gpuEnabled: true,
  blockers: [],
}

const workload: WorkloadView = {
  id: 'vector_axpy',
  title: 'Vector AXPY',
  description: 'out = a*x + y',
  category: 'vector',
  sizes: [65_536, 1_048_576],
  selectedSize: 1_048_576,
  mode: 'intelligentAuto',
  pinned: false,
  assessment,
  lastBenchmark: null,
}

const resources: ResourceBudgetStatus = {
  totalVramBytes: 4_294_967_296,
  freeVramBytes: 4_000_000_000,
  budgetBytes: 2_147_483_648,
  reservedBytes: 0,
  allocatedBytes: 0,
  remainingBudgetBytes: 2_147_483_648,
  runningGpuTasks: 0,
  queuedGpuTasks: 0,
  maxConcurrentGpuTasks: 1,
  gpuUtilizationPercent: null,
  temperatureC: null,
  powerWatts: null,
  safetyLevel: 'ok',
  gates: ['GPU utilisation 91% above limit 85%'],
  systemRamAvailablePercent: null,
}

const task: GpuTask = {
  id: 7,
  clientRef: null,
  workloadId: 'vector_axpy',
  title: 'Vector AXPY',
  params: { kind: 'vectorAxpy', n: 1024, a: 1.5 },
  status: 'running',
  priority: 'normal',
  requestedBackend: 'cuda',
  selectedBackend: 'cuda',
  assessment: null,
  queuedAt: '2026-06-08T09:00:00Z',
  startedAt: '2026-06-08T09:00:01Z',
  finishedAt: null,
  durationMs: null,
  queueMs: 1000,
  progressPercent: 40,
  cpuTimeMs: null,
  gpuTimeMs: null,
  transferTimeMs: null,
  totalTimeMs: null,
  speedup: null,
  vramUsedBytes: null,
  maxAbsError: null,
  validated: false,
  fellBack: false,
  error: null,
  warnings: [],
}

describe('BackendBadge', () => {
  it('labels the backend and marks CPU fallback', () => {
    render(<BackendBadge backend="cuda" fellBack />)
    expect(screen.getByText('CUDA')).toBeTruthy()
    expect(screen.getByText('→ CPU')).toBeTruthy()
  })
})

describe('UnsupportedState', () => {
  it('shows the real unavailable reason', () => {
    render(<UnsupportedState reason="nvcuda.dll missing" />)
    expect(screen.getByText('No GPU compute backend')).toBeTruthy()
    expect(screen.getByText('nvcuda.dll missing')).toBeTruthy()
  })
})

describe('ResourceBudgetPanel', () => {
  it('renders null telemetry as Unavailable and shows active gates', () => {
    render(<ResourceBudgetPanel resources={resources} />)
    expect(screen.getAllByText('Unavailable').length).toBeGreaterThan(0)
    expect(screen.getByText('GPU utilisation 91% above limit 85%')).toBeTruthy()
