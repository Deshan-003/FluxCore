import { useEffect, useMemo, useState } from 'react'
import {
  Activity, Bell, BrainCircuit, ChevronDown, ChevronRight, Cpu, Database, Gauge, HardDrive, LayoutDashboard,
  MemoryStick, MonitorCog, Search, Settings as SettingsIcon, ShieldCheck, SlidersHorizontal,
  Sparkles, SquareActivity, Zap, type LucideIcon,
} from 'lucide-react'
import {
  Area, AreaChart, CartesianGrid, ResponsiveContainer, Tooltip, XAxis, YAxis,
} from 'recharts'
import { useMonitoringStore } from './stores/monitoring'
import { useResourceStore } from './stores/intelligence'
import { useMemoryStore } from './stores/memory'
import MemoryIntelligencePage from './pages/MemoryIntelligence'
import OptimizationCenterPage from './pages/OptimizationCenter'
import GpuAccelerationPage from './pages/GpuAcceleration'
import ResourceIntelligencePage from './pages/ResourceIntelligence'
import {
  Detail, EmptyState, GpuMetric, Panel, ResourceBar, SensorNotice, SettingRow, Toggle,
} from './components/monitoring'
import { fmtBytes, fmtRate, metricSeriesKey, pct, statusFor, titleCase } from './lib/format'
import { filterProcesses } from './lib/processes'
import type { CpuMetrics, GpuMetrics, HardwareCheckItem, HistoryPoint, MemoryMetrics, ProcessMetrics, StorageVolume } from './types/monitoring'
import { checkHardware } from './services/monitoring'
import './App.css'

type NavItem = { label: string; icon: LucideIcon; group: string }
const NAV: NavItem[] = [
  { label: 'Overview', icon: LayoutDashboard, group: 'WORKSPACE' },
  { label: 'Resource Intelligence', icon: BrainCircuit, group: 'WORKSPACE' },
  { label: 'CPU', icon: Cpu, group: 'MONITORS' },
  { label: 'Memory', icon: MemoryStick, group: 'MONITORS' },
  { label: 'GPU', icon: MonitorCog, group: 'MONITORS' },
  { label: 'Storage', icon: HardDrive, group: 'MONITORS' },
  { label: 'Memory Intelligence', icon: BrainCircuit, group: 'MEMORY INTELLIGENCE' },
  { label: 'Optimization Center', icon: Sparkles, group: 'MEMORY INTELLIGENCE' },
  { label: 'GPU Acceleration', icon: Zap, group: 'GPU INTELLIGENCE' },
  { label: 'Processes', icon: SquareActivity, group: 'SYSTEM' },
  { label: 'Hardware', icon: Database, group: 'SYSTEM' },
  { label: 'Settings', icon: SettingsIcon, group: 'SYSTEM' },
]
function MetricTile({ icon: Icon, title, value, unit, sub, percent, data, color, onClick }: {
  icon: LucideIcon; title: string; value: string; unit?: string; sub: string; percent: number | null; data: HistoryPoint[]; color: string; onClick: () => void
}) {
  const key = metricSeriesKey(title)
  const chartData = data.slice(-24)
  return <button className={`metric-card ${statusFor(percent)}`} onClick={onClick}>
    <div className="metric-top"><span className="metric-icon" style={{ color }}><Icon size={17} /></span><span className="metric-title">{title}</span><ChevronRight size={15} className="metric-arrow" /></div>
    <div className="metric-main"><div><span className="metric-value">{value}</span>{unit && <span className="metric-unit">{unit}</span>}</div><span className={`health-dot ${statusFor(percent)}`} /></div>
    <div className="metric-sub">{sub}</div>
    <div className="metric-bottom"><span className="mini-chart"><ResponsiveContainer width="100%" height="100%"><AreaChart data={chartData}><defs><linearGradient id={`fill-${title}`} x1="0" y1="0" x2="0" y2="1"><stop offset="0%" stopColor={color} stopOpacity={0.32} /><stop offset="100%" stopColor={color} stopOpacity={0} /></linearGradient></defs><Area dataKey={key} type="monotone" stroke={color} strokeWidth={1.7} fill={`url(#fill-${title})`} isAnimationActive={false} connectNulls /></AreaChart></ResponsiveContainer></span><span className="metric-percent">{pct(percent)}</span></div>
  </button>
}

