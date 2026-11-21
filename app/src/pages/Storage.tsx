import { HardDrive } from 'lucide-react'
import { useMonitoringStore } from '../stores/monitoring'
import {
  Card, DetailList, Disclose, EmptyState, PageHeader, ResourceBar, StatusChip,
} from '../ui/kit'
import { fmtBytes, fmtRate, statusForPercent } from '../lib/format'

export default function StoragePage() {
  const snapshot = useMonitoringStore((state) => state.snapshot)
  if (!snapshot) return null
  const disks = snapshot.storage

  if (disks.length === 0) {
    return <>
      <PageHeader title="Storage" description="Drive space and live read/write activity for each connected volume." />
      <Card><EmptyState icon={HardDrive} title="No mounted volumes"
        detail="Windows did not report a mounted storage volume. FluxCore will show drives here as soon as they are available." /></Card>
    </>
  }

  return <>
    <PageHeader title="Storage" description="Drive space and live read/write activity for each connected volume." />

    <div className="rx-stack">
      {disks.map((disk) => {
        const status = statusForPercent(disk.usagePercent)
        return <div className="rx-hero" key={disk.name}>
          <span style={{ color: 'var(--rx-c-storage)' }}><HardDrive size={30} /></span>
          <div className="rx-hero-text" style={{ minWidth: 220 }}>
            <h2>{disk.name} <span className="rx-chip neutral" style={{ marginLeft: 6 }}>{disk.kind}</span></h2>
            <p>{disk.fileSystem || 'filesystem unavailable'} · {fmtBytes(disk.totalBytes)} capacity</p>
          </div>
          <div style={{ flex: 1, minWidth: 260 }}>
            <ResourceBar label="Used space" value={fmtBytes(disk.usedBytes)} total={fmtBytes(disk.totalBytes)} percent={disk.usagePercent} color="var(--rx-c-storage)" />
            <ResourceBar label="Free space" value={fmtBytes(disk.availableBytes)} total={fmtBytes(disk.totalBytes)}
              percent={disk.usagePercent == null ? null : 100 - disk.usagePercent} color="var(--rx-neutral)" />
          </div>
          <div className="rx-hero-side" style={{ flexDirection: 'column', alignItems: 'stretch', gap: 6 }}>
            <StatusChip status={status} />
            <span className="rx-data-age" style={{ fontSize: 12.5 }}>
              Read {fmtRate(disk.readBytesPerSecond)} · Write {fmtRate(disk.writeBytesPerSecond)}
            </span>
          </div>
        </div>
      })}
    </div>

    <div className="rx-grid-side">
      <Card title="About these measurements" icon={HardDrive}
        sub="Where each number comes from, so you can trust what you see.">
        <DetailList rows={[
          { label: 'Capacity & free space', value: 'Filesystem query', tip: 'Reported by Windows for each mounted volume.' },
          { label: 'Read / write rates', value: 'Performance counters', tip: 'Derived from Windows storage performance counters between collection ticks.' },
          { label: 'Drive type', value: 'Device query', tip: 'NVMe SSD, SATA SSD or HDD, as reported by the storage device.' },
          { label: 'Drive health (SMART)', value: 'Unavailable', tip: 'This volume provider does not supply SMART health. FluxCore shows Unavailable rather than guessing.' },
        ]} />
      </Card>

      <Card>
        <Disclose summary="Why is free space important?">
          <div className="rx-disclose-body">
            <p className="rx-card-sub" style={{ margin: 0 }}>
              Windows needs free space on the system drive for updates, virtual memory and temporary files. Staying above roughly 15% free
              keeps the system responsive; dropping below that is a common cause of slowdowns. FluxCore flags a drive as
              <strong> Needs attention</strong> above 60% and <strong>Critical</strong> above 85% full.
            </p>
          </div>
        </Disclose>
      </Card>
    </div>
  </>
}
