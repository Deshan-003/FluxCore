import { Zap } from 'lucide-react'
import type { ProviderStatus } from '../../types/gpu'
import { fmtBytes } from '../../lib/format'
import { Detail, Panel, SensorNotice } from '../monitoring'

export default function GpuStatusCard({ provider }: { provider: ProviderStatus }) {
  const device = provider.devices[0]
  return (
    <Panel title={provider.label} icon={Zap} className="gpu-status-card">
      <div className="gpu-grid">
        <Detail label="Provider id" value={provider.id} />
        <Detail label="Available" value={provider.available ? 'Yes' : 'No'} />
        <Detail label="Compute ready" value={provider.computeReady ? 'Yes' : 'No'} />
        <Detail label="Selected device" value={provider.selectedDevice || 'Auto'} />
