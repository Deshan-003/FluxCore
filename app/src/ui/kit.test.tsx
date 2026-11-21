import { describe, expect, it } from 'vitest'
import { act, fireEvent, render, screen } from '@testing-library/react'
import { ConfirmDialog, EmptyState, ResourceBar, StatusChip, ToastHost } from './kit'
import { dismissToast, getToasts, subscribeToasts, toast } from './toast-store'
import type { ConfirmSpec } from './kit'

describe('StatusChip', () => {
  it('always pairs the status color with a text label and icon', () => {
    render(<StatusChip status="critical" />)
    const chip = screen.getByText('Critical')
    expect(chip).toBeTruthy()
    expect(chip.parentElement?.querySelector('svg')).toBeTruthy()
  })

  it('accepts a custom label for data-specific wording', () => {
    render(<StatusChip status="attention" label="Needs attention" />)
    expect(screen.getByText('Needs attention')).toBeTruthy()
  })
})

describe('ResourceBar', () => {
  it('renders value and total as text alongside the bar', () => {
    render(<ResourceBar label="In use" value="5.0 GB" total="8.0 GB" percent={62.5} color="var(--rx-c-memory)" />)
    expect(screen.getByText('In use')).toBeTruthy()
    expect(screen.getByText('5.0 GB of 8.0 GB')).toBeTruthy()
    const fill = document.querySelector('.rx-bar span') as HTMLElement
    expect(fill.style.width).toBe('62.5%')
  })

  it('clamps percentages above 100 so bars never overflow', () => {
    render(<ResourceBar label="Committed" value="9 GB" total="8 GB" percent={112.5} color="var(--rx-c-other)" />)
    const fill = document.querySelector('.rx-bar span') as HTMLElement
    expect(fill.style.width).toBe('100%')
  })
})

describe('EmptyState', () => {
  it('explains itself and offers the suggested next action', () => {
    render(<EmptyState title="No GPU detected" detail="Windows did not report a display adapter." action={<button>Open troubleshooting</button>} />)
    expect(screen.getByText('No GPU detected')).toBeTruthy()
    expect(screen.getByText(/Windows did not report/)).toBeTruthy()
    expect(screen.getByText('Open troubleshooting')).toBeTruthy()
  })
})

describe('ConfirmDialog', () => {
  const spec: ConfirmSpec = {
    title: 'Run "Free standby memory"?',
    body: 'Releases cached standby pages back to the system.',
    facts: [
      { kind: 'what', text: 'Releases cached pages.' },
      { kind: 'benefit', text: 'Returns memory to the free pool.' },
      { kind: 'risk', text: 'Low risk — no data loss.' },
    ],
    confirmLabel: 'Run action',
    onConfirm: () => undefined,
    onCancel: () => undefined,
  }

  it('renders nothing when no spec is provided', () => {
    const { container } = render(<ConfirmDialog spec={null} />)
    expect(container.querySelector('.rx-dialog')).toBeNull()
  })

  it('explains what happens, the benefit and the risk before confirming', () => {
    render(<ConfirmDialog spec={spec} />)
    expect(screen.getByText('Run "Free standby memory"?')).toBeTruthy()
    expect(screen.getByText(/What happens:/)).toBeTruthy()
    expect(screen.getByText(/Benefit:/)).toBeTruthy()
    expect(screen.getByText(/Risk:/)).toBeTruthy()
  })

  it('invokes onConfirm and onCancel from the dialog buttons', () => {
    let confirmed = false
    let cancelled = false
    render(<ConfirmDialog spec={{ ...spec, onConfirm: () => { confirmed = true }, onCancel: () => { cancelled = true } }} />)
    fireEvent.click(screen.getByText('Run action'))
    expect(confirmed).toBe(true)
    fireEvent.click(screen.getByText('Cancel'))
    expect(cancelled).toBe(true)
  })

  it('cancels on the Escape key', () => {
    let cancelled = false
    render(<ConfirmDialog spec={{ ...spec, onCancel: () => { cancelled = true } }} />)
    fireEvent.keyDown(window, { key: 'Escape' })
    expect(cancelled).toBe(true)
  })
})

describe('ToastHost', () => {
  it('shows submitted toasts with a dismiss control', () => {
    render(<ToastHost />)
    expect(getToasts().length).toBe(0)
    act(() => { toast('Memory action completed', 'success') })
    expect(screen.getByText('Memory action completed')).toBeTruthy()
    act(() => { fireEvent.click(screen.getByLabelText('Dismiss notification')) })
    expect(getToasts().length).toBe(0)
  })

  it('keeps the subscription stable for external stores', () => {
    const unsubscribe = subscribeToasts(() => undefined)
    expect(typeof unsubscribe).toBe('function')
    unsubscribe()
    dismissToast(999) // must not throw for unknown ids
  })
})
