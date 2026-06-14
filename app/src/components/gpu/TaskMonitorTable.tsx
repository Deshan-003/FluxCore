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
