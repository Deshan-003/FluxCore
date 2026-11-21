import { Area, AreaChart, CartesianGrid, ResponsiveContainer, Tooltip, XAxis, YAxis } from 'recharts'
import { Cpu } from 'lucide-react'
import { useMonitoringStore } from '../stores/monitoring'
import {
  Card, ChartFrame, DetailList, Disclose, EmptyState, PageHeader, RXTooltip, StatusChip,
} from '../ui/kit'
import { fmtTime, pct, statusForPercent } from '../lib/format'
import { rxAxis, rxGrid, rxTick } from '../ui/chart-config'

export default function CpuPage() {
  const snapshot = useMonitoringStore((state) => state.snapshot)
  const history = useMonitoringStore((state) => state.history)
  if (!snapshot) return null
  const cpu = snapshot.cpu
  const status = statusForPercent(cpu.usagePercent)
  const rows = history.map((point) => ({ label: fmtTime(point.timestamp), cpu: point.cpuPercent }))

  return <>
    <PageHeader title="CPU" description="Processor utilization, speed and temperature — with plain-language explanations." />

    <div className="rx-hero">
      <div className="rx-hero-value">{pct(cpu.usagePercent).replace('%', '')}<small>%</small></div>
      <div className="rx-hero-text">
        <h2>{cpu.name}</h2>
        <p>{cpu.physicalCores ?? '—'} physical cores · {cpu.logicalProcessors} logical processors · currently at {cpu.frequencyMhz == null ? 'Unavailable' : `${cpu.frequencyMhz.toLocaleString()} MHz`}</p>
      </div>
      <div className="rx-hero-side">
        <StatusChip status={status} />
        <StatusChip status={cpu.temperatureC == null ? 'unavailable' : cpu.temperatureC >= 85 ? 'critical' : cpu.temperatureC >= 75 ? 'attention' : 'normal'}
          label={cpu.temperatureC == null ? 'Temp unavailable' : `${cpu.temperatureC.toFixed(0)} °C`} />
      </div>
    </div>

    <div className="rx-grid-side">
      <div className="rx-stack">
        <Card title="Utilization history" icon={Cpu} note="Measured, never estimated">
          {rows.length < 2
            ? <EmptyState title="Collecting history" detail="The chart appears as soon as real samples are collected." />
            : <ChartFrame ariaLabel="CPU utilization over the recorded window" caption="Share of time the processor spent busy across all cores. Brief spikes are normal; sustained high values mean the CPU is the bottleneck for what you run.">
                <ResponsiveContainer width="100%" height="100%">
                  <AreaChart data={rows} margin={{ top: 8, right: 8, bottom: 0, left: -18 }}>
                    <defs><linearGradient id="rxCpuFill" x1="0" y1="0" x2="0" y2="1">
                      <stop offset="0%" stopColor="var(--rx-c-cpu)" stopOpacity={0.25} />
                      <stop offset="100%" stopColor="var(--rx-c-cpu)" stopOpacity={0} />
                    </linearGradient></defs>
                    <CartesianGrid {...rxGrid} />
                    <XAxis dataKey="label" {...rxAxis} tick={rxTick} minTickGap={44} />
                    <YAxis domain={[0, 100]} {...rxAxis} tick={rxTick} tickFormatter={(v) => `${v}%`} />
                    <Tooltip content={<RXTooltip unit="%" />} />
                    <Area dataKey="cpu" name="CPU utilization" type="monotone" stroke="var(--rx-c-cpu)" strokeWidth={2} fill="url(#rxCpuFill)" isAnimationActive={false} connectNulls />
                  </AreaChart>
                </ResponsiveContainer>
              </ChartFrame>}
        </Card>

        {cpu.cores.length > 0 && <Card title="Per-core utilization" icon={Cpu} note="Live snapshot">
          <div className="rx-core-grid">
            {cpu.cores.map((core) => <div className="rx-core-cell" key={core.id}>
              <div className="labels"><span>Core {String(core.id + 1).padStart(2, '0')}</span><b>{Math.round(core.usagePercent)}%</b></div>
              <div className="rx-metric-bar" style={{ ['--rx-metric-color' as string]: 'var(--rx-c-cpu)' }}><span style={{ width: `${core.usagePercent}%` }} /></div>
            </div>)}
          </div>
        </Card>}
      </div>

      <div className="rx-stack">
        <Card title="Top CPU consumers" icon={Cpu} note="Right now">
          {snapshot.processes.length === 0
            ? <EmptyState title="No process data" detail="Process enumeration is not available from the current collector." />
            : <div className="rx-table-wrap"><table className="rx-table">
                <thead><tr><th>Process</th><th className="num">CPU</th></tr></thead>
                <tbody>
                  {snapshot.processes.slice().sort((a, b) => (b.cpuPercent ?? -1) - (a.cpuPercent ?? -1)).slice(0, 6).map((process) => (
                    <tr key={process.pid}>
                      <td><span className="rx-process-name"><span className="rx-process-dot" style={{ background: 'var(--rx-c-cpu)' }} />{process.name}</span></td>
                      <td className="num">{process.cpuPercent == null ? 'Unavailable' : `${process.cpuPercent.toFixed(1)}%`}</td>
                    </tr>
                  ))}
                </tbody>
              </table></div>}
        </Card>

        <Card title="Processor details" icon={Cpu}>
          <DetailList rows={[
            { label: 'Manufacturer', value: cpu.vendor },
            { label: 'Architecture', value: cpu.architecture ?? 'Unavailable' },
            { label: 'Current frequency', value: cpu.frequencyMhz == null ? 'Unavailable' : `${cpu.frequencyMhz.toLocaleString()} MHz`, tip: 'Live speed the cores run at right now. It rises under load and drops to save power.' },
            { label: 'Maximum frequency', value: cpu.maxFrequencyMhz == null ? 'Unavailable' : `${cpu.maxFrequencyMhz.toLocaleString()} MHz` },
            { label: 'Physical cores', value: cpu.physicalCores == null ? 'Unavailable' : String(cpu.physicalCores), tip: 'Physical processing units. Each can run independent work.' },
            { label: 'Logical processors', value: String(cpu.logicalProcessors), tip: 'Threads the OS sees — usually two per physical core.' },
            { label: 'Temperature', value: cpu.temperatureC == null ? 'Unavailable' : `${cpu.temperatureC.toFixed(1)} °C`, tip: 'Requires a supported hardware sensor. FluxCore never estimates temperatures.' },
          ]} />
        </Card>
      </div>
    </div>

    <Card>
      <Disclose summary="How FluxCore measures CPU usage">
        <div className="rx-disclose-body">
          <p className="rx-card-sub" style={{ margin: 0 }}>
            Utilization comes from Windows processor-performance counters — the same numbers Task Manager shows. A sensor the OS doesn't expose
            (for example, temperature on some systems) is displayed as <strong>Unavailable</strong> rather than estimated. FluxCore never terminates
            processes or changes processor settings on this page.
          </p>
        </div>
      </Disclose>
    </Card>
  </>
}
