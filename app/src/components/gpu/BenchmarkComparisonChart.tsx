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
          <Tooltip
            contentStyle={{ background: '#0b1220', border: '1px solid #1e293b', borderRadius: 8 }}
            labelStyle={{ color: '#e2e8f0' }}
            formatter={(value) => (value === null ? 'did not run' : `${value} ms`)}
          />
          <Legend wrapperStyle={{ color: '#94a3b8', fontSize: 12 }} />
          <Bar dataKey="cpu" name="CPU median" fill="#22d3ee" radius={[3, 3, 0, 0]} />
          <Bar dataKey="gpu" name="GPU median (end-to-end)" fill="#a78bfa" radius={[3, 3, 0, 0]} />
        </BarChart>
      </ResponsiveContainer>
      <ul className="gpu-verdicts">
        {records.slice(0, 8).map((record) => (
          <li key={record.id}>
            <strong>{record.workloadId}</strong>
            <span>{record.verdict}</span>
            {record.transferMs !== null && <em>transfer {record.transferMs.toFixed(2)} ms</em>}
          </li>
        ))}
      </ul>
    </div>
  )
}