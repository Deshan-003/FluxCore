import { EmptyState } from '../monitoring'

export default function UnsupportedState({ reason }: { reason: string | null }) {
  return (
    <EmptyState
      title="No GPU compute backend"
      detail={reason ?? 'No usable GPU compute provider was discovered. CPU workloads remain available.'}
    />
  )
}