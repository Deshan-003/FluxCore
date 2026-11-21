/*
 * FluxCore redesign preview — shared UI kit (Stage 1, preview-only).
 * Implements the components defined in UI_DESIGN_SYSTEM.md §5.
 * Production integration swaps this file's primitives for the approved
 * equivalents without changing page logic.
 *
 * Pure helpers live in format.ts, the toast store in toast-store.ts and chart
 * styling in chart-config.ts — this file exports components only (fast-refresh).
 */
import { useEffect, useState, useSyncExternalStore } from 'react'
import type { ComponentType, CSSProperties, FormEvent, ReactNode } from 'react'
import {
  AlertOctagon, AlertTriangle, CheckCircle2, ChevronRight, CircleCheck, HelpCircle, Info,
  Loader2, MinusCircle, Search, X,
} from 'lucide-react'
import type { RxStatus } from './format'
import { dismissToast, getToasts, subscribeToasts } from './toast-store'

/* ---------- status language (UI_DESIGN_SYSTEM.md §2.4) ---------- */
const STATUS_META: Record<RxStatus, { label: string; icon: ComponentType<{ size?: number | string }>; chip: string }> = {
  healthy: { label: 'Healthy', icon: CheckCircle2, chip: 'success' },
  normal: { label: 'Normal', icon: CircleCheck, chip: 'success' },
  attention: { label: 'Needs attention', icon: AlertTriangle, chip: 'warning' },
  critical: { label: 'Critical', icon: AlertOctagon, chip: 'critical' },
  unavailable: { label: 'Unavailable', icon: MinusCircle, chip: 'neutral' },
}

export function StatusChip({ status, label }: { status: RxStatus; label?: string }) {
  const meta = STATUS_META[status]
  const Icon = meta.icon
  return <span className={`rx-chip ${meta.chip}`}><Icon size={13} />{label ?? meta.label}</span>
}

/* ---------- layout primitives ---------- */
export function PageHeader({ title, description, actions }: { title: string; description: string; actions?: ReactNode }) {
  return <div className="rx-page-header">
    <div><h1>{title}</h1><p>{description}</p></div>
    {actions && <div className="rx-page-actions">{actions}</div>}
  </div>
}

export function Card({ title, icon: Icon, note, right, sub, children, className }: {
  title?: string; icon?: ComponentType<{ size?: number | string }>; note?: string; right?: ReactNode
  sub?: string; children: ReactNode; className?: string
}) {
  return <section className={`rx-card ${className ?? ''}`}>
    {title && <div className="rx-card-head">
      {Icon && <Icon size={15} />}
      <h2>{title}</h2>
      {note && <span className="rx-head-note">{note}</span>}
      {right && <div className="rx-card-head-right">{right}</div>}
    </div>}
    {sub && <p className="rx-card-sub">{sub}</p>}
    {children}
  </section>
}

export function Disclose({ summary, children, defaultOpen = false }: { summary: string; children: ReactNode; defaultOpen?: boolean }) {
  const [open, setOpen] = useState(defaultOpen)
  return <details className="rx-disclose" open={open} onToggle={(e: FormEvent<HTMLDetailsElement>) => setOpen((e.target as HTMLDetailsElement).open)}>
    <summary><ChevronRight size={14} />{summary}</summary>
    <div className="rx-disclose-body">{children}</div>
  </details>
}

export function DetailList({ rows }: { rows: { label: string; value: ReactNode; tip?: string }[] }) {
  return <dl className="rx-detail-grid">
    {rows.map((row) => <div className="rx-detail-row" key={row.label}>
      <dt>{row.label}{row.tip && <InfoTip text={row.tip} />}</dt>
      <dd>{row.value}</dd>
    </div>)}
  </dl>
}

