import type { BackendKind } from '../../types/gpu'

const LABELS: Record<BackendKind, string> = { cpu: 'CPU', cuda: 'CUDA', wgpu: 'wgpu' }

export default function BackendBadge({ backend, fellBack = false, reason }: {
  backend: BackendKind | null
  fellBack?: boolean
  reason?: string | null
}) {
  const kind = backend ?? 'cpu'
  const title = reason ?? (fellBack ? 'GPU path failed; result produced on CPU' : undefined)
  return (
    <span className={`gpu-badge ${kind}`} title={title}>
      {LABELS[kind]}
      {fellBack && <em className="gpu-badge-fallback"> → CPU</em>}
    </span>
  )
}