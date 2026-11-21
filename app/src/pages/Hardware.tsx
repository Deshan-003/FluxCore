import { useState } from 'react'
import { Cpu, HardDrive, Microchip, MonitorCog, Play, ShieldCheck } from 'lucide-react'
import { useMonitoringStore } from '../stores/monitoring'
import { checkHardware } from '../services/monitoring'
import {
  Button, Card, DetailList, EmptyState, PageHeader, StatusChip,
} from '../ui/kit'
import { fmtBytes } from '../lib/format'
import { toast } from '../ui/toast-store'
import type { HardwareCheckItem } from '../types/monitoring'

export default function HardwarePage() {
  const snapshot = useMonitoringStore((state) => state.snapshot)
  const [checks, setChecks] = useState<HardwareCheckItem[] | null>(null)
  const [checkError, setCheckError] = useState<string | null>(null)
  const [checking, setChecking] = useState(false)
  if (!snapshot) return null

  const runCheck = async () => {
    setChecking(true)
    setCheckError(null)
    try {
      const report = await checkHardware()
      setChecks(report.checks)
      const ready = report.checks.filter((item) => item.status === 'available').length
      toast(`Hardware check completed — ${ready} of ${report.checks.length} providers fully available`, 'success')
    } catch (error) {
      setCheckError(String(error))
      toast('Hardware check failed — see the diagnostics panel for details', 'error')
    } finally {
      setChecking(false)
    }
  }

  const statusChip = (status: HardwareCheckItem['status']) =>
    status === 'available' ? <StatusChip status="healthy" label="Ready" />
      : status === 'partial' ? <StatusChip status="attention" label="Limited" />
      : <StatusChip status="critical" label="Unavailable" />

  return <>
    <PageHeader title="Hardware info"
      description="What hardware FluxCore detected, and which data providers are active. Useful for troubleshooting missing sensors."
      actions={<Button variant="primary" icon={Play} loading={checking} onClick={() => void runCheck()}>
        {checking ? 'Checking…' : 'Run hardware check'}
      </Button>} />

    <Card title="Your PC at a glance" icon={Microchip}>
      <div className="rx-detail-grid">
        {[
          { label: 'Operating system', value: snapshot.hardware.osName },
          { label: 'Processor', value: snapshot.cpu.name },
          { label: 'Installed RAM', value: fmtBytes(snapshot.memory.totalBytes) },
          { label: 'Graphics card', value: snapshot.gpus[0]?.name ?? 'Not detected' },
          { label: 'Device name', value: snapshot.hardware.hostName },
          { label: 'OS version', value: snapshot.hardware.osVersion },
        ].map((row) => <div key={row.label} style={{ background: 'var(--rx-surface-2)', borderRadius: 8, padding: '10px 12px' }}>
          <div style={{ fontSize: 12, color: 'var(--rx-text-muted)', marginBottom: 3 }}>{row.label}</div>
          <div style={{ fontSize: 13, fontWeight: 600 }}>{row.value}</div>
        </div>)}
      </div>
    </Card>

    <Card title="Hardware & provider diagnostics" icon={ShieldCheck}
      sub="Confirms which hardware providers are active and which sensors are unavailable — without guessing.">
      {!checks
        ? <EmptyState icon={ShieldCheck} title="Diagnostics not run yet"
            detail={checkError ?? 'Run a check against the latest live OS snapshot to see which providers FluxCore is using and where data is unavailable.'} />
        : <div className="rx-table-wrap"><table className="rx-table">
            <thead><tr><th>Component</th><th>Provider</th><th>Status</th></tr></thead>
            <tbody>
              {checks.map((row) => <tr key={`${row.component}-${row.provider}`}>
                <td><span className="rx-process-name">
                  <span className="rx-process-dot" style={{ background: row.status === 'available' ? 'var(--rx-success)' : row.status === 'partial' ? 'var(--rx-warning)' : 'var(--rx-danger)' }} />
                  <span><strong>{row.component}</strong><br /><span style={{ fontSize: 12, color: 'var(--rx-text-muted)' }}>{row.detail}</span></span></span></td>
                <td>{row.provider}</td>
                <td>{statusChip(row.status)}</td>
              </tr>)}
            </tbody>
          </table></div>}
    </Card>

    <div className="rx-grid-3">
      <Card title="Processor" icon={Cpu}>
        <DetailList rows={[
          { label: 'Model', value: snapshot.cpu.name },
          { label: 'Manufacturer', value: snapshot.cpu.vendor },
          { label: 'Cores / threads', value: `${snapshot.cpu.physicalCores ?? '—'} / ${snapshot.cpu.logicalProcessors}` },
          { label: 'Architecture', value: snapshot.cpu.architecture ?? 'Unavailable' },
        ]} />
      </Card>
      <Card title="Graphics adapters" icon={MonitorCog}>
        {snapshot.gpus.length
          ? snapshot.gpus.map((gpu, index) => <DetailList key={`${gpu.name}-${index}`} rows={[
              { label: 'Adapter', value: gpu.name },
              { label: 'Vendor', value: gpu.vendor },
              { label: 'Dedicated VRAM', value: fmtBytes(gpu.dedicatedTotalBytes) },
            ]} />)
          : <EmptyState icon={MonitorCog} title="Adapter unavailable" detail="No display adapter identified by Windows." />}
      </Card>
      <Card title="Storage devices" icon={HardDrive}>
        {snapshot.storage.length
          ? <DetailList rows={snapshot.storage.map((disk) => ({ label: `${disk.name} · ${disk.fileSystem}`, value: `${fmtBytes(disk.totalBytes)} · ${disk.kind}` }))} />
          : <EmptyState icon={HardDrive} title="No volumes" detail="No storage volumes were reported." />}
      </Card>
    </div>
  </>
}