function ChartPanel({ history, title = 'Resource activity', memory }: { history: HistoryPoint[]; title?: string; memory?: boolean }) {
  const { settings } = useMonitoringStore()
  // Window by timestamp so the visible span honors the setting regardless of
  // the collection cadence (1 s live stream vs 5 s SQLite rows).
  const latest = history.reduce((max, point) => Math.max(max, new Date(point.timestamp).getTime()), 0)
  const cutoff = latest - settings.historyDurationMinutes * 60_000
  const rows = history
    .filter((point) => new Date(point.timestamp).getTime() >= cutoff)
    .map((point) => ({ ...point, label: new Date(point.timestamp).toLocaleTimeString([], { hour: '2-digit', minute: '2-digit', second: '2-digit' }) }))
  return <Panel title={title} icon={Activity} action={<span className="chart-period"><span className="live-dot" />LIVE</span>} className="chart-panel">
    {rows.length < 2 ? <EmptyState title="Collecting history" detail="The chart will appear as real samples are collected." /> : <div className="chart-wrap"><ResponsiveContainer width="100%" height="100%"><AreaChart data={rows} margin={{ top: 12, right: 8, bottom: 0, left: -16 }}>
      <defs><linearGradient id="cpuFill" x1="0" y1="0" x2="0" y2="1"><stop offset="0%" stopColor="#56c7e8" stopOpacity={0.22} /><stop offset="100%" stopColor="#56c7e8" stopOpacity={0} /></linearGradient><linearGradient id="ramFill" x1="0" y1="0" x2="0" y2="1"><stop offset="0%" stopColor="#a78bfa" stopOpacity={0.22} /><stop offset="100%" stopColor="#a78bfa" stopOpacity={0} /></linearGradient></defs>
      <CartesianGrid stroke="#202b3b" strokeDasharray="3 5" vertical={false} /><XAxis dataKey="label" tick={{ fill: '#778399', fontSize: 10 }} tickLine={false} axisLine={false} minTickGap={38} /><YAxis domain={[0,100]} tick={{ fill: '#778399', fontSize: 10 }} tickLine={false} axisLine={false} tickFormatter={(v) => `${v}%`} /><Tooltip contentStyle={{ background: '#101a29', border: '1px solid #2b3749', borderRadius: 9, color: '#e8eef7', fontSize: 12 }} labelStyle={{ color: '#92a1b5' }} />
      <Area dataKey="cpuPercent" name="CPU" type="monotone" stroke="#56c7e8" strokeWidth={2} fill="url(#cpuFill)" isAnimationActive={false} />
      <Area dataKey="memoryPercent" name="Memory" type="monotone" stroke="#a78bfa" strokeWidth={2} fill="url(#ramFill)" isAnimationActive={false} />
      {!memory && <Area dataKey="gpuPercent" name="GPU" type="monotone" stroke="#f4b75e" strokeWidth={1.8} fill="none" isAnimationActive={false} connectNulls />}
    </AreaChart></ResponsiveContainer></div>}
    <div className="chart-legend"><span><i className="legend-cyan" />CPU</span><span><i className="legend-violet" />Memory</span><span><i className="legend-amber" />GPU</span></div>
  </Panel>
}

