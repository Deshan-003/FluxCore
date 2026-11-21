import { describe, expect, it } from 'vitest'
import { ageOf, fmtBytes, fmtRate, metricSeriesKey, pct, statusFor, statusForPercent, titleCase } from './format'

describe('fmtBytes', () => {
  it('renders unavailable sensors as an explicit placeholder, never as zero', () => {
    expect(fmtBytes(null)).toBe('Unavailable')
    expect(fmtBytes(undefined)).toBe('Unavailable')
    expect(fmtBytes(null)).not.toBe('0 B')
  })

  it('formats whole bytes without decimals and larger units with one decimal', () => {
    expect(fmtBytes(0)).toBe('0 B')
    expect(fmtBytes(512)).toBe('512 B')
    expect(fmtBytes(1023)).toBe('1023 B')
    expect(fmtBytes(1536)).toBe('1.5 KB')
    expect(fmtBytes(1024 ** 2)).toBe('1.0 MB')
    expect(fmtBytes(3 * 1024 ** 3)).toBe('3.0 GB')
    expect(fmtBytes(2 * 1024 ** 4)).toBe('2.0 TB')
  })

  it('stops scaling at TB', () => {
    expect(fmtBytes(5 * 1024 ** 5)).toBe('5120.0 TB')
  })
})

describe('fmtRate', () => {
  it('marks missing throughput as unavailable instead of 0 B/s', () => {
    expect(fmtRate(null)).toBe('Unavailable')
    expect(fmtRate(null)).not.toBe('0 B/s')
  })

  it('appends the per-second unit to real rates', () => {
    expect(fmtRate(2048)).toBe('2.0 KB/s')
    expect(fmtRate(0)).toBe('0 B/s')
  })
})

describe('pct', () => {
  it('shows an em dash when the collector returned no percentage', () => {
    expect(pct(null)).toBe('—')
    expect(pct(undefined)).toBe('—')
    expect(pct(null)).not.toBe('0%')
  })

  it('rounds real percentages to whole numbers', () => {
    expect(pct(12.4)).toBe('12%')
    expect(pct(12.6)).toBe('13%')
    expect(pct(0)).toBe('0%')
  })
})

describe('statusFor', () => {
  it('reports unknown for unavailable readings and never healthy-by-default', () => {
    expect(statusFor(null)).toBe('unknown')
    expect(statusFor(null)).not.toBe('healthy')
  })

  it('applies the documented pressure thresholds', () => {
    expect(statusFor(0)).toBe('healthy')
    expect(statusFor(74.9)).toBe('healthy')
    expect(statusFor(75)).toBe('elevated')
    expect(statusFor(89.9)).toBe('elevated')
    expect(statusFor(90)).toBe('critical')
    expect(statusFor(100)).toBe('critical')
  })
})

describe('titleCase', () => {
  it('maps navigation labels to page headings', () => {
    expect(titleCase('Overview')).toBe('System overview')
    expect(titleCase('Hardware')).toBe('Hardware information')
    expect(titleCase('Settings')).toBe('Settings & preferences')
    expect(titleCase('Memory Intelligence')).toBe('Memory intelligence')
    expect(titleCase('Optimization Center')).toBe('Optimization center')
    expect(titleCase('CPU')).toBe('CPU monitor')
    expect(titleCase('Storage')).toBe('Storage monitor')
  })
})

describe('metricSeriesKey', () => {
  it('charts dedicated VRAM bytes for the VRAM tile, never the system-RAM series', () => {
    expect(metricSeriesKey('VRAM')).toBe('vramUsedBytes')
    expect(metricSeriesKey('VRAM')).not.toBe('memoryPercent')
    expect(metricSeriesKey('Memory')).toBe('memoryPercent')
  })

  it('maps CPU and GPU tiles to their own series', () => {
    expect(metricSeriesKey('CPU')).toBe('cpuPercent')
    expect(metricSeriesKey('GPU')).toBe('gpuPercent')
  })
})

describe('statusForPercent (redesign status language)', () => {
  it('maps documented thresholds: healthy < 30, normal < 60, attention < 85, critical >= 85', () => {
    expect(statusForPercent(10)).toBe('healthy')
    expect(statusForPercent(45)).toBe('normal')
    expect(statusForPercent(72)).toBe('attention')
    expect(statusForPercent(91)).toBe('critical')
    expect(statusForPercent(85)).toBe('critical')
    expect(statusForPercent(60)).toBe('attention')
  })

  it('never turns an unavailable sensor into a health verdict', () => {
    expect(statusForPercent(null)).toBe('unavailable')
    expect(statusForPercent(undefined)).toBe('unavailable')
  })
})

describe('ageOf', () => {
  it('stays human and truthful about data age', () => {
    const now = Date.parse('2026-11-21T12:00:00Z')
    expect(ageOf('2026-11-21T11:59:58Z', now)).toBe('just now')
    expect(ageOf('2026-11-21T11:59:40Z', now)).toBe('20s ago')
    expect(ageOf('2026-11-21T11:57:00Z', now)).toBe('3m ago')
  })
})
