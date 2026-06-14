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