function Overview({ onPage }: { onPage: (page: string) => void }) {
  const { snapshot, history } = useMonitoringStore()
  if (!snapshot) return <EmptyState title="Waiting for the monitoring engine" detail="FluxCore will show measurements as soon as the Windows collector responds." />
  const gpu = snapshot.gpus.find((item) => item.utilizationPercent !== null) ?? snapshot.gpus[0]
  const vramPercent = gpu?.dedicatedTotalBytes && gpu.dedicatedUsedBytes !== null ? gpu.dedicatedUsedBytes! * 100 / gpu.dedicatedTotalBytes : null
  const hasIo = snapshot.storage.some((disk) => disk.readBytesPerSecond !== null || disk.writeBytesPerSecond !== null)
  const diskRead = hasIo ? snapshot.storage.reduce((sum, disk) => sum + (disk.readBytesPerSecond ?? 0), 0) : null
  const diskWrite = hasIo ? snapshot.storage.reduce((sum, disk) => sum + (disk.writeBytesPerSecond ?? 0), 0) : null
  const cpuUse = snapshot.cpu.usagePercent
  const memUse = snapshot.memory.usagePercent
  const worst = cpuUse == null || memUse == null ? null : Math.max(cpuUse, memUse)
  return <>
    <div className="metric-grid">
      <MetricTile icon={Cpu} title="CPU" value={cpuUse == null ? '—' : `${Math.round(cpuUse)}`} unit={cpuUse == null ? undefined : '%'} sub={`${snapshot.cpu.logicalProcessors} logical processors`} percent={cpuUse} data={history} color="#56c7e8" onClick={() => onPage('CPU')} />
      <MetricTile icon={MemoryStick} title="Memory" value={fmtBytes(snapshot.memory.usedBytes)} sub={`${fmtBytes(snapshot.memory.availableBytes)} available · physical`} percent={memUse} data={history} color="#a78bfa" onClick={() => onPage('Memory')} />
      <MetricTile icon={MonitorCog} title="GPU" value={gpu?.utilizationPercent == null ? '—' : `${Math.round(gpu.utilizationPercent)}`} unit={gpu?.utilizationPercent == null ? '' : '%'} sub={gpu?.name ?? 'No GPU detected'} percent={gpu?.utilizationPercent ?? null} data={history} color="#f4b75e" onClick={() => onPage('GPU')} />
      <MetricTile icon={Database} title="VRAM" value={gpu?.dedicatedUsedBytes == null ? '—' : fmtBytes(gpu.dedicatedUsedBytes)} sub={gpu?.dedicatedTotalBytes == null ? 'Dedicated VRAM unavailable' : `${fmtBytes(gpu.dedicatedTotalBytes)} dedicated`} percent={vramPercent} data={history} color="#f186ab" onClick={() => onPage('GPU')} />
      <button className={`metric-card ${hasIo ? '' : 'unavailable-card'}`} onClick={() => onPage('Storage')}><div className="metric-top"><span className="metric-icon" style={{ color: '#53d4a3' }}><HardDrive size={17} /></span><span className="metric-title">Disk activity</span><ChevronRight size={15} className="metric-arrow" /></div><div className="metric-main"><span className={`metric-value ${hasIo ? '' : 'unavailable-value'}`}>{hasIo ? fmtRate(diskRead) : 'Unavailable'}</span><span className={`health-dot ${hasIo ? 'healthy' : 'unknown'}`} /></div><div className="metric-sub">{hasIo ? `write ${fmtRate(diskWrite)}` : 'Read/write counters not provided'}</div><div className="metric-bottom"><span className="metric-sub">Capacity details →</span></div></button>
    </div>
    <div className="overview-grid">
      <div className="overview-main"><ChartPanel history={history} /><Panel title="Memory pressure" icon={MemoryStick} action={<button className="text-button" onClick={() => onPage('Memory')}>Details <ChevronRight size={14} /></button>}>
        <div className="resource-list"><ResourceBar label="Physical memory" value={fmtBytes(snapshot.memory.usedBytes)} total={fmtBytes(snapshot.memory.totalBytes)} percent={memUse} color="violet" /><ResourceBar label="Available" value={fmtBytes(snapshot.memory.availableBytes)} total={fmtBytes(snapshot.memory.totalBytes)} percent={snapshot.memory.availableBytes * 100 / Math.max(1,snapshot.memory.totalBytes)} color="mint" /></div>
        <div className="memory-foot"><span>Committed <strong>{fmtBytes(snapshot.memory.committedBytes)}</strong></span><span>Commit limit <strong>{fmtBytes(snapshot.memory.commitLimitBytes)}</strong></span></div>
      </Panel></div>
      <div className="overview-side"><Panel title="System health" icon={ShieldCheck}><div className="health-summary"><span className={`health-orb ${worst == null ? 'healthy' : worst >= 90 ? 'critical' : worst >= 75 ? 'elevated' : 'healthy'}`}><ShieldCheck size={21} /></span><div><strong>{worst == null ? 'Metrics unavailable' : worst >= 90 ? 'High resource pressure' : worst >= 75 ? 'Elevated usage' : 'Systems healthy'}</strong><span>{worst == null ? 'The collector did not return usable CPU/RAM percentages.' : 'Based on CPU & physical memory thresholds'}</span></div></div><div className="health-lines"><span><i className={statusFor(cpuUse)} />CPU <b>{pct(cpuUse)}</b></span><span><i className={statusFor(memUse)} />Physical RAM <b>{pct(memUse)}</b></span></div></Panel>
        <Panel title="Storage volumes" icon={HardDrive} action={<button className="text-button" onClick={() => onPage('Storage')}>All drives <ChevronRight size={14} /></button>}>
          {snapshot.storage.length ? <div className="volume-list">{snapshot.storage.slice(0,4).map((disk) => <div className="volume-item" key={disk.name}><div className="volume-top"><span><HardDrive size={13} />{disk.name}</span><b>{pct(disk.usagePercent)}</b></div><div className="bar-track"><span className="bar-fill cyan" style={{ width: `${disk.usagePercent ?? 0}%` }} /></div><small>{fmtBytes(disk.availableBytes)} free of {fmtBytes(disk.totalBytes)}</small></div>)}</div> : <EmptyState title="No volumes found" detail="Windows did not report a mounted volume." />}</Panel>
        <Panel title="Disk throughput" icon={Zap}><div className="throughput"><div><span className="throughput-arrow read">↓</span><div><small>READ</small><strong>{fmtRate(diskRead)}</strong></div></div><div><span className="throughput-arrow write">↑</span><div><small>WRITE</small><strong>{fmtRate(diskWrite)}</strong></div></div></div>{hasIo ? <p className="muted-note">Rates are derived from Windows storage performance counters between collection ticks.</p> : <p className="muted-note">Activity counters are unavailable from the current storage provider.</p>}</Panel>
      </div>
    </div>
  </>
}

