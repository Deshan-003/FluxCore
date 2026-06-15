import {
  Bar, BarChart, CartesianGrid, Legend, ResponsiveContainer, Tooltip, XAxis, YAxis,
} from 'recharts'
import type { BenchmarkRecord } from '../../types/gpu'
import { EmptyState } from '../monitoring'

export default function BenchmarkComparisonChart({ records }: { records: BenchmarkRecord[] }) {
  if (records.length === 0) {
    return <EmptyState title="No benchmarks yet" detail="Run a CPU-vs-GPU benchmark to see measured comparisons here." />
  }
  const data = records.slice(0, 8).reverse().map((record) => ({
    name: `${record.workloadId} #${record.id}`,
    cpu: Number(record.cpu.medianMs.toFixed(3)),
    gpu: record.gpu === null ? null : Number(record.gpu.medianMs.toFixed(3)),
  }))
  return (
    <div className="gpu-bench-chart">
      <ResponsiveContainer width="100%" height={260}>
        <BarChart data={data} margin={{ top: 8, right: 12, bottom: 4, left: 0 }}>
          <CartesianGrid strokeDasharray="3 3" stroke="rgba(148,163,184,0.15)" />
          <XAxis dataKey="name" tick={{ fill: '#94a3b8', fontSize: 11 }} />
          <YAxis tick={{ fill: '#94a3b8', fontSize: 11 }} unit=" ms" />
