/**
 * NotifyPolicySection — three-way flags + channels update the v2 NotifyPolicy.
 */
import { describe, expect, it, vi } from 'vitest'
import { render, screen, fireEvent } from '@testing-library/react'
import { NotifyPolicySection } from '../NotifyPolicySection'
import type { NotifyPolicy } from '@/types'

vi.mock('react-i18next', async (importOriginal) => {
  const actual = await importOriginal<typeof import('react-i18next')>()
  return {
    ...actual,
    useTranslation: () => ({
      t: (key: string) => {
        const map: Record<string, string> = {
          'editorV2.notify.title': 'Notify',
          'editorV2.notify.channels': 'Channels',
          'editorV2.notify.channelsHint': 'Comma-separated channel ids',
          'editorV2.notify.onComplete': 'On complete',
          'editorV2.notify.onCompleteDesc': 'Notify when a run finishes successfully',
          'editorV2.notify.onAlert': 'On alert',
          'editorV2.notify.onAlertDesc': 'Notify when an alert condition fires',
          'editorV2.notify.onFailure': 'On failure',
          'editorV2.notify.onFailureDesc': 'Notify when a run fails',
          'editorV2.notify.pendingConfirmation': 'Pending confirmation',
          'editorV2.notify.pendingConfirmationDesc': 'Judgment / confirm before send',
        }
        return map[key] ?? key
      },
      i18n: { language: 'en' },
    }),
  }
})

const base: NotifyPolicy = {
  channels: ['test-channel'],
  on_complete: true,
  on_alert: false,
  alert_depends_on: [],
  on_failure: true,
  pending_confirmation: false,
}

describe('NotifyPolicySection', () => {
  it('renders on_complete / on_alert / on_failure switches', () => {
    render(<NotifyPolicySection value={base} onChange={vi.fn()} />)
    expect(screen.getByLabelText('On complete')).toBeTruthy()
    expect(screen.getByLabelText('On alert')).toBeTruthy()
    expect(screen.getByLabelText('On failure')).toBeTruthy()
    expect(screen.getByTestId('notify-channels')).toHaveProperty('value', 'test-channel')
  })

  it('toggles on_alert and reports the updated NotifyPolicy', () => {
    const onChange = vi.fn()
    render(<NotifyPolicySection value={base} onChange={onChange} />)
    fireEvent.click(screen.getByLabelText('On alert'))
    expect(onChange).toHaveBeenCalledOnce()
    const next = onChange.mock.calls[0][0] as NotifyPolicy
    expect(next.on_alert).toBe(true)
    expect(next.on_complete).toBe(true)
    expect(next.on_failure).toBe(true)
    expect(next.channels).toEqual(['test-channel'])
  })

  it('parses comma-separated channels', () => {
    const onChange = vi.fn()
    render(<NotifyPolicySection value={base} onChange={onChange} />)
    fireEvent.change(screen.getByTestId('notify-channels'), {
      target: { value: 'ops, alerts' },
    })
    expect(onChange).toHaveBeenCalledOnce()
    expect((onChange.mock.calls[0][0] as NotifyPolicy).channels).toEqual(['ops', 'alerts'])
  })
})
