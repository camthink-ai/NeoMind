/// Rendering smoke tests for the two large automation builders — the highest
/// regression-cost forms in the app (user data entry). These assert the
/// builders mount cleanly with minimal props and expose their primary
/// affordances; deep field-level behavior stays with manual QA.
import { describe, it, expect, vi } from 'vitest'
import { render, screen, fireEvent, waitFor } from '@testing-library/react'
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

  it('edit mode: prefill restores state and save carries id + updated name', async () => {
    // Prefill is the only Radix-free path to a fully-valid form (the create
    // default's data_change trigger requires a condition built through
    // Selects, which the FullScreenDialog focus trap keeps closed in jsdom).
    // This pins the EDIT save contract: id preserved, trigger restored,
    // name editable, actions composed.
    const onSave = vi.fn().mockResolvedValue(undefined)
    render(
      <SimpleRuleBuilderSplit
        open
        onOpenChange={noop}
        onSave={onSave}
        rule={{
          id: 'r-1',
          name: '旧名称',
          enabled: true,
          trigger: { trigger_type: 'manual' },
          actions: [{ type: 'notify', message: 'Rule triggered', severity: 'info' }],
        } as never}
        resources={{
          devices: [],
          deviceTypes: [],
          extensions: [],
          extensionDataSources: [],
          transformDataSources: [],
          messageChannels: [],
        }}
      />,
    )
    const overlay = document.querySelector('.fixed.inset-0') as HTMLElement
    const nameInput = overlay.querySelector('#rule-name') as HTMLInputElement
    await waitFor(() => expect(nameInput.value).toBe('旧名称')) // restore ran
    fireEvent.change(nameInput, { target: { value: '高温告警' } })

    fireEvent.click(screen.getByRole('button', { name: /save/ }))
    await waitFor(() => expect(onSave).toHaveBeenCalledTimes(1))
    const rule = onSave.mock.calls[0][0]
    expect(rule.id).toBe('r-1') // edit, not accidental create
    expect(rule.name).toBe('高温告警')
    expect(rule.trigger).toEqual({ trigger_type: 'manual' })
    expect(rule.actions).toEqual([
      { type: 'notify', message: 'Rule triggered', severity: 'info' },
    ])
  })

  it('an action this editor does not know survives an edit and save', async () => {
    // The editor used to fold every unrecognised action type onto a hardcoded
    // notify ("Rule triggered"). A rule created through the API/CLI — e.g. one
    // carrying the M2-4 `run_operator` action, which this builder has no form
    // for — was therefore rewritten the moment it was opened, and saving
    // destroyed the original action. Preserve what we cannot render.
    const onSave = vi.fn().mockResolvedValue(undefined)
    const operatorAction = { type: 'run_operator', agent_id: 'cam01-view' }
    render(
      <SimpleRuleBuilderSplit
        open
        onOpenChange={noop}
        onSave={onSave}
        rule={{
          id: 'r-9',
          name: '运行算子',
          enabled: true,
          trigger: { trigger_type: 'manual' },
          actions: [operatorAction],
        } as never}
        resources={{
          devices: [],
          deviceTypes: [],
          extensions: [],
          extensionDataSources: [],
          transformDataSources: [],
          messageChannels: [],
        }}
      />,
    )
    const overlay = document.querySelector('.fixed.inset-0') as HTMLElement
    const nameInput = overlay.querySelector('#rule-name') as HTMLInputElement
    await waitFor(() => expect(nameInput.value).toBe('运行算子')) // restore ran

    fireEvent.click(screen.getByRole('button', { name: /save/ }))
    await waitFor(() => expect(onSave).toHaveBeenCalledTimes(1))

    expect(onSave.mock.calls[0][0].actions).toEqual([operatorAction])
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
