import { useState } from 'react'
import { SquareActivity } from 'lucide-react'
import { useMonitoringStore } from '../stores/monitoring'
import { Card, EmptyState, PageHeader, SearchBox, Switch } from '../ui/kit'
import { fmtBytes, pct } from '../lib/format'
import { filterProcesses } from '../lib/processes'

export default function ProcessesPage() {
  const snapshot = useMonitoringStore((state) => state.snapshot)
  const search = useMonitoringStore((state) => state.search)
  const setSearch = useMonitoringStore((state) => state.setSearch)
  const sortBy = useMonitoringStore((state) => state.sortBy)
  const setSortBy = useMonitoringStore((state) => state.setSortBy)
  const [advanced, setAdvanced] = useState(false)
  const processes = snapshot?.processes ?? []
  const filtered = filterProcesses(processes, search, sortBy)
  if (!snapshot) return null

  if (processes.length === 0) {
    return <>
      <PageHeader title="Processes" description="Which apps and services are using your resources right now." />
      <Card><EmptyState icon={SquareActivity} title="No process data yet"
        detail="FluxCore will list running apps and their CPU/RAM use as soon as the collector responds." /></Card>
    </>
  }

  return <>
    <PageHeader title="Processes" description="Which apps and services are using your resources right now. FluxCore never terminates processes — this is read-only." />

    <Card
      title={`Running processes · ${filtered.length}${search ? ` of ${processes.length}` : ''}`}
      icon={SquareActivity}
      right={<SearchBox value={search} onChange={setSearch} placeholder="Search by name or PID" />}
    >
      <div className="rx-setting-row" style={{ borderTop: 0, paddingTop: 0 }}>
        <div className="rx-setting-text">
          <h3>Show technical columns</h3>
          <p>Adds PID and per-process GPU columns for advanced diagnostics.</p>
        </div>
        <div className="rx-setting-control"><Switch checked={advanced} onChange={setAdvanced} label="Show technical columns" /></div>
      </div>

      <div className="rx-setting-row">
        <div className="rx-setting-text">
          <h3>Sort by</h3>
          <p>Choose what to rank the list by.</p>
        </div>
        <div className="rx-setting-control">
          <select className="rx-select" value={sortBy} onChange={(event) => setSortBy(event.target.value as 'cpu' | 'memory')} aria-label="Sort processes by">
            <option value="cpu">CPU usage</option>
            <option value="memory">Memory usage</option>
          </select>
        </div>
      </div>

      {filtered.length === 0
        ? <EmptyState icon={SquareActivity} title="No matching processes" detail={`Nothing matches "${search}". Try a different name or PID.`} />
        : <div className="rx-table-wrap"><table className="rx-table">
            <thead><tr>
              <th>Process</th>
              <th className="num">CPU</th>
              <th className="num">Memory</th>
              <th className="num">Share of RAM</th>
              {advanced && <th className="num">PID</th>}
              {advanced && <th className="num">GPU</th>}
              {advanced && <th className="num">GPU memory</th>}
            </tr></thead>
            <tbody>
              {filtered.map((process) => <tr key={process.pid}>
                <td><span className="rx-process-name"><span className="rx-process-dot" style={{ background: 'var(--rx-c-cpu)' }} />{process.name}</span></td>
                <td className="num">{process.cpuPercent == null ? 'Unavailable' : `${process.cpuPercent.toFixed(1)}%`}</td>
                <td className="num">{fmtBytes(process.memoryBytes)}</td>
                <td className="num">{process.memoryPercent == null ? '—' : `${process.memoryPercent.toFixed(1)}%`}</td>
                {advanced && <td className="num">{process.pid}</td>}
                {advanced && <td className="num">{pct(process.gpuPercent)}</td>}
                {advanced && <td className="num">{fmtBytes(process.gpuMemoryBytes)}</td>}
              </tr>)}
            </tbody>
          </table></div>}

      {!advanced && <p className="rx-chart-caption">Per-process GPU usage is not provided by the current Windows collector — enable technical columns to see the values that are available. FluxCore never terminates processes.</p>}
    </Card>
  </>
}
