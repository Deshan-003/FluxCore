import { Activity, Bell, type LucideIcon } from 'lucide-react'

// Small presentational building blocks shared across the dashboard pages.
// They are kept in their own module so UI states (empty, unavailable,
// unsupported) can be rendered and asserted in isolation.

export function EmptyState({ title, detail }: { title: string; detail: string }) {
  return <div className="empty-state"><div className="empty-icon"><Activity size={20} /></div><strong>{title}</strong><span>{detail}</span></div>
}

export function Panel({ title, icon: Icon, action, children, className = '' }: { title: string; icon: LucideIcon; action?: React.ReactNode; children: React.ReactNode; className?: string }) {
  return <section className={`panel ${className}`}><div className="panel-head"><div className="panel-title"><Icon size={16} /><h2>{title}</h2></div>{action}</div>{children}</section>
}

export function ResourceBar({ label, value, total, percent, color = 'cyan' }: { label: string; value: string; total: string; percent: number | null; color?: string }) {
  return <div className="resource-row"><div className="resource-label"><span>{label}</span><span>{value}<small> / {total}</small></span></div><div className="bar-track"><span className={`bar-fill ${color}`} style={{ width: `${Math.max(0, Math.min(percent ?? 0,100))}%` }} /></div></div>
}

export function Detail({ label, value }: { label: string; value: string }) {
  return <div className="detail-row"><span>{label}</span><strong title={value}>{value}</strong></div>
}

export function GpuMetric({ label, value }: { label: string; value: string }) {
  return <div className="gpu-metric"><span>{label}</span><strong>{value}</strong></div>
}

export function SensorNotice({ text }: { text: string }) {
  return <div className="sensor-notice"><Bell size={15} /><span>{text}</span></div>
}

export function SettingRow({ title, description, children }: { title: string; description: string; children: React.ReactNode }) {
  return <div className="setting-row"><div><strong>{title}</strong><span>{description}</span></div>{children}</div>
}

export function Toggle({ checked, onChange }: { checked: boolean; onChange: (checked: boolean) => void }) {
  return <button className={`toggle ${checked ? 'on' : ''}`} role="switch" aria-checked={checked} onClick={() => onChange(!checked)}><span /></button>
}
