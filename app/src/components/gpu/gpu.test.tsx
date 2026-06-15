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