function CpuPage({ cpu, history }: { cpu: CpuMetrics; history: HistoryPoint[] }) {
  return <div className="page-grid"><div className="page-main"><div className="detail-highlight"><div className="detail-number">{cpu.usagePercent == null ? '—' : Math.round(cpu.usagePercent)}<small>%</small></div><div><span className="eyebrow">TOTAL UTILIZATION</span><h3>{cpu.name}</h3><p>{cpu.vendor} · {cpu.physicalCores ?? '—'} physical cores · {cpu.logicalProcessors} logical processors</p></div><span className={`health-pill ${statusFor(cpu.usagePercent)}`}>{statusFor(cpu.usagePercent)}</span></div><ChartPanel history={history} title="CPU utilization history" /><Panel title="Per-core utilization" icon={Cpu}><div className="core-grid">{cpu.cores.map((core) => <div className="core-cell" key={core.id}><div><span>CORE {String(core.id + 1).padStart(2,'0')}</span><b>{Math.round(core.usagePercent)}%</b></div><div className="bar-track"><span className="bar-fill cyan" style={{ width: `${core.usagePercent}%` }} /></div></div>)}</div></Panel></div><div className="page-side"><Panel title="Processor details" icon={Gauge}><Detail label="Model" value={cpu.name} /><Detail label="Manufacturer" value={cpu.vendor} /><Detail label="Architecture" value={cpu.architecture ?? 'Unavailable'} /><Detail label="Logical processors" value={String(cpu.logicalProcessors)} /><Detail label="Physical cores" value={cpu.physicalCores == null ? 'Unavailable' : String(cpu.physicalCores)} /><Detail label="Current frequency" value={cpu.frequencyMhz == null ? 'Unavailable' : `${cpu.frequencyMhz.toLocaleString()} MHz`} /><Detail label="Base / maximum frequency" value={cpu.maxFrequencyMhz == null ? 'Unavailable' : `${cpu.maxFrequencyMhz.toLocaleString()} MHz`} /><Detail label="Temperature" value={cpu.temperatureC == null ? 'Unavailable · no supported sensor' : `${cpu.temperatureC.toFixed(1)} °C`} /></Panel><SensorNotice text="CPU temperature requires a supported hardware sensor; no value is inferred. Current frequency is the live OS processor-frequency counter where available, otherwise the registered base clock." /></div></div>
}

function MemoryPage({ memory, history }: { memory: MemoryMetrics; history: HistoryPoint[] }) {
  const analysis = useMemoryStore((state) => state.intelligence?.analysis ?? null)
  return <div className="page-grid"><div className="page-main"><div className="detail-highlight"><div className="detail-number">{memory.usagePercent == null ? '—' : Math.round(memory.usagePercent)}<small>%</small></div><div><span className="eyebrow">PHYSICAL MEMORY USED</span><h3>{fmtBytes(memory.usedBytes)} of {fmtBytes(memory.totalBytes)}</h3><p>{fmtBytes(memory.availableBytes)} available · {fmtBytes(memory.freeBytes)} free</p></div><span className={`health-pill ${statusFor(memory.usagePercent)}`}>{statusFor(memory.usagePercent)}</span></div><ChartPanel history={history} title="Physical memory history" memory /><Panel title="Physical memory breakdown" icon={MemoryStick}><div className="resource-list"><ResourceBar label="In use" value={fmtBytes(memory.usedBytes)} total={fmtBytes(memory.totalBytes)} percent={memory.usagePercent} color="violet" /><ResourceBar label="Available" value={fmtBytes(memory.availableBytes)} total={fmtBytes(memory.totalBytes)} percent={memory.availableBytes * 100 / Math.max(1,memory.totalBytes)} color="mint" /></div><div className="memory-foot"><span>Cached <strong>{fmtBytes(memory.cachedBytes)}</strong></span><span>Free <strong>{fmtBytes(memory.freeBytes)}</strong></span></div></Panel></div><div className="page-side"><Panel title="Commit & paging" icon={Database}><Detail label="Committed memory" value={fmtBytes(memory.committedBytes)} /><Detail label="Commit limit" value={fmtBytes(memory.commitLimitBytes)} /><Detail label="Pagefile in use" value={fmtBytes(memory.pagefileUsedBytes)} /><Detail label="Pagefile capacity" value={fmtBytes(memory.pagefileTotalBytes)} /></Panel><Panel title="Cache, standby & compression" icon={MemoryStick}><Detail label="System cache" value={fmtBytes(analysis?.cachedBytes ?? null)} /><Detail label="Standby (reclaimable)" value={fmtBytes(analysis?.standbyBytes ?? null)} /><Detail label="Modified pages" value={fmtBytes(analysis?.modifiedBytes ?? null)} /><Detail label="Compressed" value={fmtBytes(analysis?.compressedBytes ?? null)} /><Detail label="Free & zero pages" value={fmtBytes(analysis?.freeBytes ?? null)} /></Panel><SensorNotice text="Committed memory is an OS virtual-memory counter and is deliberately shown separately from physical RAM usage. Cache and standby pages are reclaimable and never counted as used memory; counters the OS does not expose display as Unavailable." /></div></div>
}

