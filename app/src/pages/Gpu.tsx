import { Area, CartesianGrid, ComposedChart, Line, ResponsiveContainer, Tooltip, XAxis, YAxis } from 'recharts'
import { Info, MonitorCog } from 'lucide-react'
import { useMonitoringStore } from '../stores/monitoring'
import {
  Card, ChartFrame, DetailList, Disclose, EmptyState, PageHeader, RXTooltip,
  ResourceBar, StatusChip,
} from '../ui/kit'
import { fmtBytes, fmtTime, pct, statusForPercent } from '../lib/format'
import { rxAxis, rxGrid, rxTick } from '../ui/chart-config'

export default function GpuPage() {
  const snapshot = useMonitoringStore((state) => state.snapshot)
  const history = useMonitoringStore((state) => state.history)
  if (!snapshot) return null
  const gpus = snapshot.gpus
  const rows = history.map((point) => ({
    label: fmtTime(point.timestamp),
    gpu: point.gpuPercent,
    vram: point.vramUsedBytes == null || !gpus[0] || gpus[0].dedicatedTotalBytes == null
      ? null
      : (point.vramUsedBytes / gpus[0].dedicatedTotalBytes) * 100,
  }))

  if (gpus.length === 0) {
    return <>
      <PageHeader title="GPU" description="Graphics card utilization, temperature and VRAM — kept clearly separate from system RAM." />
      <Card><EmptyState icon={MonitorCog} title="No GPU detected"
        detail="Windows did not report a display adapter. FluxCore will show GPU metrics automatically when one appears — nothing is estimated in the meantime." /></Card>
    </>
  }

  return <>
    <PageHeader title="GPU" description="Graphics card utilization, temperature and VRAM — kept clearly separate from system RAM." />

    {gpus.map((gpu, index) => {
      const status = statusForPercent(gpu.utilizationPercent)
      const total = gpu.dedicatedTotalBytes
      const usedPercent = gpu.vramUsagePercent
      return <div className="rx-stack" key={`${gpu.name}-${index}`}>
        <div className="rx-hero">
          <div className="rx-hero-value">{pct(gpu.utilizationPercent).replace('%', '')}<small>{gpu.utilizationPercent == null ? '' : '%'}</small></div>
          <div className="rx-hero-text">
            <h2>{gpu.name}</h2>
            <p>{gpu.vendor} · {gpu.gpuClass ?? 'adapter class unavailable'}{gpu.driverVersion ? ` · driver ${gpu.driverVersion}` : ''}</p>
          </div>
          <div className="rx-hero-side">
            <StatusChip status={status} />
            <StatusChip status={gpu.temperatureC == null ? 'unavailable' : gpu.temperatureC >= 85 ? 'critical' : gpu.temperatureC >= 75 ? 'attention' : 'normal'}
              label={gpu.temperatureC == null ? 'Temp unavailable' : `${gpu.temperatureC.toFixed(0)} °C`} />
          </div>
        </div>

        <div className="rx-gpu-trio">
          <Card title="GPU utilization" icon={MonitorCog} note="How busy the chip is">
            <ResourceBar label="Utilization" value={pct(gpu.utilizationPercent)} percent={gpu.utilizationPercent} color="var(--rx-c-gpu)" />
            <ResourceBar label="Memory controller" value={pct(gpu.memoryUtilizationPercent)} percent={gpu.memoryUtilizationPercent} color="var(--rx-c-gpu)"
              tip="How busy the GPU's own memory bus is. High values with low utilization can indicate memory-bound workloads." />
          </Card>
          <Card title="Dedicated VRAM" icon={MonitorCog} note="The card's own memory">
            <ResourceBar label="Used" value={fmtBytes(gpu.dedicatedUsedBytes)} total={fmtBytes(total)} percent={usedPercent} color="var(--rx-c-vram)" />
            <ResourceBar label="Available" value={fmtBytes(gpu.dedicatedAvailableBytes)} total={fmtBytes(total)}
              percent={total ? 100 - (usedPercent ?? 0) : null} color="var(--rx-neutral)" />
            <p className="rx-chart-caption" style={{ display: 'flex', gap: 6, alignItems: 'flex-start' }}>
              <Info size={13} style={{ marginTop: 2, flexShrink: 0 }} />
              <span>VRAM is the graphics card's own memory — separate from system RAM. It cannot be used as extra RAM, and FluxCore never treats it as such.</span>
            </p>
          </Card>
          <Card title="Shared GPU memory" icon={MonitorCog} note="Borrowed from RAM">
            <ResourceBar label="In use" value={fmtBytes(gpu.sharedUsedBytes)} percent={null} color="var(--rx-c-other)"
              tip="When VRAM runs out, Windows lends the GPU part of system RAM (up to half). This is slower than VRAM and reduces the RAM available to apps." />
            <p className="rx-chart-caption">Shared memory usage above a few hundred MB usually means a workload wants more dedicated VRAM than the card has.</p>
          </Card>
        </div>
      </div>
    })}

    <Card title="GPU & VRAM history" icon={MonitorCog} note="Measured, never estimated">
      {rows.filter((row) => row.gpu != null || row.vram != null).length < 2
        ? <EmptyState title="Collecting history" detail="The chart appears as soon as real samples are collected." />
        : <ChartFrame ariaLabel="GPU utilization and VRAM usage history"
            caption="Amber: how busy the GPU chip is. Rose: how much of the card's own memory (VRAM) is used. The two lines are independent — high VRAM with low utilization is normal when models stay loaded.">
            <ResponsiveContainer width="100%" height="100%">
              <ComposedChart data={rows} margin={{ top: 8, right: 8, bottom: 0, left: -18 }}>
                <defs><linearGradient id="rxGpuFill" x1="0" y1="0" x2="0" y2="1">
                  <stop offset="0%" stopColor="var(--rx-c-gpu)" stopOpacity={0.24} />
                  <stop offset="100%" stopColor="var(--rx-c-gpu)" stopOpacity={0} />
                </linearGradient></defs>
                <CartesianGrid {...rxGrid} />
                <XAxis dataKey="label" {...rxAxis} tick={rxTick} minTickGap={44} />
                <YAxis domain={[0, 100]} {...rxAxis} tick={rxTick} tickFormatter={(v) => `${v}%`} />
                <Tooltip content={<RXTooltip unit="%" />} />
                <Area dataKey="gpu" name="GPU utilization" type="monotone" stroke="var(--rx-c-gpu)" strokeWidth={2} fill="url(#rxGpuFill)" isAnimationActive={false} connectNulls />
                <Line dataKey="vram" name="VRAM used" type="monotone" stroke="var(--rx-c-vram)" strokeWidth={2} fill="none" dot={false} isAnimationActive={false} connectNulls />
              </ComposedChart>
            </ResponsiveContainer>
          </ChartFrame>}
      <div className="rx-legend">
        <span><i style={{ background: 'var(--rx-c-gpu)' }} />GPU utilization</span>
        <span><i style={{ background: 'var(--rx-c-vram)' }} />VRAM used (% of dedicated)</span>
      </div>
    </Card>

    <Card>
      <Disclose summary="Technical details">
        <div className="rx-disclose-body">
          {gpus.map((gpu, index) => <DetailList key={`${gpu.name}-${index}`} rows={[
            { label: 'Provider', value: gpu.provider, tip: 'The data source FluxCore uses. NVIDIA metrics come through NVML; other vendors currently provide device identification only.' },
            { label: 'GPU class', value: gpu.gpuClass ?? 'Unavailable' },
            { label: 'Driver version', value: gpu.driverVersion ?? 'Unavailable' },
            { label: 'Core clock', value: gpu.coreClockMhz == null ? 'Unavailable' : `${gpu.coreClockMhz} MHz` },
            { label: 'Memory clock', value: gpu.memoryClockMhz == null ? 'Unavailable' : `${gpu.memoryClockMhz} MHz` },
            { label: 'Power draw', value: gpu.powerWatts == null ? 'Unavailable' : `${gpu.powerWatts.toFixed(1)} W` },
            { label: 'Fan speed', value: gpu.fanPercent == null ? 'Unavailable' : `${gpu.fanPercent}%` },
            { label: 'Temperature', value: gpu.temperatureC == null ? 'Unavailable' : `${gpu.temperatureC.toFixed(1)} °C` },
          ]} />)}
        </div>
      </Disclose>
    </Card>
  </>
}
