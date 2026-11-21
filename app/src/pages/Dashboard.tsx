import {
  AlertOctagon, ArrowRight, CheckCircle2, Cpu, Database, HardDrive, Info,
  MemoryStick, MonitorCog, RefreshCw, ShieldCheck, SquareActivity, Zap,
} from 'lucide-react'
import { useMonitoringStore } from '../stores/monitoring'
import {
  Button, Card, DetailList, Disclose, EmptyState, InfoTip, MetricCard, PageHeader,
  ResourceBar, StatusChip,
} from '../ui/kit'
import { fmtBytes, pct, statusForPercent, type RxStatus } from '../lib/format'

// Rule-based, fully transparent insights derived from the live snapshot.
// FluxCore's statistical analysis lives in Resource Intelligence; the dashboard
// only states threshold facts and links to the page that explains them.
interface Insight { severity: 'critical' | 'warning' | 'info' | 'success'; title: string; body: string; target?: string; targetLabel?: string }

export default function Dashboard({ onNavigate }: { onNavigate: (id: string) => void }) {
  const snapshot = useMonitoringStore((state) => state.snapshot)
  const history = useMonitoringStore((state) => state.history)
  if (!snapshot) {
    return <>
      <PageHeader title="Dashboard" description="A plain-language overview of your computer's health and resource usage." />
      <Card>
        <EmptyState icon={ShieldCheck} title="Waiting for the monitoring engine"
          detail="FluxCore will show measurements here as soon as the Windows collector responds. Nothing is estimated in the meantime."
          action={<Button variant="primary" icon={RefreshCw}>Open troubleshooting</Button>} />
      </Card>
    </>
  }

  const gpu = snapshot.gpus.find((item) => item.utilizationPercent !== null) ?? snapshot.gpus[0]
  const cpuStatus = statusForPercent(snapshot.cpu.usagePercent)
  const memStatus = statusForPercent(snapshot.memory.usagePercent)
  const gpuStatus = statusForPercent(gpu?.utilizationPercent ?? null)
  const vramStatus = statusForPercent(gpu?.vramUsagePercent ?? null)
  const storageStatus = statusForPercent(snapshot.storage[0]?.usagePercent ?? null)
  const statuses: [string, RxStatus][] = [
    ['CPU', cpuStatus], ['Memory', memStatus], ['GPU', gpuStatus], ['VRAM', vramStatus], ['Storage', storageStatus],
  ]
  const worst: RxStatus = statuses.some(([, s]) => s === 'critical') ? 'critical'
    : statuses.some(([, s]) => s === 'attention') ? 'attention'
    : statuses.every(([, s]) => s === 'healthy' || s === 'normal') ? 'healthy'
    : 'normal'
  const worstText = worst === 'critical' ? 'A resource is saturated and performance may be affected'
    : worst === 'attention' ? 'One or more resources are working hard'
    : 'All monitored systems are within normal ranges'

  const insights: Insight[] = []
  if (memStatus === 'critical' || memStatus === 'attention') {
    insights.push({
      severity: memStatus === 'critical' ? 'critical' : 'warning',
      title: `Memory usage is ${snapshot.memory.usagePercent == null ? 'high' : `${Math.round(snapshot.memory.usagePercent)}%`}`,
      body: `Physical RAM in use is at ${pct(snapshot.memory.usagePercent)} with ${fmtBytes(snapshot.memory.availableBytes)} still available. Open Memory Management to see which apps use the most and which safe cleanup actions exist.`,
      target: 'memory-management', targetLabel: 'Open Memory Management',
    })
  }
  if (cpuStatus === 'critical' || cpuStatus === 'attention') {
    insights.push({
      severity: cpuStatus === 'critical' ? 'warning' : 'info',
      title: `CPU utilization is ${pct(snapshot.cpu.usagePercent)}`,
      body: 'Sustained high processor load usually means a background task is busy. Check the top consumers on the CPU page.',
      target: 'cpu', targetLabel: 'View CPU details',
    })
  }
  if (vramStatus === 'critical' || vramStatus === 'attention') {
    insights.push({
      severity: 'warning',
      title: 'VRAM is running low',
      body: `${pct(gpu?.vramUsagePercent ?? null)} of the graphics card's dedicated VRAM is in use. New GPU tasks may fail or fall back to the CPU.`,
      target: 'gpu-acceleration', targetLabel: 'Open GPU Acceleration',
    })
  }
  if (storageStatus === 'critical') {
    insights.push({
      severity: 'warning',
      title: `${snapshot.storage[0]?.name ?? 'System drive'} is almost full`,
      body: 'Windows needs free space for updates and virtual memory; below ~15% free the system can slow down.',
      target: 'storage', targetLabel: 'View storage',
    })
  }
  if (gpu && gpu.utilizationPercent != null && gpu.utilizationPercent < 30 && (gpu.vramUsagePercent ?? 100) < 70) {
    insights.push({
      severity: 'info',
      title: 'Good time for heavy jobs',
      body: `The GPU has headroom (${pct(gpu.utilizationPercent)} busy) — a good moment to run GPU-accelerated workloads.`,
      target: 'gpu-acceleration', targetLabel: 'Open GPU Acceleration',
    })
  }
  if (insights.length === 0) {
    insights.push({ severity: 'success', title: 'Everything looks healthy', body: 'CPU, memory, GPU, VRAM and storage are all in their normal ranges. No action needed.' })
    if (gpu?.dedicatedAvailableBytes != null && gpu.dedicatedAvailableBytes > 1024 ** 3) {
      insights.push({ severity: 'info', title: 'Reclaimable memory available', body: 'Cached/standby RAM is returned to apps on demand — no action required. VRAM also has headroom for new workloads.', target: 'memory-management', targetLabel: 'How memory works' })
    }
  }

  const spark = (key: 'cpuPercent' | 'memoryPercent' | 'gpuPercent') =>
    history.map((point) => ({ t: point.timestamp, v: point[key] }))
  const vramSpark = history.map((point) => ({
    t: point.timestamp,
    v: point.vramUsedBytes == null || !gpu || gpu.dedicatedTotalBytes == null ? null : (point.vramUsedBytes / gpu.dedicatedTotalBytes) * 100,
  }))
  const chipLabel = (status: RxStatus) =>
    status === 'healthy' ? 'Healthy' : status === 'normal' ? 'Normal' : status === 'attention' ? 'Needs attention' : status === 'critical' ? 'Critical' : 'Unavailable'

  return <>
    <PageHeader title="Dashboard" description="A plain-language overview of your computer's health and resource usage. Click any card for details." />

    <div className="rx-hero">
      <span style={{ color: worst === 'critical' ? 'var(--rx-danger)' : worst === 'attention' ? 'var(--rx-warning)' : 'var(--rx-success)' }}>
        {worst === 'critical' ? <AlertOctagon size={34} /> : worst === 'attention' ? <ShieldCheck size={34} /> : <CheckCircle2 size={34} />}
      </span>
      <div className="rx-hero-text">
        <h2>{worst === 'critical' ? 'System needs attention now' : worst === 'attention' ? 'System needs attention' : 'System is healthy'}</h2>
        <p>{worstText}. Based on the highest utilization across CPU, memory, GPU and VRAM<InfoTip text="Status thresholds: Healthy below 30%, Normal below 60%, Needs attention at 60% or more, Critical at 85% or more. Storage counts when above 85% full." /></p>
      </div>
      <div className="rx-hero-side">
        {statuses.map(([name, status]) => <StatusChip key={name} status={status} label={`${name}: ${chipLabel(status)}`} />)}
      </div>
    </div>

    <div className="rx-metric-row">
      <MetricCard icon={Cpu} name="CPU" caption={`${snapshot.cpu.logicalProcessors} logical processors`}
        value={pct(snapshot.cpu.usagePercent).replace('%', '')} unit={snapshot.cpu.usagePercent == null ? undefined : '%'}
        percent={snapshot.cpu.usagePercent} color="var(--rx-c-cpu)" history={spark('cpuPercent')} status={cpuStatus} onOpen={() => onNavigate('cpu')} />
      <MetricCard icon={MemoryStick} name="Memory" caption={`${fmtBytes(snapshot.memory.availableBytes)} available of ${fmtBytes(snapshot.memory.totalBytes)}`}
        value={fmtBytes(snapshot.memory.usedBytes)} percent={snapshot.memory.usagePercent} color="var(--rx-c-memory)"
        history={spark('memoryPercent')} status={memStatus} onOpen={() => onNavigate('memory')} />
      <MetricCard icon={MonitorCog} name="GPU" caption={gpu?.name ?? 'No GPU detected'}
        value={gpu ? pct(gpu.utilizationPercent).replace('%', '') : '—'} unit={gpu?.utilizationPercent == null ? undefined : '%'}
        percent={gpu?.utilizationPercent ?? null} color="var(--rx-c-gpu)" history={spark('gpuPercent')} status={gpuStatus} onOpen={() => onNavigate('gpu')} />
      <MetricCard icon={Database} name="VRAM" caption={gpu ? `of ${fmtBytes(gpu.dedicatedTotalBytes)} dedicated` : 'Dedicated VRAM unavailable'}
        value={gpu ? fmtBytes(gpu.dedicatedUsedBytes) : '—'} percent={gpu?.vramUsagePercent ?? null} color="var(--rx-c-vram)"
        history={vramSpark} status={vramStatus} onOpen={() => onNavigate('gpu')} />
      <MetricCard icon={HardDrive} name="Storage" caption={snapshot.storage[0] ? `C: · ${snapshot.storage[0].kind} · ${fmtBytes(snapshot.storage[0].totalBytes)}` : 'No volumes found'}
        value={snapshot.storage[0] ? fmtBytes(snapshot.storage[0].availableBytes) : '—'} unit={snapshot.storage[0] ? 'free' : undefined}
        percent={snapshot.storage[0]?.usagePercent ?? null} color="var(--rx-c-storage)" history={[]} status={storageStatus} onOpen={() => onNavigate('storage')} />
    </div>

    <div className="rx-grid-side">
      <Card title="What FluxCore noticed" icon={Info} note="Rule-based facts from the live snapshot">
        <div>
          {insights.map((insight) => <div className="rx-insight" key={insight.title}>
            <span className={`rx-insight-icon ${insight.severity}`}>
              {insight.severity === 'critical' || insight.severity === 'warning' ? <AlertOctagon size={16} /> : insight.severity === 'success' ? <CheckCircle2 size={16} /> : <Info size={16} />}
            </span>
            <div>
              <h3>{insight.title}</h3>
              <p>{insight.body}</p>
              {insight.target && <div className="rx-insight-link">
                <button className="rx-text-link" onClick={() => onNavigate(insight.target!)}>{insight.targetLabel}<ArrowRight size={13} /></button>
              </div>}
            </div>
          </div>)}
        </div>
      </Card>

      <div className="rx-stack">
        <Card title="Quick actions" icon={Zap}>
          <div className="rx-stack" style={{ gap: 8 }}>
            <Button icon={SquareActivity} onClick={() => onNavigate('processes')}>See what's running</Button>
            <Button icon={MemoryStick} onClick={() => onNavigate('memory-management')}>Memory management</Button>
            <Button icon={Zap} onClick={() => onNavigate('gpu-acceleration')}>GPU acceleration</Button>
            <Button icon={ShieldCheck} onClick={() => onNavigate('resource-intelligence')}>Review recommendations</Button>
          </div>
        </Card>
        <Card title="Memory at a glance" icon={MemoryStick} right={<button className="rx-text-link" onClick={() => onNavigate('memory')}>Details<ArrowRight size={13} /></button>}>
          <ResourceBar label="In use" value={fmtBytes(snapshot.memory.usedBytes)} total={fmtBytes(snapshot.memory.totalBytes)}
            percent={snapshot.memory.usagePercent} color="var(--rx-c-memory)" />
          <ResourceBar label="Committed" value={fmtBytes(snapshot.memory.committedBytes)} total={fmtBytes(snapshot.memory.commitLimitBytes)}
            percent={snapshot.memory.committedBytes == null || snapshot.memory.commitLimitBytes == null ? null : (snapshot.memory.committedBytes / snapshot.memory.commitLimitBytes) * 100}
            color="var(--rx-c-other)"
            tip="Committed memory is what Windows has promised to all running apps — it counts RAM plus the pagefile. It is normal for it to exceed physical RAM." />
        </Card>
      </div>
    </div>

    <Card>
      <Disclose summary="Technical details">
        <div className="rx-disclose-body">
          <DetailList rows={[
            { label: 'Processor', value: snapshot.cpu.name },
            { label: 'Logical processors', value: String(snapshot.cpu.logicalProcessors) },
            { label: 'CPU frequency', value: snapshot.cpu.frequencyMhz == null ? 'Unavailable' : `${snapshot.cpu.frequencyMhz.toLocaleString()} MHz` },
            { label: 'CPU temperature', value: snapshot.cpu.temperatureC == null ? 'Unavailable' : `${snapshot.cpu.temperatureC.toFixed(1)} °C`, tip: 'Requires a supported hardware sensor. FluxCore never estimates temperatures.' },
            { label: 'Memory total', value: fmtBytes(snapshot.memory.totalBytes) },
            { label: 'Commit limit', value: fmtBytes(snapshot.memory.commitLimitBytes), tip: 'RAM plus pagefile capacity — the most memory Windows can promise at once.' },
            { label: 'Pagefile in use', value: fmtBytes(snapshot.memory.pagefileUsedBytes) },
            { label: 'GPU driver', value: gpu?.driverVersion ?? 'Unavailable' },
            { label: 'GPU core clock', value: gpu?.coreClockMhz == null ? 'Unavailable' : `${gpu.coreClockMhz} MHz` },
            { label: 'GPU power draw', value: gpu?.powerWatts == null ? 'Unavailable' : `${gpu.powerWatts.toFixed(1)} W` },
            { label: 'Shared GPU memory', value: fmtBytes(gpu?.sharedUsedBytes ?? null), tip: 'A portion of system RAM the GPU may borrow. Separate from dedicated VRAM.' },
            { label: 'Device', value: snapshot.hardware.hostName },
          ]} />
        </div>
      </Disclose>
    </Card>
  </>
}