/* ---------- buttons ---------- */
export function Button({ variant = 'secondary', small, icon: Icon, loading, children, onClick, disabled }: {
  variant?: 'primary' | 'secondary' | 'ghost' | 'danger'; small?: boolean
  icon?: ComponentType<{ size?: number | string }>; loading?: boolean
  children: ReactNode; onClick?: () => void; disabled?: boolean
}) {
  return <button className={`rx-btn ${variant} ${small ? 'small' : ''}`} onClick={onClick} disabled={disabled || loading}>
    {loading ? <Loader2 size={14} className="rx-spin" /> : Icon ? <Icon size={14} /> : null}
    {children}
  </button>
}

export function IconButton({ icon: Icon, label, onClick }: { icon: ComponentType<{ size?: number | string }>; label: string; onClick?: () => void }) {
  return <button className="rx-icon-btn" aria-label={label} title={label} onClick={onClick}><Icon size={15} /></button>
}

/* ---------- bars ---------- */
export function ResourceBar({ label, value, total, percent, color, segments, tip }: {
  label: string; value: string; total?: string; percent: number | null; color?: string
  segments?: { label: string; percent: number; color: string }[]; tip?: string
}) {
  return <div className="rx-bar-row">
    <div className="rx-bar-labels">
      <span>{label}{tip && <InfoTip text={tip} />}</span>
      <b>{value}{total ? ` of ${total}` : ''}</b>
    </div>
    <div className="rx-bar">
      {segments
        ? segments.map((seg) => <span key={seg.label} style={{ width: `${seg.percent}%`, background: seg.color }} />)
        : <span style={{ width: `${Math.min(100, percent ?? 0)}%`, background: color ?? 'var(--rx-accent)' }} />}
    </div>
  </div>
}

/* ---------- metric card ---------- */
export function MetricCard({ icon: Icon, name, caption, value, unit, percent, color, history, status, onOpen }: {
  icon: ComponentType<{ size?: number | string }>; name: string; caption: string
  value: string; unit?: string; percent: number | null; color: string
  history: { t: number | string; v: number | null }[]; status: RxStatus; onOpen: () => void
}) {
  return <button className="rx-metric" style={{ ['--rx-metric-color' as string]: color }} onClick={onOpen}>
    <div className="rx-metric-top"><Icon size={16} /><span className="rx-metric-name">{name}</span><StatusChip status={status} /></div>
    <div className="rx-metric-value-row">
      <span className="rx-metric-value">{value}</span>{unit && <span className="rx-metric-unit">{unit}</span>}
    </div>
    <div className="rx-metric-caption">{caption}</div>
    <div className="rx-metric-bar"><span style={{ width: `${Math.min(100, percent ?? 0)}%` }} /></div>
    <Sparkline data={history} color={color} />
  </button>
}

function Sparkline({ data, color }: { data: { t: number | string; v: number | null }[]; color: string }) {
  const pts = data.slice(-40)
  if (pts.length < 2) return <div className="rx-metric-spark" />
  const vs = pts.map((p) => p.v)
  const min = Math.min(...vs.filter((v): v is number => v != null), 0)
  const max = Math.max(...vs.filter((v): v is number => v != null), 1)
  const w = 100; const h = 38
  const path = pts.map((p, i) => {
    const x = (i / (pts.length - 1)) * w
    const y = p.v == null ? h : h - ((p.v - min) / (max - min || 1)) * (h - 4) - 2
    return `${i === 0 ? 'M' : 'L'}${x.toFixed(1)},${y.toFixed(1)}`
  }).join(' ')
  const area = `${path} L${w},${h} L0,${h} Z`
  const gid = `rx-spark-${color.replace(/[^a-z0-9]/gi, '')}`
  return <div className="rx-metric-spark">
    <svg viewBox={`0 0 ${w} ${h}`} preserveAspectRatio="none" width="100%" height="100%" role="img" aria-label="Recent trend sparkline">
      <defs><linearGradient id={gid} x1="0" y1="0" x2="0" y2="1">
        <stop offset="0%" stopColor={color} stopOpacity={0.3} /><stop offset="100%" stopColor={color} stopOpacity={0} />
      </linearGradient></defs>
      <path d={area} fill={`url(#${gid})`} stroke="none" />
      <path d={path} fill="none" stroke={color} strokeWidth={1.6} vectorEffect="non-scaling-stroke" />
    </svg>
  </div>
}

