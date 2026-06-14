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
