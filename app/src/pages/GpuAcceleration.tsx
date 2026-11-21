import { useEffect, useState } from 'react'
import { Bar, BarChart, CartesianGrid, Legend, ResponsiveContainer, Tooltip, XAxis, YAxis } from 'recharts'
import {
  Gauge, ListTodo, MonitorCog, Play, RefreshCw, Trash2, X, Zap,
} from 'lucide-react'
import { useGpuStore } from '../stores/gpu'
import { onGpuCapabilitiesEvent, onGpuTaskEvent } from '../services/gpu'
import {
  Button, Card, ChartFrame, ConfirmDialog, DetailList, Disclose, EmptyState, InfoTip, PageHeader,
  RXTooltip, ResourceBar, StatusChip, Tabs, ToggleGroup, type ConfirmSpec,
} from '../ui/kit'
import { fmtBytes } from '../lib/format'
import { rxAxis, rxGrid, rxTick } from '../ui/chart-config'
import { toast } from '../ui/toast-store'
import type {
  AccelerationConfig, AccelerationMode, BackendKind, GpuTask, ProviderStatus,
  TaskStatus, WorkloadParams, WorkloadSetting, WorkloadView,
} from '../types/gpu'

const TABS = [
  { id: 'overview', label: 'Overview' },
  { id: 'workloads', label: 'Workload intelligence' },
  { id: 'center', label: 'Acceleration center' },
  { id: 'tasks', label: 'Task monitor' },
  { id: 'benchmarks', label: 'Performance comparison' },
] as const

const MODE_LABELS: Record<AccelerationMode, string> = {
  cpuOnly: 'CPU only',
  gpuAcceleration: 'GPU acceleration',
  intelligentAuto: 'Intelligent auto',
  manual: 'Manual',
}
const MODE_HINTS: Record<AccelerationMode, string> = {
  cpuOnly: 'Every workload runs on the CPU; the GPU is still monitored.',
  gpuAcceleration: 'Supported workloads use the GPU when available and safe.',
  intelligentAuto: 'The engine decides per workload from size and measured history.',
  manual: 'Per-workload backend pins are honoured.',
}
const CLASS_LABELS: Record<string, string> = {
  gpuRecommended: 'GPU recommended',
  cpuRecommended: 'CPU recommended',
  gpuAvailableNotBeneficial: 'GPU available, not beneficial',
  insufficientGpuResources: 'Insufficient GPU resources',
  unsupportedWorkload: 'Unsupported workload',
  accelerationUnavailable: 'Acceleration unavailable',
}
const TASK_STATUS: Record<TaskStatus, { status: 'healthy' | 'normal' | 'attention' | 'critical' | 'unavailable'; label: string }> = {
  queued: { status: 'unavailable', label: 'Queued' },
  running: { status: 'normal', label: 'Running' },
  succeeded: { status: 'healthy', label: 'Completed' },
  failed: { status: 'critical', label: 'Failed' },
  cancelled: { status: 'attention', label: 'Cancelled' },
  timedOut: { status: 'critical', label: 'Timed out' },
  rejected: { status: 'attention', label: 'Rejected' },
}
const backendLabel = (backend: BackendKind | null) => backend === 'cuda' ? 'CUDA' : backend === 'wgpu' ? 'WebGPU (DX12)' : 'CPU'

const ms = (value: number | null) => (value === null ? '—' : `${value.toFixed(2)} ms`)

function paramsFor(workloadId: string, size: number): WorkloadParams {
  switch (workloadId) {
    case 'vector_scale': return { kind: 'vectorScale', n: size, a: 1.5 }
    case 'matmul': return { kind: 'matmul', m: size, k: size, n: size }
    case 'row_sum': return { kind: 'rowSum', rows: size, cols: size }
    default: return { kind: 'vectorAxpy', n: size, a: 1.5 }
  }
}

function upsertSetting(settings: WorkloadSetting[], id: string, patch: Partial<WorkloadSetting>): WorkloadSetting[] {
  const existing = settings.find((item) => item.workloadId === id)
  const base: WorkloadSetting = existing ?? { workloadId: id, mode: 'intelligentAuto', backend: 'cpu', enabled: true, selectedSize: 0 }
  const next = { ...base, ...patch }
  return existing ? settings.map((item) => (item.workloadId === id ? next : item)) : [...settings, next]
}

