import { describe, expect, it, vi } from 'vitest'
import { render, screen } from '@testing-library/react'
import { CommandDeliveryLadderPanel } from '../CommandDeliveryLadderPanel'

vi.mock('react-i18next', async (importOriginal) => {
  const actual = await importOriginal<typeof import('react-i18next')>()
  return {
    ...actual,
    useTranslation: () => ({
      t: (key: string, opts?: { count?: number; defaultValue?: string }) => {
        const k = key.replace(/^devices:/, '')
        if (k === 'commandDelivery.summaryAdapter') return 'Adapter accepted (dispatch only)'
        if (k === 'commandDelivery.summaryExtension') return 'Extension accepted (not a device ACK)'
        if (k === 'commandDelivery.noElevateHint') {
          return 'No evidence for device ACK or physical effect — tier not elevated.'
        }
        if (k === 'commandDelivery.adapters' && opts?.count != null) {
          return `${opts.count} adapter(s)`
        }
        if (k.startsWith('commandDelivery.tier.')) return k.slice('commandDelivery.tier.'.length)
        if (k.startsWith('commandDelivery.state.')) return k.slice('commandDelivery.state.'.length)
        return opts?.defaultValue ?? key
      },
      i18n: { language: 'en' },
    }),
  }
})

describe('CommandDeliveryLadderPanel', () => {
  it('shows adapter dispatch without verified green', () => {
    const { container } = render(
      <CommandDeliveryLadderPanel
        result={{
          sent: true,
          delivery: { adapter_accepted: { adapters: 1 } },
          delivery_note: 'broker publish; device acknowledgment: none',
        }}
      />,
    )
    expect(container.querySelector('[data-delivery-kind="adapter_accepted"]')).toBeTruthy()
    expect(container.querySelector('[data-verified="false"]')).toBeTruthy()
    expect(container.querySelector('[data-tier="device_ack"][data-tier-state="not_reached"]')).toBeTruthy()
    expect(screen.getByText(/Adapter accepted/i)).toBeTruthy()
    expect(screen.getByText(/tier not elevated/i)).toBeTruthy()
  })

  it('marks extension path without elevating to device ACK', () => {
    const { container } = render(
      <CommandDeliveryLadderPanel result={{ sent: true, delivery: 'extension_accepted' }} />,
    )
    expect(container.querySelector('[data-delivery-kind="extension_accepted"]')).toBeTruthy()
    expect(container.querySelector('[data-tier="device_ack"][data-tier-state="not_reached"]')).toBeTruthy()
    expect(container.querySelector('[data-verified="false"]')).toBeTruthy()
  })
})