/* ---------- tabs ---------- */
export function Tabs({ tabs, active, onChange }: {
  tabs: { id: string; label: string; count?: number }[]; active: string; onChange: (id: string) => void
}) {
  return <div className="rx-tabs" role="tablist">
    {tabs.map((tab) => <button key={tab.id} role="tab" aria-selected={active === tab.id}
      className={`rx-tab ${active === tab.id ? 'active' : ''}`} onClick={() => onChange(tab.id)}>
      {tab.label}{tab.count != null && <span className="rx-tab-count">{tab.count}</span>}
    </button>)}
  </div>
}

/* ---------- switch & settings ---------- */
export function Switch({ checked, onChange, label }: { checked: boolean; onChange: (v: boolean) => void; label: string }) {
  return <button role="switch" aria-checked={checked} aria-label={label} className="rx-switch" onClick={() => onChange(!checked)} />
}

export function SettingRow({ title, description, control, planned, tip }: {
  title: string; description: string; control: ReactNode; planned?: boolean; tip?: string
}) {
  return <div className={`rx-setting-row ${planned ? 'planned' : ''}`}>
    <div className="rx-setting-text">
      <h3>{title}{tip && <InfoTip text={tip} />}{planned && <span className="rx-head-note"> · Planned for a later phase</span>}</h3>
      <p>{description}</p>
    </div>
    <div className="rx-setting-control">{control}</div>
  </div>
}

export function ToggleGroup({ options, value, onChange }: {
  options: { id: string; label: string }[]; value: string; onChange: (id: string) => void
}) {
  return <div className="rx-toggle-group" role="radiogroup">
    {options.map((o) => <button key={o.id} role="radio" aria-checked={value === o.id}
      className={value === o.id ? 'active' : ''} onClick={() => onChange(o.id)}>{o.label}</button>)}
  </div>
}

export function SearchBox({ value, onChange, placeholder }: { value: string; onChange: (v: string) => void; placeholder: string }) {
  return <label className="rx-search"><Search size={14} />
    <input value={value} onChange={(e) => onChange(e.target.value)} placeholder={placeholder} aria-label={placeholder} />
  </label>
}

/* ---------- InfoTip ---------- */
export function InfoTip({ text }: { text: string }) {
  const [open, setOpen] = useState(false)
  return <span className="rx-tip-wrap">
    <button className="rx-tip-btn" aria-label="What does this mean?" aria-expanded={open}
      onClick={() => setOpen(!open)} onBlur={() => setOpen(false)} onMouseLeave={() => setOpen(false)} onMouseEnter={() => setOpen(true)}>
      <HelpCircle size={13} />
    </button>
    {open && <span className="rx-tip-pop" role="tooltip">{text}</span>}
  </span>
}

/* ---------- toasts ---------- */
export function ToastHost() {
  const items = useSyncExternalStore(subscribeToasts, getToasts, getToasts)
  if (items.length === 0) return null
  return <div className="rx-toasts" aria-live="polite">
    {items.map((t) => <div key={t.id} className={`rx-toast ${t.kind}`}>
      {t.kind === 'success' ? <CheckCircle2 size={15} /> : t.kind === 'error' ? <AlertOctagon size={15} /> : <Info size={15} />}
      <span>{t.text}</span>
      <button className="rx-icon-btn" style={{ width: 24, height: 24 }} aria-label="Dismiss notification"
        onClick={() => dismissToast(t.id)}><X size={13} /></button>
    </div>)}
  </div>
}