function ProviderCard({ provider }: { provider: ProviderStatus }) {
  const chip = provider.computeReady ? <StatusChip status="healthy" label="Ready" />
    : provider.available ? <StatusChip status="attention" label="Limited" />
    : <StatusChip status="unavailable" label="Unavailable" />
  return <div className="rx-rec-card">
    <h3>{provider.label} <span style={{ marginLeft: 6 }}>{chip}</span></h3>
    <p>{provider.detail}</p>
    {provider.lastError && <p className="rx-chart-caption" role="status">{provider.lastError}</p>}
    <DetailList rows={[
      { label: 'Devices', value: provider.devices.length ? provider.devices.map((device) => device.name).join(', ') : 'None' },
      { label: 'Selected device', value: provider.selectedDevice || '—' },
    ]} />
  </div>
}

function TaskTable({ tasks, onCancel, confirm }: { tasks: GpuTask[]; onCancel?: (id: number) => void; confirm: (spec: ConfirmSpec) => void }) {
  if (tasks.length === 0) {
    return <EmptyState title="No tasks yet" detail="Run a workload from the Workload intelligence tab to see it here." />
  }
  return <div className="rx-table-wrap"><table className="rx-table">
    <thead><tr><th>Task</th><th>Backend</th><th>Status</th><th style={{ width: '22%' }}>Progress</th><th className="num">Duration</th>{onCancel && <th />}</tr></thead>
    <tbody>
      {tasks.map((task) => {
        const meta = TASK_STATUS[task.status]
        return <tr key={task.id}>
          <td><strong>{task.title}</strong>{task.fellBack && <span className="rx-chip attention" style={{ marginLeft: 6 }}>fell back to CPU</span>}</td>
          <td><span className="rx-chip info">{backendLabel(task.selectedBackend ?? task.requestedBackend)}</span></td>
          <td><StatusChip status={meta.status} label={meta.label} /></td>
          <td>{task.status === 'running' && task.progressPercent != null
            ? <div className="rx-metric-bar" style={{ ['--rx-metric-color' as string]: 'var(--rx-c-other)' }}><span style={{ width: `${task.progressPercent}%` }} /></div>
            : <span style={{ color: 'var(--rx-text-muted)', fontSize: 12 }}>{task.progressPercent == null ? '—' : `${Math.round(task.progressPercent)}%`}</span>}</td>
          <td className="num">{task.durationMs == null ? '—' : task.durationMs < 1000 ? `${task.durationMs} ms` : `${(task.durationMs / 1000).toFixed(1)} s`}</td>
          {onCancel && <td>{(task.status === 'queued' || task.status === 'running') && <Button small variant="danger" icon={X}
            onClick={() => confirm({
              title: 'Cancel this task?',
              body: 'The GPU task stops immediately and its partial result is discarded. Completed work is unaffected.',
              confirmLabel: 'Cancel task',
              onConfirm: () => onCancel(task.id),
              onCancel: () => undefined,
            })}>Cancel</Button>}</td>}
        </tr>
      })}
    </tbody>
  </table></div>
}

