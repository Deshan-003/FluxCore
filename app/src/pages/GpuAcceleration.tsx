import { useEffect, useState } from 'react'
import {
  Gauge, ListChecks, MonitorCog, RefreshCw, Sparkles, Trash2, Zap,
} from 'lucide-react'
import { Tabs, TabsContent, TabsList, TabsTrigger } from '../components/ui/tabs'
import {
  Detail, EmptyState, Panel, SensorNotice, SettingRow, Toggle,
} from '../components/monitoring'
import { useGpuStore } from '../stores/gpu'
import { onGpuCapabilitiesEvent, onGpuTaskEvent } from '../services/gpu'
import type {
  AccelerationConfig, AccelerationMode, BackendKind, WorkloadParams, WorkloadSetting, WorkloadView,
} from '../types/gpu'
import GpuStatusCard from '../components/gpu/GpuStatusCard'
import ResourceBudgetPanel from '../components/gpu/ResourceBudgetPanel'
import WorkloadRecommendationRow from '../components/gpu/WorkloadRecommendationRow'
import TaskMonitorTable from '../components/gpu/TaskMonitorTable'
import BenchmarkComparisonChart from '../components/gpu/BenchmarkComparisonChart'
import UnsupportedState from '../components/gpu/UnsupportedState'

const MODES: { value: AccelerationMode; label: string; hint: string }[] = [
  { value: 'cpuOnly', label: 'CPU Only', hint: 'Every workload runs on the CPU; GPU is still monitored.' },
  { value: 'gpuAcceleration', label: 'GPU Acceleration', hint: 'Supported workloads use the GPU when available and safe.' },
  { value: 'intelligentAuto', label: 'Intelligent Auto', hint: 'The engine decides per workload from size and measured history.' },
  { value: 'manual', label: 'Manual', hint: 'Per-workload backend pins below are honoured.' },
]

function paramsFor(workloadId: string, size: number): WorkloadParams {
  switch (workloadId) {
    case 'vector_scale': return { kind: 'vectorScale', n: size, a: 1.5 }
    case 'matmul': return { kind: 'matmul', m: size, k: size, n: size }
    case 'row_sum': return { kind: 'rowSum', rows: size, cols: size }
    default: return { kind: 'vectorAxpy', n: size, a: 1.5 }
  }
}

