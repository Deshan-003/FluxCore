import { Play } from 'lucide-react'
import type { Suitability, WorkloadView } from '../../types/gpu'
import { fmtBytes } from '../../lib/format'
import BackendBadge from './BackendBadge'

const CLASS_LABEL: Record<Suitability, string> = {
  gpuRecommended: 'GPU recommended',
  cpuRecommended: 'CPU recommended',
  gpuAvailableNotBeneficial: 'GPU available, not beneficial',
  insufficientGpuResources: 'Insufficient GPU resources',
  unsupportedWorkload: 'Unsupported workload',
  accelerationUnavailable: 'Acceleration unavailable',
}

function ms(value: number | null): string {
  return value === null ? '—' : `${value.toFixed(2)} ms`
}

export default function WorkloadRecommendationRow({ workload, busy, onRun, onSize }: {
  workload: WorkloadView
  busy: boolean
  onRun: (workload: WorkloadView) => void
  onSize: (workload: WorkloadView, size: number) => void
}) {
  const assessment = workload.assessment
  return (
    <div className="gpu-workload-row">
      <div className="gpu-workload-head">
        <strong>{workload.title}</strong>
        <span className={`gpu-pill ${assessment.classification}`}>
          {CLASS_LABEL[assessment.classification]}
        </span>
        <BackendBadge backend={assessment.selectedBackend} />
        <span className="gpu-estimate-source">
