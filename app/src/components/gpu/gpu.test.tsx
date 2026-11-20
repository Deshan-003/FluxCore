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
  })
})

describe('WorkloadRecommendationRow', () => {
  it('shows classification, reason and measured source, and runs the workload', () => {
    const onRun = vi.fn()
    const onSize = vi.fn()
    render(<WorkloadRecommendationRow workload={workload} busy={false} onRun={onRun} onSize={onSize} />)
    expect(screen.getByText('GPU recommended')).toBeTruthy()
    expect(screen.getByText(assessment.reason)).toBeTruthy()
    expect(screen.getByText('measured')).toBeTruthy()
    fireEvent.click(screen.getByRole('button', { name: /Run/ }))
    expect(onRun).toHaveBeenCalledWith(workload)
    fireEvent.change(screen.getByRole('combobox'), { target: { value: '65536' } })
    expect(onSize).toHaveBeenCalledWith(workload, 65_536)
  })
})

describe('TaskMonitorTable', () => {
  it('renders task rows and offers cancel only while active', () => {
    const onCancel = vi.fn()
    render(<TaskMonitorTable tasks={[task]} onCancel={onCancel} />)
    expect(screen.getByText('Vector AXPY')).toBeTruthy()
    expect(screen.getByText('running')).toBeTruthy()
    fireEvent.click(screen.getByRole('button', { name: 'Cancel task 7' }))
    expect(onCancel).toHaveBeenCalledWith(7)
  })
  it('shows an empty hint when there are no tasks', () => {
    render(<TaskMonitorTable tasks={[]} onCancel={() => undefined} />)
    expect(screen.getByText(/No tasks yet/)).toBeTruthy()
  })
})

describe('BenchmarkComparisonChart', () => {
  it('shows an empty state without measurements', () => {
    render(<BenchmarkComparisonChart records={[]} />)
    expect(screen.getByText('No benchmarks yet')).toBeTruthy()
  })
  it('lists honest verdicts including slowdowns', () => {
    const record = {
      id: 1,
      createdAt: '2026-06-08T09:00:00Z',
      workloadId: 'vector_scale',
      params: { kind: 'vectorScale', n: 65_536, a: 2 },
      backend: 'cuda',
      providerId: 'cuda',
      deviceId: '0',
      cpu: { runs: 3, minMs: 0.1, medianMs: 0.2, meanMs: 0.2, maxMs: 0.3, stddevMs: 0.05 },
      gpu: { runs: 3, minMs: 0.4, medianMs: 0.5, meanMs: 0.5, maxMs: 0.6, stddevMs: 0.05 },
      transferMs: 0.2,
      speedup: 0.4,
      vramUsedBytes: 1024,
      validated: true,
      maxAbsError: 0,
      verdict: 'GPU was 2.50x slower than CPU at this size',
      environment: null,
    } as unknown as BenchmarkRecord
    render(<BenchmarkComparisonChart records={[record]} />)
    expect(screen.getByText('GPU was 2.50x slower than CPU at this size')).toBeTruthy()
  })
})