import { X } from 'lucide-react'
import type { GpuTask } from '../../types/gpu'
import { fmtBytes } from '../../lib/format'
import BackendBadge from './BackendBadge'

function ms(value: number | null): string {
  return value === null ? '—' : value.toFixed(1)
}

export default function TaskMonitorTable({ tasks, onCancel }: {
  tasks: GpuTask[]
  onCancel: (taskId: number) => void
}) {
  if (tasks.length === 0) {
    return <p className="gpu-empty-row">No tasks yet. Run a workload from Workload Intelligence.</p>
  }
  return (
    <table className="gpu-task-table">
      <thead>
        <tr>
          <th>ID</th><th>Workload</th><th>Backend</th><th>Status</th><th>Progress</th>
          <th>Total ms</th><th>Speedup</th><th>VRAM</th><th>Notes</th><th></th>
        </tr>
      </thead>
      <tbody>
        {tasks.map((task) => (
          <tr key={task.id}>
            <td>{task.id}</td>
            <td title={task.workloadId}>{task.title}</td>
            <td><BackendBadge backend={task.selectedBackend} fellBack={task.fellBack} /></td>
            <td><span className={`gpu-pill status-${task.status}`}>{task.status}</span></td>
            <td>{task.progressPercent === null ? '—' : `${task.progressPercent.toFixed(0)}%`}</td>
            <td>{ms(task.totalTimeMs)}</td>
            <td>{task.speedup === null ? '—' : `${task.speedup.toFixed(2)}x`}</td>
            <td>{fmtBytes(task.vramUsedBytes)}</td>
            <td className="gpu-task-notes">
              {task.error && <span className="gpu-error">{task.error}</span>}
              {task.warnings.map((warning) => <span key={warning} className="gpu-warning">{warning}</span>)}
            </td>
            <td>
              {(task.status === 'queued' || task.status === 'running') && (
                <button className="button ghost" aria-label={`Cancel task ${task.id}`} onClick={() => onCancel(task.id)}>
                  <X size={14} />
                </button>
              )}
            </td>
          </tr>
        ))}
      </tbody>
    </table>
  )
}