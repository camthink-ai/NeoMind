/// Rendering smoke tests for the two large automation builders — the highest
/// regression-cost forms in the app (user data entry). These assert the
/// builders mount cleanly with minimal props and expose their primary
/// affordances; deep field-level behavior stays with manual QA.
import { describe, it, expect } from 'vitest'
import { render, screen } from '@testing-library/react'
import { SimpleRuleBuilderSplit } from '../SimpleRuleBuilderSplit'
import { TransformBuilder as TransformBuilderSplit } from '../TransformBuilderSplit'

const noop = () => {}
const noopAsync = async () => {}

describe('SimpleRuleBuilderSplit', () => {
  it('renders with the create-rule affordances when open', () => {
    render(
      <SimpleRuleBuilderSplit
        open
        onOpenChange={noop}
        onSave={noopAsync}
        resources={{
          devices: [
            {
              id: 'dev-1',
              name: '温度传感器',
              device_type: 'sensor',
              metrics: [{ name: 'temperature', data_type: 'number', unit: '°C' }],
              commands: [],
              online: true,
            },
          ],
          deviceTypes: [],
          extensions: [],
          extensionDataSources: [],
          transformDataSources: [],
          messageChannels: [],
        }}
      />,
    )
    // FullScreenDialog portals a fixed overlay with an h1 title; without an
    // i18n provider t() returns the key — assert on the key text.
    const heading = screen.getByRole('heading', { level: 1 })
    expect(heading.textContent).toContain('newRule')
  })

  it('renders nothing visible when closed', () => {
    render(<SimpleRuleBuilderSplit open={false} onOpenChange={noop} onSave={noopAsync} />)
    const overlay = document.querySelector('.fixed.inset-0')
    expect(overlay).toBeTruthy() // portal exists
    expect(overlay!.classList.contains('hidden')).toBe(true) // but is hidden
  })
})

describe('TransformBuilderSplit', () => {
  const devices = [{ id: 'dev-1', name: '温度传感器', device_type: 'sensor' }]

  it('renders with the transform form when open', () => {
    render(
      <TransformBuilderSplit
        open
        onOpenChange={noop}
        transform={null}
        devices={devices}
        onSave={noop}
      />,
    )
    const heading = screen.getByRole('heading', { level: 1 })
    expect(heading.textContent).toContain('title') // tBuilder('title') key fallback
  })

  it('editing an existing transform pre-fills its name', () => {
    render(
      <TransformBuilderSplit
        open
        onOpenChange={noop}
        transform={{
          id: 't1',
          name: '温度均值',
          scope: 'global',
          js_code: 'return 1',
          output_metrics: [],
          enabled: true,
        } as never}
        devices={devices}
        onSave={noop}
      />,
    )
    const overlay = document.querySelector('.fixed.inset-0') as HTMLElement
    const input = overlay.querySelector('input') as HTMLInputElement
    expect(input?.value).toBe('温度均值')
  })
})