/* ---------- confirm dialog (accessible, Escape to close) ---------- */
export interface ConfirmSpec {
  title: string; body: string
  facts?: { kind: 'what' | 'benefit' | 'risk'; text: string }[]
  confirmLabel: string; danger?: boolean
  onConfirm: () => void; onCancel: () => void
}
export function ConfirmDialog({ spec }: { spec: ConfirmSpec | null }) {
  useEffect(() => {
    if (!spec) return
    const onKey = (e: KeyboardEvent) => { if (e.key === 'Escape') spec.onCancel() }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [spec])
  if (!spec) return null
  const factIcon = { what: Info, benefit: CheckCircle2, risk: AlertTriangle }
  return <div className="rx-dialog-overlay" onClick={spec.onCancel}>
    <div className="rx-dialog" role="dialog" aria-modal="true" aria-label={spec.title} onClick={(e) => e.stopPropagation()}>
      <h2>{spec.title}</h2>
      <p>{spec.body}</p>
      {spec.facts && <div className="rx-dialog-facts">
        {spec.facts.map((f) => { const Icon = factIcon[f.kind]; return <div key={f.kind}><Icon size={14} /><span><strong>{f.kind === 'what' ? 'What happens:' : f.kind === 'benefit' ? 'Benefit:' : 'Risk:'}</strong> {f.text}</span></div> })}
      </div>}
      <div className="rx-dialog-actions">
        <button className="rx-btn secondary" onClick={spec.onCancel}>Cancel</button>
        <button className={`rx-btn ${spec.danger ? 'danger' : 'primary'}`} autoFocus onClick={spec.onConfirm}>{spec.confirmLabel}</button>
      </div>
    </div>
  </div>
}

/* ---------- charts ---------- */
interface TipEntry { name?: string; value?: number | string; color?: string; dataKey?: string | number }
export function RXTooltip({ active, payload, label, unit, formatter }: {
  active?: boolean; payload?: TipEntry[]; label?: string | number
  unit?: string; formatter?: (v: number | string) => string
}) {
  if (!active || !payload || payload.length === 0) return null
  return <div style={{
    background: 'var(--rx-surface-3)', border: '1px solid var(--rx-border-strong)',
    borderRadius: 9, padding: '8px 11px', fontSize: 12, color: 'var(--rx-text-secondary)',
  }}>
    <div style={{ color: 'var(--rx-text-muted)', marginBottom: 4 }}>{label}</div>
    {payload.map((entry, i) => <div key={i} style={{ display: 'flex', alignItems: 'center', gap: 7, marginTop: 2 }}>
      <span style={{ width: 8, height: 8, borderRadius: 3, background: entry.color }} />
      <span>{entry.name}</span>
      <span style={{ marginLeft: 'auto', fontFamily: 'var(--rx-font-mono)', color: 'var(--rx-text)', paddingLeft: 12 }}>
        {formatter ? formatter(entry.value ?? '') : `${entry.value}${unit ?? ''}`}
      </span>
    </div>)}
  </div>
}

export function ChartFrame({ caption, children, height = '', ariaLabel }: {
  caption?: string; children: ReactNode; height?: 'short' | 'tall' | ''; ariaLabel?: string
}) {
  return <div>
    <div className={`rx-chart ${height}`} role="img" aria-label={ariaLabel ?? 'Chart of recent measurements'}>{children}</div>
    {caption && <p className="rx-chart-caption">{caption}</p>}
  </div>
}

/* ---------- loading / empty / error ---------- */
export function Skeleton({ w, h, style }: { w?: string; h?: number | string; style?: CSSProperties }) {
  return <div className="rx-skeleton" style={{ width: w ?? '100%', height: h ?? 14, ...style }} aria-hidden="true" />
}
export function ChartSkeleton() {
  return <div className="rx-chart" aria-hidden="true"><Skeleton h="100%" /></div>
}
export function EmptyState({ icon: Icon, title, detail, action }: {
  icon?: ComponentType<{ size?: number | string }>; title: string; detail: string; action?: ReactNode
}) {
  return <div className="rx-empty">
    {Icon && <Icon size={30} />}
    <h3>{title}</h3>
    <p>{detail}</p>
    {action}
  </div>
}
