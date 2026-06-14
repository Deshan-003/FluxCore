import type { BackendKind } from '../../types/gpu'

const LABELS: Record<BackendKind, string> = { cpu: 'CPU', cuda: 'CUDA', wgpu: 'wgpu' }

export default function BackendBadge({ backend, fellBack = false, reason }: {
  backend: BackendKind | null
  fellBack?: boolean
  reason?: string | null
}) {
