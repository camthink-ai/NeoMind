import { describe, expect, it, vi } from 'vitest'
import { fireEvent, render, screen } from '@testing-library/react'
import type { AgentSpec } from '@/types'
import { AgentEditor } from '../AgentEditor'
import { specToDraft } from '../defaults'

vi.mock('@/hooks/useMobile', () => ({
  useIsMobile: () => false,
  useSafeAreaInsets: () => ({ top: 0, bottom: 0, left: 0, right: 0 }),
}))

vi.mock('@/lib/api', () => ({
  api: {
    listLlmBackends: vi.fn(async () => ({ backends: [] })),
    getDevices: vi.fn(async () => ({ devices: [] })),
    listExtensions: vi.fn(async () => ({ extensions: [] })),
    listChannels: vi.fn(async () => ({ channels: [] })),
  },
}))

vi.mock('react-i18next', async (importOriginal) => {
  const actual = await importOriginal<typeof import('react-i18next')>()
  return {
    ...actual,
    useTranslation: () => ({
      t: (key: string, opts?: string | Record<string, unknown>) => {
        if (typeof opts === 'string') return opts
        if (opts && typeof opts === 'object' && typeof opts.defaultValue === 'string') {
          return opts.defaultValue
        }
        return key
      },
      i18n: { language: 'en' },
    }),
  }
})

describe('AgentEditor create UX', () => {
  it('shows the form immediately below Task type even before a template is selected', () => {
    const { baseElement } = render(
      <AgentEditor open onOpenChange={vi.fn()} onSave={vi.fn()} />,
    )

    expect(baseElement.querySelector('[data-section="templates"]')).toBeTruthy()
    expect(baseElement.querySelector('[data-section="basics"]')).toBeTruthy()
    expect(baseElement.querySelector('[data-section="when"]')).toBeTruthy()
    expect(baseElement.querySelector('[data-section="sources"]')).toBeTruthy()
    expect(baseElement.querySelector('[data-section="output"]')).toBeTruthy()
    expect(baseElement.querySelector('[data-section="notify"]')).toBeTruthy()
  })

  it('allows saving once name and prompt are filled without requiring a template click, and preserves them when applying a template', () => {
    render(<AgentEditor open onOpenChange={vi.fn()} onSave={vi.fn()} />)

    const saveBtn = screen.getByTestId('agent-editor-save')
    expect(saveBtn).toBeDisabled()

    const nameInput = screen.getByPlaceholderText('agents:editorV2.namePlaceholder')
    const promptInput = screen.getByPlaceholderText('agents:editorV2.promptPlaceholder')
    fireEvent.change(nameInput, { target: { value: 'My Monitor' } })
    fireEvent.change(promptInput, { target: { value: 'Check sensors every morning.' } })

    expect(saveBtn).not.toBeDisabled()

    // Clicking a template applies its defaults while preserving typed name/prompt
    fireEvent.click(screen.getByRole('button', { name: 'agents:editorV2.templates.daily_brief.name' }))
    expect((nameInput as HTMLInputElement).value).toBe('My Monitor')
    expect((promptInput as HTMLTextAreaElement).value).toBe('Check sensors every morning.')
  })

  it('edits an old/legacy-shaped agent without trigger field without crashing', () => {
    const legacyAgent = {
      id: 'legacy-1',
      name: 'Legacy Sensor Agent',
      user_prompt: 'Watch sensor temperature',
      schedule: {
        schedule_type: 'interval',
        interval_seconds: 600,
      },
      resources: [
        {
          resource_type: 'metric',
          resource_id: 'dev-1:temperature',
          name: 'Temperature',
          config: {},
        },
      ],
      output_schema: [{ name: 'status', field_type: { type: 'text' }, description: 'Status' }],
      tool_config: { enabled: true, allowed_tools: ['shell'] },
      notify: { channels: ['IM'], on: 'always' },
    } as unknown as AgentSpec

    const draft = specToDraft(legacyAgent)
    expect(draft.trigger).toEqual({ kind: 'interval', every_secs: 600 })
    expect(draft.tool_policy).toEqual({ scoped: { allowed_tools: ['shell'], allowed_commands: [] } })
    expect(draft.input.requirements).toHaveLength(1)
    expect(draft.output?.fields.map((f) => f.name)).toEqual(['status'])
    expect(draft.notify?.on_complete).toBe(true)

    render(
      <AgentEditor open agent={legacyAgent} onOpenChange={vi.fn()} onSave={vi.fn()} />,
    )
    expect(screen.getByDisplayValue('Legacy Sensor Agent')).toBeTruthy()
    expect(screen.getByTestId('agent-editor-save')).not.toBeDisabled()
  })

  it('handles missing/undefined trigger, tool_policy, and input safely on edit', () => {
    const partialAgent = {
      id: 'partial-1',
      name: 'Partial Agent',
      user_prompt: 'Do something',
      trigger: undefined,
      tool_policy: undefined,
      input: undefined,
      output: undefined,
      notify: undefined,
    } as unknown as AgentSpec

    render(
      <AgentEditor open agent={partialAgent} onOpenChange={vi.fn()} onSave={vi.fn()} />,
    )
    expect(screen.getByDisplayValue('Partial Agent')).toBeTruthy()
    expect(screen.getByTestId('agent-editor-save')).not.toBeDisabled()
  })
})