function GpuPage({ gpus, history }: { gpus: GpuMetrics[]; history: HistoryPoint[] }) {
  return <div className="page-main wide-page"><ChartPanel history={history} title="GPU & memory utilization" />{gpus.length ? <div className="gpu-grid">{gpus.map((gpu,index) => <Panel title={gpu.name} icon={MonitorCog} key={`${gpu.name}-${index}`}><div className="gpu-provider"><span className="provider-badge">{gpu.vendor}</span><span>{gpu.provider}</span></div><div className="gpu-metrics"><GpuMetric label="GPU utilization" value={pct(gpu.utilizationPercent)} /><GpuMetric label="GPU class" value={gpu.gpuClass ?? 'Unknown'} /><GpuMetric label="Driver version" value={gpu.driverVersion ?? 'Unavailable'} /><GpuMetric label="Dedicated memory used" value={fmtBytes(gpu.dedicatedUsedBytes)} /><GpuMetric label="Dedicated memory total" value={fmtBytes(gpu.dedicatedTotalBytes)} /><GpuMetric label="Dedicated memory available" value={fmtBytes(gpu.dedicatedAvailableBytes)} /><GpuMetric label="VRAM usage" value={pct(gpu.vramUsagePercent)} /><GpuMetric label="Shared GPU memory" value={fmtBytes(gpu.sharedUsedBytes)} /><GpuMetric label="Memory controller activity" value={pct(gpu.memoryUtilizationPercent)} /><GpuMetric label="Temperature" value={gpu.temperatureC == null ? 'Unavailable' : `${gpu.temperatureC.toFixed(0)} °C`} /><GpuMetric label="Core clock" value={gpu.coreClockMhz == null ? 'Unavailable' : `${gpu.coreClockMhz} MHz`} /><GpuMetric label="Memory clock" value={gpu.memoryClockMhz == null ? 'Unavailable' : `${gpu.memoryClockMhz} MHz`} /><GpuMetric label="Power draw" value={gpu.powerWatts == null ? 'Unavailable' : `${gpu.powerWatts.toFixed(1)} W`} /><GpuMetric label="Fan speed" value={gpu.fanPercent == null ? 'Unavailable' : `${gpu.fanPercent}%`} /></div><div className="info-callout"><ShieldCheck size={15} /><span>Dedicated VRAM and shared GPU memory are separate pools. Neither is presented as system RAM.</span></div></Panel>)}</div> : <EmptyState title="No GPU identified" detail="Windows did not report a display adapter." />}<SensorNotice text="NVIDIA metrics are collected through NVML when available. Other GPU vendors currently provide device identification only; unsupported sensors display as unavailable." /></div>
}

function StoragePage({ storage }: { storage: StorageVolume[] }) {
  return <div className="page-main wide-page"><Panel title="Mounted storage volumes" icon={HardDrive}><p className="section-description">Capacity usage is filesystem space. Live read/write throughput and drive type come from Windows storage performance and device queries where available; temperature and SMART health are not supplied by this volume provider.</p>{storage.length ? <div className="storage-table">{storage.map((disk) => <div className="storage-row" key={disk.name}><div className="storage-drive"><span className="drive-icon"><HardDrive size={17} /></span><div><strong>{disk.name}</strong><small>{disk.fileSystem || 'filesystem unavailable'} · {disk.kind}</small></div></div><div className="storage-capacity"><div className="volume-top"><span>{fmtBytes(disk.usedBytes)} used</span><b>{pct(disk.usagePercent)}</b></div><div className="bar-track"><span className="bar-fill cyan" style={{ width: `${disk.usagePercent ?? 0}%` }} /></div><small>{fmtBytes(disk.availableBytes)} available of {fmtBytes(disk.totalBytes)}</small></div><div className="storage-sensor"><small>READ / WRITE</small><b>{fmtRate(disk.readBytesPerSecond)} · {fmtRate(disk.writeBytesPerSecond)}</b></div></div>)}</div> : <EmptyState title="No mounted volumes" detail="No volumes were reported by Windows." />}</Panel></div>
}