export default function GpuAccelerationPage() {
  const state = useGpuStore((s) => s.state)
  const config = useGpuStore((s) => s.config)
  const benchmarks = useGpuStore((s) => s.benchmarks)
  const history = useGpuStore((s) => s.history)
  const loading = useGpuStore((s) => s.loading)
  const benchmarkBusy = useGpuStore((s) => s.benchmarkBusy)
  const error = useGpuStore((s) => s.error)
  const prefs = useGpuStore((s) => s.prefs)
  const refresh = useGpuStore((s) => s.refresh)
  const setMode = useGpuStore((s) => s.setMode)
  const saveConfig = useGpuStore((s) => s.saveConfig)
  const submit = useGpuStore((s) => s.submit)
  const cancel = useGpuStore((s) => s.cancel)
  const clearFinished = useGpuStore((s) => s.clearFinished)
  const runBenchmark = useGpuStore((s) => s.runBenchmark)
  const probe = useGpuStore((s) => s.probe)
  const loadHistory = useGpuStore((s) => s.loadHistory)
  const loadBenchmarks = useGpuStore((s) => s.loadBenchmarks)
  const clearBenchmarks = useGpuStore((s) => s.clearBenchmarks)
  const setPrefs = useGpuStore((s) => s.setPrefs)

  const [draft, setDraft] = useState<AccelerationConfig | null>(null)
  const [benchWorkload, setBenchWorkload] = useState('vector_axpy')
  const [benchSize, setBenchSize] = useState(1_048_576)
  const [warmup, setWarmup] = useState(2)
  const [runs, setRuns] = useState(5)
  const [confirm, setConfirm] = useState<ConfirmSpec | null>(null)

  useEffect(() => {
    void refresh()
    void loadBenchmarks()
    void loadHistory()
  }, [refresh, loadBenchmarks, loadHistory])

  useEffect(() => {
    const task = onGpuTaskEvent((payload) => useGpuStore.getState().applyTaskEvent(payload))
    const caps = onGpuCapabilitiesEvent(() => void useGpuStore.getState().refresh())
    return () => {
      void task.then((unlisten) => unlisten())
      void caps.then((unlisten) => unlisten())
    }
  }, [])

  useEffect(() => {
    if (!prefs.autoRefresh) return undefined
    const id = setInterval(() => {
      if (document.visibilityState === 'visible') void useGpuStore.getState().refresh()
    }, 3000)
    return () => clearInterval(id)
  }, [prefs.autoRefresh])

  if (error && !state) {
    return <>
      <PageHeader title="GPU acceleration" description="Run supported workloads on your graphics card instead of the CPU — when it helps." />
      <Card><EmptyState icon={Zap} title="GPU engine unavailable" detail={error}
        action={<Button variant="primary" onClick={() => void refresh()}>Retry</Button>} /></Card>
    </>
  }
  if (!state || !config) {
    return <>
      <PageHeader title="GPU acceleration" description="Run supported workloads on your graphics card instead of the CPU — when it helps." />
      <Card>
        <div className="rx-stack" aria-hidden="true">
          <div className="rx-skeleton" style={{ height: 90 }} />
          <div className="rx-skeleton" style={{ height: 200 }} />
        </div>
        <EmptyState icon={Zap} title="Loading GPU engine" detail="Reading provider capabilities and the resource budget…" />
      </Card>
    </>
  }

  const active = draft ?? config
  const patchDraft = (patch: Partial<AccelerationConfig>) => setDraft({ ...active, ...patch })
  const runWorkload = (workload: WorkloadView) => {
    void submit({
      workloadId: workload.id,
      params: paramsFor(workload.id, workload.selectedSize),
      priority: 'normal',
      backend: null,
      clientRef: null,
      requireGpu: false,
      timeoutMs: null,
    }).then((task) => {
      if (task) toast(`"${workload.title}" submitted (${backendLabel(task.selectedBackend ?? task.requestedBackend)})`, 'success')
    })
  }
  const changeSize = (workload: WorkloadView, size: number) => {
    setPrefs({ selectedSizes: { ...prefs.selectedSizes, [workload.id]: size } })
    void saveConfig({ ...config, workloadSettings: upsertSetting(config.workloadSettings, workload.id, { selectedSize: size }) })
  }
  const benchSizes = state.workloads.find((w) => w.id === benchWorkload)?.sizes ?? [1_048_576]
  const resources = state.resources
  const budgetPercent = resources.budgetBytes > 0 ? (resources.reservedBytes / resources.budgetBytes) * 100 : null
  const benchRecords = benchmarks.slice(0, 8).reverse().map((record) => ({
    name: `${record.workloadId} #${record.id}`,
    cpu: Number(record.cpu.medianMs.toFixed(3)),
    gpu: record.gpu === null ? null : Number(record.gpu.medianMs.toFixed(3)),
  }))
  const latestBenchmark = benchmarks[0] ?? null

  return <>
    <PageHeader title="GPU acceleration"
      description="Run supported workloads on your graphics card instead of the CPU. FluxCore only uses the GPU's own memory (VRAM) — never as system RAM." />

    <Tabs tabs={TABS.map((tab) => ({ id: tab.id, label: tab.label, count: tab.id === 'tasks' ? state.tasks.length : undefined }))}
      active={prefs.lastTab} onChange={(id) => setPrefs({ lastTab: id })} />

    {error && <div className="rx-card status-edge critical" role="alert">
      <div className="rx-card-head"><h2>The last GPU operation failed</h2></div>
      <p className="rx-card-sub" style={{ margin: 0 }}>{error}</p>
    </div>}

    {prefs.lastTab === 'overview' && <>
      {!state.available && <Card><EmptyState icon={Zap} title="GPU acceleration is not available on this system"
        detail={state.unavailableReason ?? 'No supported compute provider was found. Supported workloads will run on the CPU automatically.'} /></Card>}
      {state.safetyNotes.map((note) => <p className="rx-chart-caption" key={note}>{note}</p>)}
      <div className="rx-grid-side">
        <Card title="Compute providers" icon={Zap} sub="The paths FluxCore can use to run a workload. The first ready provider in the priority order wins.">
          <div className="rx-stack" style={{ gap: 10 }}>
            {state.providers.map((provider) => <ProviderCard key={provider.id} provider={provider} />)}
          </div>
        </Card>
        <Card title="VRAM budget" icon={Gauge}
          sub="FluxCore only ever budgets VRAM for its own GPU compute tasks. VRAM is the graphics card's own memory and is never used as system RAM.">
          <ResourceBar label="Budgeted for FluxCore tasks" value={fmtBytes(resources.reservedBytes)} total={fmtBytes(resources.budgetBytes)}
            percent={budgetPercent} color="var(--rx-c-vram)" />
          <DetailList rows={[
            { label: 'Dedicated VRAM total', value: fmtBytes(resources.totalVramBytes) },
            { label: 'Dedicated VRAM free', value: fmtBytes(resources.freeVramBytes) },
            { label: 'Remaining budget', value: fmtBytes(resources.remainingBudgetBytes) },
            { label: 'Running GPU tasks', value: `${resources.runningGpuTasks} / ${resources.maxConcurrentGpuTasks}` },
            { label: 'GPU utilization', value: resources.gpuUtilizationPercent == null ? 'Unavailable' : `${resources.gpuUtilizationPercent.toFixed(0)}%` },
            { label: 'GPU temperature', value: resources.temperatureC == null ? 'Unavailable' : `${resources.temperatureC.toFixed(0)} °C` },
          ]} />
          {resources.gates.map((gate) => <p className="rx-chart-caption" key={gate}>{gate}</p>)}
        </Card>
      </div>
      <Card>
        <Disclose summary="Environment details">
          <div className="rx-disclose-body">
            <DetailList rows={[
              { label: 'OS', value: `${state.environment.osName} ${state.environment.osVersion}` },
              { label: 'CPU', value: state.environment.cpuName },
              { label: 'Logical processors', value: String(state.environment.logicalProcessors) },
              { label: 'GPU', value: state.environment.gpuName },
              { label: 'Driver', value: state.environment.driverVersion ?? 'Unavailable' },
              { label: 'CUDA toolkit', value: state.environment.cudaToolkitPresent ? 'Present' : 'Absent' },
              { label: 'CUDA compute path', value: state.environment.cudaComputePath },
              { label: 'wgpu', value: `${state.environment.wgpuVersion} (${state.environment.wgpuBackend ?? 'no adapter'})` },
              { label: 'Vulkan / DX12', value: `${state.environment.vulkanPresent ? 'yes' : 'no'} / ${state.environment.dx12Present ? 'yes' : 'no'}` },
            ]} />
            <div style={{ marginTop: 12 }}>
              <Button icon={RefreshCw} loading={loading} onClick={() => void probe(true)}>Re-probe providers</Button>
            </div>
          </div>
        </Disclose>
      </Card>
    </>

    }

    {prefs.lastTab === 'workloads' && <Card title="Detected workloads & recommendations" icon={Zap}
      sub="FluxCore matches each workload to the fastest available path and tells you why. Sizes and runs are safe and repeatable.">
      <div className="rx-stack" style={{ gap: 12 }}>
        {state.workloads.map((workload) => {
          const assessment = workload.assessment
          return <div className="rx-rec-card" key={workload.id}>
            <h3>{workload.title} <span className="rx-chip neutral" style={{ marginLeft: 6 }}>{workload.selectedSize.toLocaleString()} elements</span></h3>
            <div className="rx-rec-meta">
              <span className={`rx-chip ${assessment.classification === 'gpuRecommended' ? 'success' : assessment.classification === 'cpuRecommended' ? 'neutral' : 'warning'}`}>
                {CLASS_LABELS[assessment.classification] ?? assessment.classification}
              </span>
              <span className="rx-chip info">{backendLabel(assessment.selectedBackend)}</span>
              <span className="rx-chip neutral">{assessment.estimateSource === 'measured' ? 'measured evidence' : assessment.estimateSource === 'mixed' ? 'mixed evidence' : 'model estimate'}</span>
            </div>
            <p><strong>Why:</strong> {assessment.reason}</p>
            <DetailList rows={[
              { label: 'CPU estimate', value: ms(assessment.estimatedCpuMs) },
              { label: 'GPU estimate (end-to-end)', value: ms(assessment.estimatedGpuTotalMs) },
              { label: 'Transfer overhead', value: `${ms(assessment.transferOverheadMs)} (${fmtBytes(assessment.transferBytes)})` },
              { label: 'VRAM needed / budget', value: `${fmtBytes(assessment.requiredVramBytes)} / ${fmtBytes(assessment.gpuBudgetBytes)}` },
            ]} />
            {assessment.blockers.map((blocker) => <p className="rx-chart-caption" key={blocker} style={{ color: 'var(--rx-warning)' }}>{blocker}</p>)}
            <div className="rx-rec-row">
              <label style={{ display: 'flex', alignItems: 'center', gap: 7, fontSize: 12.5 }}>
                Size
                <select className="rx-select" value={workload.selectedSize} aria-label={`${workload.title} size`}
                  onChange={(event) => changeSize(workload, Number(event.target.value))}>
                  {workload.sizes.map((size) => <option key={size} value={size}>{size.toLocaleString()}</option>)}
                </select>
              </label>
              <Button variant="primary" icon={Play} loading={loading} onClick={() => runWorkload(workload)}>Run workload</Button>
              {workload.lastBenchmark && <span className="rx-chart-caption" title={workload.lastBenchmark.verdict}>
                last benchmark: {workload.lastBenchmark.speedup === null ? 'CPU only' : `${workload.lastBenchmark.speedup.toFixed(2)}× faster on GPU`}
              </span>}
            </div>
          </div>
        })}
      </div>
    </Card>}

    {prefs.lastTab === 'center' && <>
      <Card title="Execution mode" icon={Zap} sub="What happens when a supported workload starts.">
        <ToggleGroup
          options={(Object.keys(MODE_LABELS) as AccelerationMode[]).map((value) => ({ id: value, label: MODE_LABELS[value] }))}
          value={state.mode}
          onChange={(id) => void setMode(id as AccelerationMode).then(() => toast(`Execution mode set to "${MODE_LABELS[id as AccelerationMode]}"`, 'success'))}
        />
        <p className="rx-chart-caption" style={{ marginTop: 10 }}>{MODE_HINTS[state.mode]}</p>
      </Card>

      <div className="rx-grid-2">
        <Card title="Resource limits" icon={Gauge}
          sub="Hard caps that protect the GPU and your VRAM budget."
          right={<><Button small onClick={() => setDraft(null)}>Reset</Button><Button small variant="primary" onClick={() => void saveConfig(active).then(() => toast('Resource limits saved', 'success'))}>Save</Button></>}>
          <div className="rx-detail-grid" style={{ gridTemplateColumns: '1fr 1fr' }}>
            <div className="rx-field"><label>VRAM budget (%)</label><input className="rx-input rx-number-input" type="number" min={1} max={90} value={active.vramBudgetPercent} aria-label="VRAM budget percent" onChange={(e) => patchDraft({ vramBudgetPercent: Number(e.target.value) })} /><span className="help">Share of dedicated VRAM FluxCore may reserve.</span></div>
            <div className="rx-field"><label>VRAM cap (MiB, 0 = none)</label><input className="rx-input rx-number-input" type="number" min={0} max={4096} value={active.vramBudgetCapMb} aria-label="VRAM cap in MiB" onChange={(e) => patchDraft({ vramBudgetCapMb: Number(e.target.value) })} /><span className="help">Absolute upper bound on top of the percent.</span></div>
            <div className="rx-field"><label>Max GPU utilization (%)</label><input className="rx-input rx-number-input" type="number" min={10} max={100} value={active.maxGpuUtilizationPercent} aria-label="Max GPU utilization percent" onChange={(e) => patchDraft({ maxGpuUtilizationPercent: Number(e.target.value) })} /><span className="help">Tasks defer when the GPU is busier.</span></div>
            <div className="rx-field"><label>Max temperature (°C)</label><input className="rx-input rx-number-input" type="number" min={40} max={110} value={active.maxTemperatureC} aria-label="Max temperature" onChange={(e) => patchDraft({ maxTemperatureC: Number(e.target.value) })} /><span className="help">Tasks defer above this temperature.</span></div>
            <div className="rx-field"><label>Concurrent GPU tasks</label><input className="rx-input rx-number-input" type="number" min={1} max={2} value={active.maxConcurrentGpuTasks} aria-label="Concurrent GPU tasks" onChange={(e) => patchDraft({ maxConcurrentGpuTasks: Number(e.target.value) })} /><span className="help">How many GPU tasks may run at once.</span></div>
            <div className="rx-field"><label>Concurrent CPU tasks</label><input className="rx-input rx-number-input" type="number" min={1} max={4} value={active.maxConcurrentCpuTasks} aria-label="Concurrent CPU tasks" onChange={(e) => patchDraft({ maxConcurrentCpuTasks: Number(e.target.value) })} /><span className="help">CPU fallback parallelism.</span></div>
            <div className="rx-field"><label>Max queue length</label><input className="rx-input rx-number-input" type="number" min={1} max={256} value={active.maxQueueLength} aria-label="Max queue length" onChange={(e) => patchDraft({ maxQueueLength: Number(e.target.value) })} /><span className="help">Pending tasks allowed before rejection.</span></div>
            <div className="rx-field"><label>Task timeout (ms)</label><input className="rx-input rx-number-input" type="number" min={500} max={60000} step={100} value={active.taskTimeoutMs} aria-label="Task timeout" onChange={(e) => patchDraft({ taskTimeoutMs: Number(e.target.value) })} /><span className="help">Hard wall-clock limit per task.</span></div>
            <div className="rx-field"><label>Min system RAM available (%)</label><input className="rx-input rx-number-input" type="number" min={1} max={50} value={active.minSystemRamAvailablePercent} aria-label="Min system RAM available percent" onChange={(e) => patchDraft({ minSystemRamAvailablePercent: Number(e.target.value) })} /><span className="help">Safety floor for shared-memory fallbacks.</span></div>
            <div className="rx-field"><label>Retention (days)</label><input className="rx-input rx-number-input" type="number" min={1} max={90} value={active.retentionDays} aria-label="History retention days" onChange={(e) => patchDraft({ retentionDays: Number(e.target.value) })} /><span className="help">How long task/benchmark history is kept.</span></div>
          </div>
          <div className="rx-setting-row" style={{ borderBottom: 0 }}>
            <div className="rx-setting-text"><h3>Validate GPU results</h3><p>Compare every GPU output against the CPU reference.</p></div>
            <div className="rx-setting-control"><Button small variant={active.validateResults ? 'primary' : 'secondary'} onClick={() => patchDraft({ validateResults: !active.validateResults })}>{active.validateResults ? 'On' : 'Off'}</Button></div>
          </div>
          <div className="rx-setting-row">
            <div className="rx-setting-text"><h3>Persist history</h3><p>Store task history and benchmarks in SQLite.</p></div>
            <div className="rx-setting-control"><Button small variant={active.persistHistory ? 'primary' : 'secondary'} onClick={() => patchDraft({ persistHistory: !active.persistHistory })}>{active.persistHistory ? 'On' : 'Off'}</Button></div>
          </div>
          <div className="rx-setting-row">
            <div className="rx-setting-text"><h3>Live task events</h3><p>Emit fluxcore-gpu-task events to the UI.</p></div>
            <div className="rx-setting-control"><Button small variant={active.emitEvents ? 'primary' : 'secondary'} onClick={() => patchDraft({ emitEvents: !active.emitEvents })}>{active.emitEvents ? 'On' : 'Off'}</Button></div>
          </div>
          <div className="rx-setting-row" style={{ borderBottom: 0 }}>
            <div className="rx-setting-text"><h3>Auto refresh</h3><p>Refresh this page while it is visible.</p></div>
            <div className="rx-setting-control"><Button small variant={prefs.autoRefresh ? 'primary' : 'secondary'} onClick={() => setPrefs({ autoRefresh: !prefs.autoRefresh })}>{prefs.autoRefresh ? 'On' : 'Off'}</Button></div>
          </div>
        </Card>

        <div className="rx-stack">
          <Card title="Provider priority" icon={ListTodo} sub="Order matters: the first ready provider runs the workload.">
            <div className="rx-stack" style={{ gap: 8 }}>
              {active.providerPriority.map((id, index) => {
                const provider = state.providers.find((entry) => entry.id === id)
                return <div key={id} style={{ display: 'flex', alignItems: 'center', gap: 10, background: 'var(--rx-surface-2)', borderRadius: 8, padding: '9px 12px' }}>
                  <span className="rx-chip neutral">{index + 1}</span>
                  <span style={{ fontWeight: 600, fontSize: 13 }}>{provider?.label ?? id}</span>
                  <span className="rx-data-age" style={{ marginLeft: 'auto' }}>{provider?.computeReady ? 'Ready' : 'Unavailable'}</span>
                  <Button small disabled={index === 0 || draft === null} onClick={() => {
                    if (draft === null) setDraft({ ...config })
                    const order = [...(draft ?? config).providerPriority]
                    ;[order[index - 1], order[index]] = [order[index], order[index - 1]]
                    patchDraft({ providerPriority: order })
                  }}>Up</Button>
                </div>
              })}
            </div>
          </Card>
          <Card>
            <Disclose summary="Per-workload pins (Manual mode)">
              <div className="rx-disclose-body">
                <p className="rx-card-sub" style={{ margin: '0 0 10px' }}>Honoured when the execution mode is Manual.</p>
                {state.workloads.map((workload) => {
                  const setting = active.workloadSettings.find((item) => item.workloadId === workload.id)
                  return <div className="rx-setting-row" key={workload.id}>
                    <div className="rx-setting-text"><h3>{workload.title}</h3><p>Mode and backend used in Manual mode.</p></div>
                    <div className="rx-setting-control" style={{ display: 'flex', gap: 8 }}>
                      <select className="rx-select" value={setting?.mode ?? 'intelligentAuto'} aria-label={`${workload.title} mode`}
                        onChange={(e) => patchDraft({ workloadSettings: upsertSetting(active.workloadSettings, workload.id, { mode: e.target.value as AccelerationMode }) })}>
                        {(Object.keys(MODE_LABELS) as AccelerationMode[]).map((value) => <option key={value} value={value}>{MODE_LABELS[value]}</option>)}
                      </select>
                      <select className="rx-select" value={setting?.backend ?? 'cpu'} aria-label={`${workload.title} backend`}
                        onChange={(e) => patchDraft({ workloadSettings: upsertSetting(active.workloadSettings, workload.id, { backend: e.target.value as BackendKind }) })}>
                        <option value="cpu">CPU</option><option value="cuda">CUDA</option><option value="wgpu">wgpu</option>
                      </select>
                    </div>
                  </div>
                })}
              </div>
            </Disclose>
          </Card>
        </div>
      </div>
    </>

    }

    {prefs.lastTab === 'tasks' && <>
      <Card title="Active tasks" icon={ListTodo} right={<Button small icon={Trash2} onClick={() => void clearFinished().then(() => toast('Finished tasks cleared', 'success'))}>Clear finished</Button>}>
        <TaskTable tasks={state.tasks} confirm={setConfirm} onCancel={(id) => void cancel(id).then(() => toast('Task cancelled', 'info'))} />
      </Card>
      <Card title="Task history" icon={ListTodo} note="Kept locally on this PC" right={<Button small icon={RefreshCw} onClick={() => void loadHistory()}>Reload</Button>}>
        <TaskTable tasks={history} confirm={setConfirm} />
      </Card>
    </>}

    {prefs.lastTab === 'benchmarks' && <>
      <Card title="Run a benchmark" icon={Play}
        sub="Runs the same workload on CPU and GPU and compares the time. Safe and repeatable — results are kept locally.">
        <div style={{ display: 'flex', gap: 12, alignItems: 'flex-end', flexWrap: 'wrap' }}>
          <div className="rx-field"><label>Workload</label>
            <select className="rx-select" value={benchWorkload} aria-label="Benchmark workload"
              onChange={(e) => { setBenchWorkload(e.target.value); setBenchSize(state.workloads.find((w) => w.id === e.target.value)?.sizes[0] ?? 1_048_576) }}>
              {state.workloads.map((workload) => <option key={workload.id} value={workload.id}>{workload.title}</option>)}
            </select></div>
          <div className="rx-field"><label>Size</label>
            <select className="rx-select" value={benchSize} aria-label="Benchmark size" onChange={(e) => setBenchSize(Number(e.target.value))}>
              {benchSizes.map((size) => <option key={size} value={size}>{size.toLocaleString()}</option>)}
            </select></div>
          <div className="rx-field"><label>Warm-up runs</label><input className="rx-input rx-number-input" type="number" min={0} max={10} value={warmup} aria-label="Warm-up runs" onChange={(e) => setWarmup(Number(e.target.value))} /></div>
          <div className="rx-field"><label>Measured runs</label><input className="rx-input rx-number-input" type="number" min={1} max={50} value={runs} aria-label="Measured runs" onChange={(e) => setRuns(Number(e.target.value))} /></div>
          <Button variant="primary" icon={Play} loading={benchmarkBusy}
            onClick={() => void runBenchmark({
              workloadId: benchWorkload, params: paramsFor(benchWorkload, benchSize),
              warmupRuns: warmup, measuredRuns: runs, validate: true,
            }).then((record) => {
              if (record) toast(`Benchmark finished: ${record.verdict}`, 'success')
            })}>
            {benchmarkBusy ? 'Benchmarking…' : 'Run benchmark'}
          </Button>
        </div>
        <p className="rx-chart-caption" style={{ marginTop: 10 }}>
          Benchmarks use identical inputs on both backends, discard warm-ups and report medians. A slowdown is reported as a slowdown.
        </p>
      </Card>

      <Card title="Measured comparison" icon={Gauge} right={<Button small icon={Trash2} onClick={() => void clearBenchmarks().then(() => toast('Benchmark history cleared', 'info'))}>Clear history</Button>}>
        {benchRecords.length === 0
          ? <EmptyState title="No benchmarks yet" detail="Run a CPU-vs-GPU benchmark to see measured comparisons here." />
          : <ChartFrame ariaLabel="Benchmark comparison chart" height="tall"
              caption="Lower is better (median duration, ms). GPU bars show end-to-end time including transfers.">
              <ResponsiveContainer width="100%" height="100%">
                <BarChart data={benchRecords} margin={{ top: 8, right: 8, bottom: 0, left: -8 }}>
                  <CartesianGrid {...rxGrid} />
                  <XAxis dataKey="name" {...rxAxis} tick={rxTick} />
                  <YAxis {...rxAxis} tick={rxTick} unit=" ms" />
                  <Tooltip content={<RXTooltip formatter={(v) => `${Number(v).toFixed(2)} ms`} />} cursor={{ fill: 'var(--rx-accent-subtle)' }} />
                  <Legend wrapperStyle={{ color: 'var(--rx-text-muted)', fontSize: 12 }} />
                  <Bar dataKey="cpu" name="CPU median" fill="var(--rx-c-other)" radius={[6, 6, 0, 0]} isAnimationActive={false} />
                  <Bar dataKey="gpu" name="GPU median (end-to-end)" fill="var(--rx-c-gpu)" radius={[6, 6, 0, 0]} isAnimationActive={false} />
                </BarChart>
              </ResponsiveContainer>
            </ChartFrame>}
        {latestBenchmark && <div className="rx-hero" style={{ padding: 14 }}>
          <MonitorCog size={24} style={{ color: 'var(--rx-c-gpu)' }} />
          <div className="rx-hero-text">
            <h2>{latestBenchmark.workloadId.replaceAll('_', ' ')} · {latestBenchmark.verdict}</h2>
            <p>CPU median {latestBenchmark.cpu.medianMs.toFixed(2)} ms · GPU median {latestBenchmark.gpu == null ? 'did not run' : `${latestBenchmark.gpu.medianMs.toFixed(2)} ms`}{latestBenchmark.transferMs != null ? ` · transfer ${latestBenchmark.transferMs.toFixed(2)} ms` : ''}{latestBenchmark.speedup != null ? ` · ${latestBenchmark.speedup.toFixed(2)}× speed-up` : ''}</p>
          </div>
        </div>}
        <InfoTip text="Medians filter out outliers from Windows scheduling. Benchmarks never modify models or files; they only measure." />
      </Card>
    </>}

    <ConfirmDialog spec={confirm} />
  </>
}
