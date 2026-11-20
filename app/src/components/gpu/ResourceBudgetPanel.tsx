import { Gauge } from 'lucide-react'
import type { ResourceBudgetStatus } from '../../types/gpu'
import { Detail, Panel, ResourceBar, SensorNotice } from '../monitoring'

function mib(bytes: number | null): string {
  if (bytes === null) return 'Unavailable'
  return `${(bytes / (1024 * 1024)).toFixed(0)} MiB`
}

function percent(value: number | null): string {
  return value === null ? 'Unavailable' : `${value.toFixed(0)}%`
}

export default function ResourceBudgetPanel({ resources }: { resources: ResourceBudgetStatus }) {
  const usedPercent = resources.budgetBytes > 0
    ? (resources.reservedBytes / resources.budgetBytes) * 100
    : null
  return (
    <Panel title="GPU resource budget" icon={Gauge}>
      <ResourceBar
        label="FluxCore VRAM budget"
        value={mib(resources.reservedBytes)}
        total={mib(resources.budgetBytes)}
        percent={usedPercent}
        color="violet"
      />
      <div className="gpu-grid">
        <Detail label="Dedicated VRAM total" value={mib(resources.totalVramBytes)} />
        <Detail label="Dedicated VRAM free" value={mib(resources.freeVramBytes)} />
        <Detail label="Remaining budget" value={mib(resources.remainingBudgetBytes)} />
        <Detail label="Running GPU tasks" value={`${resources.runningGpuTasks} / ${resources.maxConcurrentGpuTasks}`} />
        <Detail label="Queued GPU tasks" value={String(resources.queuedGpuTasks)} />
        <Detail label="GPU utilisation" value={percent(resources.gpuUtilizationPercent)} />
        <Detail label="GPU temperature" value={resources.temperatureC === null ? 'Unavailable' : `${resources.temperatureC.toFixed(0)}°C`} />
        <Detail label="System RAM available" value={percent(resources.systemRamAvailablePercent)} />
      </div>
      <SensorNotice text="Dedicated VRAM is GPU memory, not system RAM. The budget limits only FluxCore's own allocations." />
      {resources.gates.map((gate) => <SensorNotice key={gate} text={gate} />)}
    </Panel>
  )
}