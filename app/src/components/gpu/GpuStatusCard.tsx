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
        <Detail label="Device" value={device?.name ?? 'Unavailable'} />
        <Detail label="Backend" value={device?.backend ?? 'Unavailable'} />
        <Detail label="Compute capability" value={device?.computeCapability ?? 'Unavailable'} />
        <Detail label="Driver" value={device?.driverVersion ?? 'Unavailable'} />
        <Detail label="Dedicated VRAM" value={fmtBytes(device?.dedicatedTotalBytes)} />
        <Detail label="Free VRAM" value={fmtBytes(device?.dedicatedFreeBytes)} />
        <Detail label="Supported workloads" value={String(provider.supportedWorkloads.length)} />
        <Detail label="Discrete GPU" value={device ? (device.isDiscrete ? 'Yes' : 'No') : 'Unavailable'} />
      </div>
      {!provider.computeReady && provider.detail && <SensorNotice text={provider.detail} />}
      {provider.lastError && <SensorNotice text={provider.lastError} />}
      {provider.capabilities.backendNotes.map((note) => <SensorNotice key={note} text={note} />)}
    </Panel>
  )
}