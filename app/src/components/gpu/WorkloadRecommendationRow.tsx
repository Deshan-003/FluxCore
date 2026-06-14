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
          {assessment.estimateSource === 'measured' ? 'measured' : assessment.estimateSource === 'mixed' ? 'mixed' : 'estimate'}
        </span>
      </div>
      <p className="gpu-reason">{assessment.reason}</p>
      <div className="gpu-grid">
        <span>CPU estimate: <strong>{ms(assessment.estimatedCpuMs)}</strong></span>
        <span>GPU estimate: <strong>{ms(assessment.estimatedGpuTotalMs)}</strong></span>
        <span>Transfer: <strong>{ms(assessment.transferOverheadMs)}</strong> ({fmtBytes(assessment.transferBytes)})</span>
        <span>VRAM needed: <strong>{fmtBytes(assessment.requiredVramBytes)}</strong> / budget {fmtBytes(assessment.gpuBudgetBytes)}</span>
      </div>
      {assessment.blockers.map((blocker) => <p key={blocker} className="gpu-blocker">{blocker}</p>)}
      <div className="gpu-workload-actions">
        <label className="gpu-size-picker">
          <span>Size</span>
          <select
            value={workload.selectedSize}
            onChange={(event) => onSize(workload, Number(event.target.value))}
          >
            {workload.sizes.map((size) => <option key={size} value={size}>{size.toLocaleString()}</option>)}
          </select>
        </label>
        <button className="button primary" disabled={busy} onClick={() => onRun(workload)}>
          <Play size={14} /> Run
        </button>
        {workload.lastBenchmark && (
          <span className="gpu-last-bench" title={workload.lastBenchmark.verdict}>
            last: {workload.lastBenchmark.speedup === null ? 'CPU only' : `${workload.lastBenchmark.speedup.toFixed(2)}x`}
          </span>
        )}
      </div>
    </div>
  )
}