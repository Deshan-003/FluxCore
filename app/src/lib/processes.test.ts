import { describe, expect, it } from 'vitest'
import { filterProcesses } from './processes'
import type { ProcessMetrics } from '../types/monitoring'

function proc(pid: number, name: string, cpuPercent: number | null, memoryBytes: number): ProcessMetrics {
  return {
    pid,
    name,
    cpuPercent,
    memoryBytes,
    memoryPercent: null,
    gpuPercent: null,
    gpuMemoryBytes: null,
    startedAt: null,
  }
}

const chrome = proc(1000, 'chrome.exe', 12.5, 500 * 1024 ** 2)
const code = proc(2000, 'Code.exe', 3.2, 900 * 1024 ** 2)
const svchost = proc(800, 'svchost.exe', null, 40 * 1024 ** 2)
const explorer = proc(3000, 'explorer.exe', 0.4, 120 * 1024 ** 2)
const all = [chrome, code, svchost, explorer]

describe('filterProcesses search', () => {
  it('returns every process for an empty query', () => {
    expect(filterProcesses(all, '', 'cpu')).toHaveLength(4)
  })

  it('matches process names case-insensitively', () => {
    expect(filterProcesses(all, 'CHROME', 'cpu').map((p) => p.pid)).toEqual([1000])
    expect(filterProcesses(all, 'code', 'cpu').map((p) => p.pid)).toEqual([2000])
  })

  it('matches by PID substring', () => {
    expect(filterProcesses(all, '300', 'cpu').map((p) => p.pid)).toEqual([3000])
    expect(filterProcesses(all, '800', 'cpu').map((p) => p.pid)).toEqual([800])
  })

  it('returns an empty list when nothing matches', () => {
    expect(filterProcesses(all, 'nonexistent-process', 'cpu')).toEqual([])
  })
})

describe('filterProcesses sorting', () => {
  it('sorts by CPU descending and pushes unavailable readings last', () => {
    expect(filterProcesses(all, '', 'cpu').map((p) => p.name))
      .toEqual(['chrome.exe', 'Code.exe', 'explorer.exe', 'svchost.exe'])
  })

  it('sorts by resident memory descending', () => {
    expect(filterProcesses(all, '', 'memory').map((p) => p.name))
      .toEqual(['Code.exe', 'chrome.exe', 'explorer.exe', 'svchost.exe'])
  })

  it('does not mutate the input array', () => {
    const input = [...all]
    filterProcesses(input, '', 'memory')
    expect(input).toEqual(all)
  })
})

describe('filterProcesses limit', () => {
  it('caps the visible rows at 100 by default', () => {
    const many = Array.from({ length: 250 }, (_, index) => proc(index + 1, `proc-${index}.exe`, 1, index))
    expect(filterProcesses(many, '', 'cpu')).toHaveLength(100)
  })

  it('accepts an explicit limit', () => {
    expect(filterProcesses(all, '', 'cpu', 2)).toHaveLength(2)
  })
})