function ProcessesPage({ processes }: { processes: ProcessMetrics[] }) {
  const { search, setSearch, sortBy, setSortBy } = useMonitoringStore()
  const filtered = useMemo(() => filterProcesses(processes, search, sortBy), [processes, search, sortBy])
  return <div className="page-main wide-page"><Panel title={`Running processes · ${processes.length}`} icon={SquareActivity} action={<label className="search-box"><Search size={15} /><input value={search} onChange={(event) => setSearch(event.target.value)} placeholder="Search process or PID" /></label>}><div className="process-controls"><span>Live process snapshot from Windows</span><label>Sort by <select value={sortBy} onChange={(event) => setSortBy(event.target.value as 'cpu'|'memory')}><option value="cpu">CPU</option><option value="memory">Memory</option></select></label></div><div className="process-table-wrap"><table className="process-table"><thead><tr><th>PROCESS</th><th>PID</th><th>CPU</th><th>MEMORY</th><th>MEMORY %</th><th>GPU</th><th>GPU MEMORY</th></tr></thead><tbody>{filtered.map((process) => <tr key={process.pid}><td><span className="process-name"><span className="process-dot" />{process.name}</span></td><td>{process.pid}</td><td>{process.cpuPercent == null ? 'Unavailable' : `${process.cpuPercent.toFixed(1)}%`}</td><td>{fmtBytes(process.memoryBytes)}</td><td>{process.memoryPercent == null ? '—' : `${process.memoryPercent.toFixed(1)}%`}</td><td>{pct(process.gpuPercent)}</td><td>{fmtBytes(process.gpuMemoryBytes)}</td></tr>)}</tbody></table>{filtered.length === 0 && <EmptyState title="No matching processes" detail="Try a different process name or PID." />}</div><p className="muted-note">Per-process GPU utilization is not provided by the current Windows collector. FluxCore never terminates processes.</p></Panel></div>
}

function HardwarePage({ cpu, gpus, storage, osName, osVersion, hostName, memory }: { cpu: CpuMetrics; gpus: GpuMetrics[]; storage: StorageVolume[]; osName: string; osVersion: string; hostName: string; memory: MemoryMetrics }) {
  const [checks, setChecks] = useState<HardwareCheckItem[] | null>(null)
  const [checkError, setCheckError] = useState<string | null>(null)
  const [checking, setChecking] = useState(false)
  const runCheck = async () => {
    setChecking(true); setCheckError(null)
    try { const report = await checkHardware(); setChecks(report.checks) } catch (error) { setCheckError(String(error)) } finally { setChecking(false) }
  }
  return <div className="page-main wide-page"><Panel title="Hardware & provider diagnostics" icon={ShieldCheck} action={<button className="diagnostic-button" onClick={() => void runCheck()} disabled={checking}>{checking ? 'Checking…' : 'Run hardware check'}</button>}>{checks ? <div className="diagnostic-list">{checks.map((item) => <div className="diagnostic-row" key={`${item.component}-${item.provider}`}><span className={`diagnostic-status ${item.status}`} /><div><strong>{item.component}</strong><span>{item.provider}</span><small>{item.detail}</small></div><b className={`diagnostic-label ${item.status}`}>{item.status}</b></div>)}</div> : <EmptyState title="Check hardware monitoring" detail={checkError ?? 'Run a check against the latest live OS snapshot to confirm which hardware providers are active and which sensors are unavailable.'} />}</Panel><div className="hardware-grid"><Panel title="Operating system" icon={LayoutDashboard}><Detail label="OS" value={osName} /><Detail label="Kernel version" value={osVersion} /><Detail label="Device" value={hostName} /></Panel><Panel title="Processor" icon={Cpu}><Detail label="Model" value={cpu.name} /><Detail label="Vendor" value={cpu.vendor} /><Detail label="Cores / threads" value={`${cpu.physicalCores ?? '—'} / ${cpu.logicalProcessors}`} /><Detail label="Installed physical RAM" value={fmtBytes(memory.totalBytes)} /></Panel><Panel title="Graphics adapters" icon={MonitorCog}>{gpus.length ? gpus.map((gpu,index)=><div key={`${gpu.name}${index}`} className="hardware-gpu"><Detail label="Adapter" value={gpu.name} /><Detail label="Vendor" value={gpu.vendor} /><Detail label="Dedicated VRAM" value={fmtBytes(gpu.dedicatedTotalBytes)} /></div>) : <EmptyState title="Adapter unavailable" detail="No display adapter identified." />}</Panel></div><Panel title="Storage devices" icon={HardDrive}>{storage.map((disk)=><Detail key={disk.name} label={`${disk.name} · ${disk.fileSystem}`} value={`${fmtBytes(disk.totalBytes)} total · ${disk.kind}`} />)}</Panel></div>
}

