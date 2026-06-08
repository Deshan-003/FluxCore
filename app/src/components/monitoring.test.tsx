import { describe, expect, it, vi } from 'vitest'
import { fireEvent, render, screen } from '@testing-library/react'
import { Cpu } from 'lucide-react'
import {
  Detail, EmptyState, GpuMetric, Panel, ResourceBar, SensorNotice, SettingRow, Toggle,
} from './monitoring'

describe('EmptyState', () => {
  it('renders the empty-state title and detail', () => {
    render(<EmptyState title="No mounted volumes" detail="No volumes were reported by Windows." />)
    expect(screen.getByText('No mounted volumes')).toBeTruthy()
    expect(screen.getByText('No volumes were reported by Windows.')).toBeTruthy()
  })
})

describe('SensorNotice', () => {
  it('renders unsupported-sensor explanations verbatim', () => {
    const text = 'CPU temperature requires a supported hardware sensor; no value is inferred.'
    render(<SensorNotice text={text} />)
    expect(screen.getByText(text)).toBeTruthy()
  })
})

describe('ResourceBar', () => {
  it('renders an empty bar when the percentage is unavailable', () => {
    const { container } = render(
      <ResourceBar label="Physical memory" value="Unavailable" total="Unavailable" percent={null} />,
    )
    const fill = container.querySelector('.bar-fill') as HTMLElement
    expect(fill.style.width).toBe('0%')
    expect(screen.getByText('Physical memory')).toBeTruthy()
  })

  it('clamps the fill width into the 0-100% range', () => {
    const { container } = render(
      <ResourceBar label="In use" value="8 GB" total="8 GB" percent={150} />,
    )
    expect((container.querySelector('.bar-fill') as HTMLElement).style.width).toBe('100%')
  })

  it('renders the real percentage as width', () => {
    const { container } = render(
      <ResourceBar label="In use" value="4 GB" total="8 GB" percent={50} color="violet" />,
    )
    const fill = container.querySelector('.bar-fill') as HTMLElement
    expect(fill.style.width).toBe('50%')
    expect(fill.className).toContain('violet')
  })
})

describe('Detail and GpuMetric', () => {
  it('render label/value pairs and keep Unavailable values visible', () => {
    render(<Detail label="Temperature" value="Unavailable · no supported sensor" />)
    expect(screen.getByText('Temperature')).toBeTruthy()
    expect(screen.getByText('Unavailable · no supported sensor')).toBeTruthy()

    render(<GpuMetric label="Driver version" value="555.99" />)
    expect(screen.getByText('Driver version')).toBeTruthy()
    expect(screen.getByText('555.99')).toBeTruthy()
  })
})

describe('Panel', () => {
  it('renders the heading, optional action and children', () => {
    render(
      <Panel title="Processor details" icon={Cpu} action={<button>Refresh</button>}>
        <span>panel body</span>
      </Panel>,
    )
    expect(screen.getByRole('heading', { name: 'Processor details' })).toBeTruthy()
    expect(screen.getByRole('button', { name: 'Refresh' })).toBeTruthy()
    expect(screen.getByText('panel body')).toBeTruthy()
  })
})

describe('Toggle', () => {
  it('exposes switch semantics and flips on click', () => {
    const onChange = vi.fn()
    render(<Toggle checked={false} onChange={onChange} />)
    const toggle = screen.getByRole('switch')
    expect(toggle.getAttribute('aria-checked')).toBe('false')
    fireEvent.click(toggle)
    expect(onChange).toHaveBeenCalledWith(true)
  })
})

describe('SettingRow', () => {
  it('renders the preference title, description and control', () => {
    render(
      <SettingRow title="Enable monitoring" description="Pause collection while keeping the application open.">
        <Toggle checked onChange={() => {}} />
      </SettingRow>,
    )
    expect(screen.getByText('Enable monitoring')).toBeTruthy()
    expect(screen.getByText('Pause collection while keeping the application open.')).toBeTruthy()
    expect(screen.getByRole('switch').getAttribute('aria-checked')).toBe('true')
  })
})
