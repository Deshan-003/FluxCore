import { Area, AreaChart, CartesianGrid, ResponsiveContainer, Tooltip, XAxis, YAxis } from 'recharts'
import { Database, MemoryStick } from 'lucide-react'
import { useMonitoringStore } from '../stores/monitoring'
import { useMemoryStore } from '../stores/memory'
import {
  Card, ChartFrame, DetailList, Disclose, EmptyState, InfoTip, PageHeader, RXTooltip,
  ResourceBar, StatusChip,
} from '../ui/kit'
import { fmtBytes, fmtTime, statusForPercent } from '../lib/format'
import { rxAxis, rxGrid, rxTick } from '../ui/chart-config'

export default function MemoryPage() {
  const snapshot = useMonitoringStore((state) => state.snapshot)
  const history = useMonitoringStore((state) => state.history)
  const analysis = useMemoryStore((state) => state.intelligence?.analysis ?? null)
  if (!snapshot) return null
  const memory = snapshot.memory
  const status = statusForPercent(memory.usagePercent)
  const rows = history.map((point) => ({ label: fmtTime(point.timestamp), used: point.memoryPercent }))
  const commitPercent = memory.committedBytes != null && memory.commitLimitBytes
    ? (memory.committedBytes / memory.commitLimitBytes) * 100 : null

  return <>
    <PageHeader title="Memory (RAM)" description="What your computer's working memory is doing right now — in plain language." />

    <div className="rx-hero">
      <div className="rx-hero-value">{fmtBytes(memory.usedBytes)}</div>
      <div className="rx-hero-text">
        <h2>In use, of {fmtBytes(memory.totalBytes)} installed</h2>
        <p>{fmtBytes(memory.availableBytes)} available for new apps{analysis?.memoryLoadPercent != null ? ` · memory load ${analysis.memoryLoadPercent.toFixed(0)}%` : ''}<InfoTip text="Memory pressure describes how hard Windows is working to keep apps in RAM. The Memory Management page shows the full pressure analysis with its reasons." /></p>
      </div>
      <div className="rx-hero-side"><StatusChip status={status} /></div>
    </div>

    <div className="rx-grid-side">
      <div className="rx-stack">
        <Card title="Physical memory history" icon={MemoryStick} note="Measured, never estimated">
          {rows.length < 2
            ? <EmptyState title="Collecting history" detail="The chart appears as soon as real samples are collected." />
            : <ChartFrame ariaLabel="Physical memory usage history" caption="The violet line is the share of RAM in use. 'Available' is what apps can claim immediately — cached files inside it are reclaimable within milliseconds.">
                <ResponsiveContainer width="100%" height="100%">
                  <AreaChart data={rows} margin={{ top: 8, right: 8, bottom: 0, left: -18 }}>
                    <defs><linearGradient id="rxMemUsed" x1="0" y1="0" x2="0" y2="1">
                      <stop offset="0%" stopColor="var(--rx-c-memory)" stopOpacity={0.28} />
                      <stop offset="100%" stopColor="var(--rx-c-memory)" stopOpacity={0} />
                    </linearGradient></defs>
                    <CartesianGrid {...rxGrid} />
                    <XAxis dataKey="label" {...rxAxis} tick={rxTick} minTickGap={44} />
                    <YAxis domain={[0, 100]} {...rxAxis} tick={rxTick} tickFormatter={(v) => `${v}%`} />
                    <Tooltip content={<RXTooltip unit="%" />} />
                    <Area dataKey="used" name="In use" type="monotone" stroke="var(--rx-c-memory)" strokeWidth={2} fill="url(#rxMemUsed)" isAnimationActive={false} connectNulls />
                  </AreaChart>
                </ResponsiveContainer>
              </ChartFrame>}
        </Card>

        <Card title="Physical memory breakdown" icon={MemoryStick}
          sub="RAM is split into what apps actively use, cache Windows keeps for speed, and what is truly free.">
          <ResourceBar label="In use by apps" value={fmtBytes(memory.usedBytes)} total={fmtBytes(memory.totalBytes)} percent={memory.usagePercent} color="var(--rx-c-memory)" />
          <ResourceBar label="Available" value={fmtBytes(memory.availableBytes)} total={fmtBytes(memory.totalBytes)}
            percent={memory.totalBytes ? (memory.availableBytes / memory.totalBytes) * 100 : null} color="var(--rx-c-storage)"
            tip="Instantly claimable memory, including reclaimable cache. Windows returns cached pages to apps the moment they need them." />
          <ResourceBar label="Cached" value={fmtBytes(memory.cachedBytes)} total={fmtBytes(memory.totalBytes)}
            percent={memory.totalBytes && memory.cachedBytes != null ? (memory.cachedBytes / memory.totalBytes) * 100 : null} color="var(--rx-neutral)"
            tip="Files Windows keeps in RAM so reopening them is instant. This memory is NOT wasted." />
        </Card>
      </div>

      <div className="rx-stack">
        <Card title="Top memory consumers" icon={MemoryStick} note="Right now">
          {snapshot.processes.length === 0
            ? <EmptyState title="No process data" detail="Process enumeration is not available from the current collector." />
            : <div className="rx-table-wrap"><table className="rx-table">
                <thead><tr><th>Process</th><th className="num">RAM</th><th className="num">Share</th></tr></thead>
                <tbody>
                  {snapshot.processes.slice().sort((a, b) => b.memoryBytes - a.memoryBytes).slice(0, 6).map((process) => (
                    <tr key={process.pid}>
                      <td><span className="rx-process-name"><span className="rx-process-dot" style={{ background: 'var(--rx-c-memory)' }} />{process.name}</span></td>
                      <td className="num">{fmtBytes(process.memoryBytes)}</td>
                      <td className="num">{process.memoryPercent == null ? '—' : `${process.memoryPercent.toFixed(1)}%`}</td>
                    </tr>
                  ))}
                </tbody>
              </table></div>}
        </Card>

        <Card title="Committed memory & pagefile" icon={Database}
          sub="Virtual memory: what Windows has promised to apps, backed by RAM plus the pagefile on disk.">
          <ResourceBar label="Committed" value={fmtBytes(memory.committedBytes)} total={fmtBytes(memory.commitLimitBytes)} percent={commitPercent} color="var(--rx-c-other)"
            tip="The total memory Windows has promised to all running processes. It can exceed physical RAM because the pagefile on disk covers the difference." />
          <DetailList rows={[
            { label: 'Pagefile in use', value: fmtBytes(memory.pagefileUsedBytes), tip: 'Memory currently parked on disk. Some use is normal; sustained growth under light load signals RAM pressure.' },
            { label: 'Pagefile capacity', value: fmtBytes(memory.pagefileTotalBytes) },
            { label: 'Standby (reclaimable)', value: fmtBytes(analysis?.standbyBytes ?? null), tip: 'Cached data returned to apps the moment they request memory.' },
            { label: 'Modified pages', value: fmtBytes(analysis?.modifiedBytes ?? null), tip: 'Changed data waiting to be written to disk.' },
            { label: 'Compressed', value: fmtBytes(analysis?.compressedBytes ?? null), tip: 'RAM Windows compressed to fit more in. Saves space at a small CPU cost.' },
          ]} />
        </Card>
      </div>
    </div>

    <Card>
      <Disclose summary="Physical RAM vs committed memory — why they differ">
        <div className="rx-disclose-body">
          <p className="rx-card-sub" style={{ margin: 0 }}>
            <strong>Physical RAM</strong> is the memory chips inside your computer. <strong>Committed memory</strong> is every promise Windows has made to running
            apps — even memory that currently lives on the disk pagefile. Commit can be larger than RAM without a problem; only sustained growth of
            <em> pagefile use</em> together with high pressure indicates that RAM is too small for the workload. Counters the OS does not expose are
            shown as Unavailable, never estimated.
          </p>
        </div>
      </Disclose>
    </Card>
  </>
}