function SettingsPage() {
  const { settings, updateSettings } = useMonitoringStore()
  const patch = (values: Partial<typeof settings>) => void updateSettings({ ...settings, ...values })
  return <div className="page-main settings-page"><Panel title="Monitoring preferences" icon={SlidersHorizontal}><SettingRow title="Enable monitoring" description="Pause collection while keeping the application open."><Toggle checked={settings.monitoringEnabled} onChange={(checked) => patch({ monitoringEnabled: checked })} /></SettingRow><SettingRow title="Refresh interval" description="Controls dashboard polling frequency (250 ms to 60 sec)."><select className="setting-select" value={settings.refreshIntervalMs} onChange={(event) => patch({ refreshIntervalMs: Number(event.target.value) })}><option value={500}>0.5 seconds</option><option value={1000}>1 second</option><option value={2000}>2 seconds</option><option value={5000}>5 seconds</option><option value={10000}>10 seconds</option></select></SettingRow><SettingRow title="Chart history" description="Visible in-memory chart window."><select className="setting-select" value={settings.historyDurationMinutes} onChange={(event) => patch({ historyDurationMinutes: Number(event.target.value) })}><option value={5}>5 minutes</option><option value={10}>10 minutes</option><option value={30}>30 minutes</option><option value={60}>1 hour</option><option value={360}>6 hours</option></select></SettingRow><SettingRow title="Persist history" description="Save a sample every 5 seconds in the local SQLite database (7-day retention)."><Toggle checked={settings.persistHistory} onChange={(checked) => patch({ persistHistory: checked })} /></SettingRow><SettingRow title="Start with Windows" description="Preference only. Startup registration is not enabled in this phase."><Toggle checked={settings.startOnLogin} onChange={(checked) => patch({ startOnLogin: checked })} /></SettingRow><SettingRow title="Minimize to tray" description="Tray support is not configured; this preference is stored for a future phase."><Toggle checked={settings.minimizeToTray} onChange={(checked) => patch({ minimizeToTray: checked })} /></SettingRow><SettingRow title="Theme" description="Choose the application color scheme."><select className="setting-select" value={settings.theme} onChange={(event) => patch({ theme: event.target.value as 'dark'|'light'|'system' })}><option value="dark">Dark</option><option value="light">Light</option><option value="system">System</option></select></SettingRow></Panel><SensorNotice text="Settings persist locally and are validated by the backend; rejected values roll back automatically. Startup integration and tray behavior are preferences only and are not yet applied by the Windows shell." /></div>
}

