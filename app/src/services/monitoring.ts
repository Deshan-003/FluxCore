import { invoke, isTauri } from '@tauri-apps/api/core'
import type { HardwareCheckReport, HistoryPoint, MonitoringSettings, SystemSnapshot } from '../types/monitoring'

export const DEFAULT_SETTINGS: MonitoringSettings = {
  refreshIntervalMs: 1000,
  historyDurationMinutes: 10,
  persistHistory: true,
  monitoringEnabled: true,
  startOnLogin: false,
  minimizeToTray: false,
  theme: 'dark',
}

export async function getSnapshot(): Promise<SystemSnapshot> {
  return invoke<SystemSnapshot>('get_snapshot')
}

export async function getSettings(): Promise<MonitoringSettings> {
  return invoke<MonitoringSettings>('get_settings')
}

export async function saveSettings(settings: MonitoringSettings): Promise<void> {
  await invoke('save_settings', { settings })
}

export async function getHistory(): Promise<HistoryPoint[]> {
  return invoke<HistoryPoint[]>('get_history')
}

export async function checkHardware(): Promise<HardwareCheckReport> {
  return invoke<HardwareCheckReport>('check_hardware')
}

export function isDesktopRuntime(): boolean {
  try { return isTauri() } catch { return false }
}
