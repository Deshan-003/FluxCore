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
import { useMemoryStore } from './stores/memory'
import MemoryIntelligencePage from './pages/MemoryIntelligence'
import OptimizationCenterPage from './pages/OptimizationCenter'
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
  { label: 'CPU', icon: Cpu, group: 'MONITORS' },
  { label: 'Memory', icon: MemoryStick, group: 'MONITORS' },
  { label: 'GPU', icon: MonitorCog, group: 'MONITORS' },
  { label: 'Storage', icon: HardDrive, group: 'MONITORS' },
  { label: 'Memory Intelligence', icon: BrainCircuit, group: 'MEMORY INTELLIGENCE' },
  { label: 'Optimization Center', icon: Sparkles, group: 'MEMORY INTELLIGENCE' },
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
  const cutoff = Date.now() - settings.historyDurationMinutes * 60_000
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