function App() {
  const { snapshot, history, settings, status, error, page, setPage, initialize, refresh } = useMonitoringStore()
  const memoryRefresh = useMemoryStore((state) => state.refresh)
  const reportActive = useMemoryStore((state) => state.reportActive)
  const [navOpen, setNavOpen] = useState(true)
  const [clock, setClock] = useState(new Date())
  useEffect(() => { void initialize() }, [initialize])
  useEffect(() => {
    const visible = page === 'Resource Intelligence' && !document.hidden
    if (status === 'connected') void useResourceStore.getState().refresh()
    if (!visible) return
    const timer = window.setInterval(() => {
      if (!document.hidden) void useResourceStore.getState().refresh()
    }, 15000)
    return () => window.clearInterval(timer)
  }, [page, status])
  useEffect(() => {
    if (status !== 'connected') return
    const timer = window.setInterval(() => void memoryRefresh(), Math.max(2000, settings.refreshIntervalMs))
    void memoryRefresh()
    return () => window.clearInterval(timer)
  }, [status, settings.refreshIntervalMs, memoryRefresh])
  const intelligenceVisible = page === 'Memory Intelligence' || page === 'Optimization Center'
  useEffect(() => {
    const active = intelligenceVisible && !document.hidden
    void reportActive(active)
    if (!active) return
    const heartbeat = window.setInterval(() => void reportActive(true), 30000)
    const onVisibility = () => void reportActive(intelligenceVisible && !document.hidden)
    document.addEventListener('visibilitychange', onVisibility)
    return () => { window.clearInterval(heartbeat); document.removeEventListener('visibilitychange', onVisibility) }
  }, [intelligenceVisible, reportActive])
  useEffect(() => { const timer = window.setInterval(() => setClock(new Date()),1000); return () => window.clearInterval(timer) }, [])
  useEffect(() => {
    const resolve = () => {
      const resolved = settings.theme === 'system'
        ? (window.matchMedia?.('(prefers-color-scheme: light)').matches ? 'light' : 'dark')
        : settings.theme
      document.documentElement.dataset.theme = resolved
    }
    resolve()
    const media = window.matchMedia?.('(prefers-color-scheme: light)')
    media?.addEventListener('change', resolve)
    return () => media?.removeEventListener('change', resolve)
  }, [settings.theme])
  const pageContent = useMemo(() => {
    if (page === 'Resource Intelligence' && status === 'connected') return <ResourceIntelligencePage />
    if (!snapshot) return <div className="content-empty"><EmptyState title={status === 'web-preview' ? 'Open FluxCore for Windows' : status === 'error' ? 'Monitoring engine unavailable' : 'Connecting to Windows monitoring'} detail={error ?? 'Waiting for the first hardware sample.'} /></div>
    switch (page) {
      case 'CPU': return <CpuPage cpu={snapshot.cpu} history={history} />
      case 'Memory': return <MemoryPage memory={snapshot.memory} history={history} />
      case 'GPU': return <GpuPage gpus={snapshot.gpus} history={history} />
      case 'Storage': return <StoragePage storage={snapshot.storage} />
      case 'Memory Intelligence': return <MemoryIntelligencePage />
      case 'Optimization Center': return <OptimizationCenterPage />
      case 'GPU Acceleration': return <GpuAccelerationPage />
      case 'Processes': return <ProcessesPage processes={snapshot.processes} />
      case 'Hardware': return <HardwarePage cpu={snapshot.cpu} gpus={snapshot.gpus} storage={snapshot.storage} osName={snapshot.hardware.osName} osVersion={snapshot.hardware.osVersion} hostName={snapshot.hardware.hostName} memory={snapshot.memory} />
      case 'Settings': return <SettingsPage />
      default: return <Overview onPage={setPage} />
    }
  }, [snapshot, page, history, status, error, setPage])
  const grouped = ['WORKSPACE', 'MONITORS', 'MEMORY INTELLIGENCE', 'GPU INTELLIGENCE', 'SYSTEM']
  return <div className="app-shell">
    <aside className={`sidebar ${navOpen ? '' : 'collapsed'}`}><div className="brand"><div className="brand-mark"><Activity size={18} strokeWidth={2.5} /></div>{navOpen && <div className="brand-name"><strong>fluxcore</strong><span>RESOURCE INTELLIGENCE</span></div>}<button className="collapse-button" onClick={() => setNavOpen(!navOpen)} aria-label="Toggle sidebar"><ChevronDown size={15} /></button></div>
      <div className="system-pill"><span className={`status-light ${status}`} />{navOpen && <><span>{status === 'connected' ? snapshot?.monitoring ? 'Monitoring active' : 'Monitoring paused' : status === 'web-preview' ? 'Desktop app required' : status === 'error' ? 'Connection issue' : 'Starting engine'}</span><span className="system-live">{status === 'connected' ? 'LIVE' : '—'}</span></>}</div>
      <nav className="nav-list">{grouped.map((group) => <div className="nav-group" key={group}>{navOpen && <span className="nav-label">{group}</span>}{NAV.filter((item) => item.group === group).map(({label,icon:Icon}) => <button key={label} className={`nav-item ${page===label?'active':''}`} onClick={() => setPage(label)} title={!navOpen ? label : undefined}><Icon size={17} /><span>{navOpen && label}</span>{label==='Processes' && navOpen && snapshot && <small>{snapshot.processes.length}</small>}</button>)}</div>)}</nav>
      <div className="sidebar-bottom">{navOpen && <div className="sidebar-tip"><div className="tip-icon"><Zap size={15} /></div><span>Resource intelligence<br /><small>Built for clarity. Not guesswork.</small></span></div>}<div className="sidebar-version">{navOpen ? 'FLUXCORE 0.1.0 · WINDOWS' : '0.1'}</div></div>
    </aside>
    <main className="main-shell"><header className="topbar"><div className="breadcrumb"><span>FLUXCORE</span><ChevronRight size={13} /><strong>{page.toUpperCase()}</strong></div><div className="topbar-right"><span className="clock-label">{clock.toLocaleTimeString([], { hour: '2-digit', minute: '2-digit', second: '2-digit' })}</span><span className="topbar-separator" /><span className="collector-state"><i className={`status-light ${status}`} />{status === 'connected' ? 'COLLECTOR ONLINE' : status === 'web-preview' ? 'PREVIEW MODE' : status.toUpperCase()}</span><button className="icon-button" aria-label="Refresh metrics" onClick={() => void refresh()}><Activity size={16} /></button></div></header>
      <div className="content"><div className="page-heading"><div><div className="eyebrow">RESOURCE INTELLIGENCE <span>·</span> {snapshot?.hardware.hostName ?? 'LOCAL SYSTEM'}</div><h1>{titleCase(page)}</h1><p>{page === 'Overview' ? 'A clear view of your system, in real time.' : page === 'Settings' ? 'Tune collection and application preferences.' : 'Live system measurements, collected directly from Windows.'}</p></div>{page !== 'Settings' && <div className="last-updated"><span className="live-dot" />{snapshot ? `UPDATED ${new Date(snapshot.collectedAt).toLocaleTimeString()}` : 'WAITING FOR DATA'}</div>}</div>
        {error && status === 'connected' && <div className="inline-warning"><Bell size={15} />{error}</div>}{pageContent}
      </div><footer className="app-footer"><span><i className={`status-light ${status}`} />{status === 'connected' ? 'Local collection · No cloud upload' : 'Local-only hardware monitoring'}</span><span>Unavailable sensors are never estimated</span></footer>
    </main>
  </div>
}

export default App
