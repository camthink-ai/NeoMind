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
        if (k === 'commandDelivery.summaryAdapter') return 'Sent. The device has not confirmed.'
        if (k === 'commandDelivery.summaryExtension') return 'Extension received it. The device has not confirmed.'
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
    expect(screen.getByText('Sent. The device has not confirmed.')).toBeTruthy()
    expect(screen.queryByText(/broker publish/i)).toBeNull()
    expect(container.querySelector('[data-tier]')).toBeNull()
  })

  it('marks extension path without claiming a device confirmation', () => {
    const { container } = render(
      <CommandDeliveryLadderPanel result={{ sent: true, delivery: 'extension_accepted' }} />,
    )
    expect(container.querySelector('[data-delivery-kind="extension_accepted"]')).toBeTruthy()
    expect(container.querySelector('[data-verified="false"]')).toBeTruthy()
    expect(screen.getByText('Extension received it. The device has not confirmed.')).toBeTruthy()
  })
})