function upsertSetting(
  settings: WorkloadSetting[], id: string, patch: Partial<WorkloadSetting>,
): WorkloadSetting[] {
  const existing = settings.find((item) => item.workloadId === id)
  const base: WorkloadSetting = existing ?? {
    workloadId: id, mode: 'intelligentAuto', backend: 'cpu', enabled: true, selectedSize: 0,
  }
  const next = { ...base, ...patch }
  return existing
    ? settings.map((item) => (item.workloadId === id ? next : item))
    : [...settings, next]
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

  const [draftOverride, setDraft] = useState<AccelerationConfig | null>(null)
  const [benchWorkload, setBenchWorkload] = useState('vector_axpy')
  const [benchSize, setBenchSize] = useState(1_048_576)
  const [warmup, setWarmup] = useState(2)
  const [runs, setRuns] = useState(5)

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
    return (
      <div className="gpu-page">
        <EmptyState title="GPU engine unavailable" detail={error} />
        <button className="button primary" onClick={() => void refresh()}>Retry</button>
      </div>
    )
  }
  if (!state || !config || !draft) {
    return (
      <div className="gpu-page">
        <EmptyState title="Loading GPU engine" detail="Reading provider capabilities and resource budget…" />
      </div>
    )
  }

  const draft = draftOverride ?? config
  const runWorkload = (workload: WorkloadView) => {
    void submit({
      workloadId: workload.id,
      params: paramsFor(workload.id, workload.selectedSize),
      priority: 'normal',
      backend: null,
      clientRef: null,
      requireGpu: false,
      timeoutMs: null,
    })
  }
  const changeSize = (workload: WorkloadView, size: number) => {
    setPrefs({ selectedSizes: { ...prefs.selectedSizes, [workload.id]: size } })
    void saveConfig({
      ...config,
      workloadSettings: upsertSetting(config.workloadSettings, workload.id, { selectedSize: size }),
    })
  }
  const patchDraft = (patch: Partial<AccelerationConfig>) => setDraft({ ...draft, ...patch })
  const benchSizes = state.workloads.find((w) => w.id === benchWorkload)?.sizes ?? [1_048_576]

  return (
    <div className="gpu-page">
      <Tabs
        value={prefs.lastTab}
        onValueChange={(value) => setPrefs({ lastTab: value })}
        className="gpu-tabs"
      >
        <TabsList>
          <TabsTrigger value="overview">Overview</TabsTrigger>
          <TabsTrigger value="workloads">Workload Intelligence</TabsTrigger>
          <TabsTrigger value="center">Acceleration Center</TabsTrigger>
          <TabsTrigger value="tasks">Task Monitor</TabsTrigger>
          <TabsTrigger value="benchmarks">Performance Comparison</TabsTrigger>
        </TabsList>

        <TabsContent value="overview">
          {!state.available && (
            <UnsupportedState reason={state.unavailableReason} />
          )}
          {state.safetyNotes.map((note) => <SensorNotice key={note} text={note} />)}
          <div className="gpu-columns">
            {state.providers.map((provider) => <GpuStatusCard key={provider.id} provider={provider} />)}
          </div>
          <ResourceBudgetPanel resources={state.resources} />
          <Panel title="Environment" icon={MonitorCog} action={
            <button className="button ghost" onClick={() => void probe(true)} disabled={loading}>
              <RefreshCw size={14} /> Re-probe
            </button>
          }>
            <div className="gpu-grid">
              <Detail label="OS" value={`${state.environment.osName} ${state.environment.osVersion}`} />
              <Detail label="CPU" value={state.environment.cpuName} />
              <Detail label="Logical processors" value={String(state.environment.logicalProcessors)} />
              <Detail label="GPU" value={state.environment.gpuName} />
              <Detail label="Driver" value={state.environment.driverVersion ?? 'Unavailable'} />
              <Detail label="CUDA toolkit" value={state.environment.cudaToolkitPresent ? 'Present' : 'Absent'} />
              <Detail label="CUDA compute path" value={state.environment.cudaComputePath} />
              <Detail label="wgpu" value={`${state.environment.wgpuVersion} (${state.environment.wgpuBackend ?? 'no adapter'})`} />
              <Detail label="Vulkan / DX12" value={`${state.environment.vulkanPresent ? 'yes' : 'no'} / ${state.environment.dx12Present ? 'yes' : 'no'}`} />
            </div>
          </Panel>
        </TabsContent>

        <TabsContent value="workloads">
          <Panel title="Workload suitability" icon={Sparkles}>
            {state.workloads.map((workload) => (
              <WorkloadRecommendationRow
                key={workload.id}
                workload={workload}
                busy={loading}
                onRun={runWorkload}
                onSize={changeSize}
              />
            ))}
          </Panel>
        </TabsContent>

        <TabsContent value="center">
          <Panel title="Execution mode" icon={Zap}>
            <SettingRow title="Acceleration mode" description={MODES.find((m) => m.value === state.mode)?.hint ?? ''}>
              <select
                value={state.mode}
                onChange={(event) => void setMode(event.target.value as AccelerationMode)}
              >
                {MODES.map((mode) => <option key={mode.value} value={mode.value}>{mode.label}</option>)}
              </select>
            </SettingRow>
          </Panel>
          <Panel title="Resource limits" icon={Gauge} action={
            <span>
              <button className="button ghost" onClick={() => setDraft(null)}>Reset</button>
              <button className="button primary" onClick={() => void saveConfig(draft)}>Save</button>
            </span>
          }>
            <div className="gpu-config-grid">
              <label>VRAM budget (%)<input type="number" min={1} max={90} step={1} value={draft.vramBudgetPercent} onChange={(e) => patchDraft({ vramBudgetPercent: Number(e.target.value) })} /></label>
              <label>VRAM cap (MiB, 0 = none)<input type="number" min={0} max={4096} value={draft.vramBudgetCapMb} onChange={(e) => patchDraft({ vramBudgetCapMb: Number(e.target.value) })} /></label>
              <label>Max GPU utilisation (%)<input type="number" min={10} max={100} value={draft.maxGpuUtilizationPercent} onChange={(e) => patchDraft({ maxGpuUtilizationPercent: Number(e.target.value) })} /></label>
              <label>Max temperature (°C)<input type="number" min={40} max={110} value={draft.maxTemperatureC} onChange={(e) => patchDraft({ maxTemperatureC: Number(e.target.value) })} /></label>
              <label>Concurrent GPU tasks<input type="number" min={1} max={2} value={draft.maxConcurrentGpuTasks} onChange={(e) => patchDraft({ maxConcurrentGpuTasks: Number(e.target.value) })} /></label>
              <label>Concurrent CPU tasks<input type="number" min={1} max={4} value={draft.maxConcurrentCpuTasks} onChange={(e) => patchDraft({ maxConcurrentCpuTasks: Number(e.target.value) })} /></label>
              <label>Max queue length<input type="number" min={1} max={256} value={draft.maxQueueLength} onChange={(e) => patchDraft({ maxQueueLength: Number(e.target.value) })} /></label>
              <label>Task timeout (ms)<input type="number" min={500} max={60000} step={100} value={draft.taskTimeoutMs} onChange={(e) => patchDraft({ taskTimeoutMs: Number(e.target.value) })} /></label>
              <label>Min system RAM available (%)<input type="number" min={1} max={50} value={draft.minSystemRamAvailablePercent} onChange={(e) => patchDraft({ minSystemRamAvailablePercent: Number(e.target.value) })} /></label>
              <label>Retention (days)<input type="number" min={1} max={90} value={draft.retentionDays} onChange={(e) => patchDraft({ retentionDays: Number(e.target.value) })} /></label>
            </div>
            <SettingRow title="Validate GPU results" description="Compare every GPU output against the CPU reference.">
              <Toggle checked={draft.validateResults} onChange={(checked) => patchDraft({ validateResults: checked })} />
            </SettingRow>
            <SettingRow title="Persist history" description="Store task history and benchmarks in SQLite.">
              <Toggle checked={draft.persistHistory} onChange={(checked) => patchDraft({ persistHistory: checked })} />
            </SettingRow>
            <SettingRow title="Live task events" description="Emit fluxcore-gpu-task events to the UI.">
              <Toggle checked={draft.emitEvents} onChange={(checked) => patchDraft({ emitEvents: checked })} />
            </SettingRow>
            <SettingRow title="Auto refresh" description="Refresh this page while it is visible.">
              <Toggle checked={prefs.autoRefresh} onChange={(checked) => setPrefs({ autoRefresh: checked })} />
            </SettingRow>
          </Panel>
          <Panel title="Provider priority" icon={ListChecks}>
            {draft.providerPriority.map((id, index) => (
              <SettingRow key={id} title={id} description={`Preference position ${index + 1}`}>
                <span>
                  <button className="button ghost" disabled={index === 0} onClick={() => {
                    const order = [...draft.providerPriority]
                    ;[order[index - 1], order[index]] = [order[index], order[index - 1]]
                    patchDraft({ providerPriority: order })
                  }}>Up</button>
                  <button className="button ghost" disabled={index === draft.providerPriority.length - 1} onClick={() => {
                    const order = [...draft.providerPriority]
                    ;[order[index + 1], order[index]] = [order[index], order[index + 1]]
                    patchDraft({ providerPriority: order })
                  }}>Down</button>
                </span>
              </SettingRow>
            ))}
          </Panel>
          <Panel title="Per-workload pins (Manual mode)" icon={ListChecks}>
            {state.workloads.map((workload) => {
              const setting = draft.workloadSettings.find((item) => item.workloadId === workload.id)
              return (
                <SettingRow key={workload.id} title={workload.title} description="Honoured when the execution mode is Manual.">
                  <span className="gpu-pin-controls">
                    <select
                      value={setting?.mode ?? 'intelligentAuto'}
                      onChange={(e) => patchDraft({
                        workloadSettings: upsertSetting(draft.workloadSettings, workload.id, { mode: e.target.value as AccelerationMode }),
                      })}
                    >
                      {MODES.map((mode) => <option key={mode.value} value={mode.value}>{mode.label}</option>)}
                    </select>
                    <select
                      value={setting?.backend ?? 'cpu'}
                      onChange={(e) => patchDraft({
                        workloadSettings: upsertSetting(draft.workloadSettings, workload.id, { backend: e.target.value as BackendKind }),
                      })}
                    >
                      <option value="cpu">CPU</option>
                      <option value="cuda">CUDA</option>
                      <option value="wgpu">wgpu</option>
                    </select>
                    <Toggle
                      checked={setting?.enabled ?? true}
                      onChange={(checked) => patchDraft({
                        workloadSettings: upsertSetting(draft.workloadSettings, workload.id, { enabled: checked }),
                      })}
                    />
                  </span>
                </SettingRow>
              )
            })}
          </Panel>
        </TabsContent>

        <TabsContent value="tasks">
          <Panel title="Active tasks" icon={ListChecks} action={
            <button className="button ghost" onClick={() => void clearFinished()}>
              <Trash2 size={14} /> Clear finished
            </button>
          }>
            <TaskMonitorTable tasks={state.tasks} onCancel={(id) => void cancel(id)} />
          </Panel>
          <Panel title="Task history (SQLite)" icon={ListChecks} action={
            <button className="button ghost" onClick={() => void loadHistory()}>
              <RefreshCw size={14} /> Reload
            </button>
          }>
            <TaskMonitorTable tasks={history} onCancel={() => undefined} />
          </Panel>
        </TabsContent>

        <TabsContent value="benchmarks">
          <Panel title="Run a CPU-vs-GPU benchmark" icon={Gauge} action={
            <button
              className="button primary"
              disabled={benchmarkBusy}
              onClick={() => void runBenchmark({
                workloadId: benchWorkload,
                params: paramsFor(benchWorkload, benchSize),
                warmupRuns: warmup,
                measuredRuns: runs,
                validate: true,
              })}
            >
              <Sparkles size={14} /> {benchmarkBusy ? 'Benchmarking…' : 'Run benchmark'}
            </button>
          }>
            <div className="gpu-config-grid">
              <label>Workload
                <select value={benchWorkload} onChange={(e) => { setBenchWorkload(e.target.value); setBenchSize(state.workloads.find((w) => w.id === e.target.value)?.sizes[0] ?? 1_048_576) }}>
                  {state.workloads.map((workload) => <option key={workload.id} value={workload.id}>{workload.title}</option>)}
                </select>
              </label>
              <label>Size
                <select value={benchSize} onChange={(e) => setBenchSize(Number(e.target.value))}>
                  {benchSizes.map((size) => <option key={size} value={size}>{size.toLocaleString()}</option>)}
                </select>
              </label>
              <label>Warm-up runs<input type="number" min={0} max={10} value={warmup} onChange={(e) => setWarmup(Number(e.target.value))} /></label>
              <label>Measured runs<input type="number" min={1} max={50} value={runs} onChange={(e) => setRuns(Number(e.target.value))} /></label>
            </div>
            <SensorNotice text="Benchmarks use identical inputs on both backends, discard warm-ups and report medians. A slowdown is reported as a slowdown." />
          </Panel>
          <Panel title="Measured comparison" icon={Gauge} action={
            <button className="button ghost" onClick={() => void clearBenchmarks()}>
              <Trash2 size={14} /> Clear history
            </button>
          }>
            <BenchmarkComparisonChart records={benchmarks} />
          </Panel>
        </TabsContent>
      </Tabs>
    </div>
  )
}