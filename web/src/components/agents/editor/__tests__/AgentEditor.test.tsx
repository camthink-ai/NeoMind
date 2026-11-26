import { describe, expect, it, vi, beforeEach } from 'vitest'
import { fireEvent, render, screen, waitFor } from '@testing-library/react'
import type { AgentSpec } from '@/types'
import { AgentEditor } from '../AgentEditor'
import { specToDraft } from '../defaults'

const mockTestAgentPreview = vi.fn()

vi.mock('@/hooks/useMobile', () => ({
  useIsMobile: () => false,
  useSafeAreaInsets: () => ({ top: 0, bottom: 0, left: 0, right: 0 }),
}))

vi.mock('@/hooks/use-toast', () => ({
  useToast: () => ({ toast: vi.fn() }),
}))

vi.mock('@/lib/api', () => ({
  api: {
    listLlmBackends: vi.fn(async () => ({ backends: [] })),
    getDevices: vi.fn(async () => ({ devices: [], count: 0 })),
    getDeviceTypes: vi.fn(async () => ({ device_types: [], count: 0 })),
    listExtensions: vi.fn(async () => []),
    listAgents: vi.fn(async () => ({ agents: [], count: 0 })),
    getAgentTools: vi.fn(async () => ({ tools: [], count: 0 })),
    testAgentPreview: (...args: unknown[]) => mockTestAgentPreview(...args),
  },
}))

vi.mock('@/lib/api/agents', () => ({
  agentsApi: {
    planPreview: vi.fn(async () => ({
      effective_plan: {
        agent_id: 'preview',
        config_version: 0,
        trigger_summary: 'manual',
        authorized_reads: [],
        tool_policy: 'disabled',
        notify: { channels: [], on_failure: true },
        controlled_actions: [],
        warnings: [],
        external_read_counts: { weather: 0, tariff: 0, other: 0 },
      },
      dry_run: false,
    })),
  },
}))

vi.mock('@/lib/api/channels', () => ({
  channelsApi: {
    listMessageChannels: vi.fn(async () => ({ channels: [] })),
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

describe('AgentEditor unified BuilderShell UX', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    mockTestAgentPreview.mockResolvedValue({
      dry_run: true,
      execution: {
        status: 'Completed',
        conclusion: 'Preview passed: all sensors healthy.',
        structured_result: { status: 'normal' },
        evidence: { items: [{ key: 'k1' }] },
      },
    })
  })

  it('renders BuilderShell left config rail and 3-step unified workspace immediately on create', () => {
    const { baseElement } = render(
      <AgentEditor open onOpenChange={vi.fn()} onSave={vi.fn()} />,
    )

    // Left config rail
    expect(baseElement.querySelector('[data-section="templates"]')).toBeTruthy()
    expect(baseElement.querySelector('[data-section="model-picker"]')).toBeTruthy()
    expect(baseElement.querySelector('[data-section="plan-preview"]')).toBeTruthy()

    // Right 3-step workspace (zero Simple-vs-Advanced duplication)
    expect(baseElement.querySelector('[data-section="basics"]')).toBeTruthy()
    expect(baseElement.querySelector('[data-section="sources"]')).toBeTruthy()
    expect(baseElement.querySelector('[data-section="actions-output"]')).toBeTruthy()
    expect(baseElement.querySelector('[data-section="tool-policy"]')).toBeTruthy()
    expect(baseElement.querySelector('[data-section="controlled-actions"]')).toBeTruthy()
    expect(baseElement.querySelector('[data-section="output"]')).toBeTruthy()
    expect(baseElement.querySelector('[data-section="schedule-notify"]')).toBeTruthy()
    expect(baseElement.querySelector('[data-section="when"]')).toBeTruthy()
    expect(baseElement.querySelector('[data-section="pacing"]')).toBeTruthy()
    expect(baseElement.querySelector('[data-section="notify"]')).toBeTruthy()
  })

  it('applies template prompt/name when empty, preserves custom text, and supports pre-save Dry Run', async () => {
    render(<AgentEditor open onOpenChange={vi.fn()} onSave={vi.fn()} />)

    const saveBtn = screen.getByTestId('agent-editor-save')
    expect(saveBtn).toBeDisabled()

    // Clicking a template seeds localized name + prompt immediately
    fireEvent.click(screen.getByRole('button', { name: 'agents:editorV2.templates.daily_brief.name' }))
    const nameInput = screen.getByPlaceholderText('agents:editorV2.namePlaceholder') as HTMLInputElement
    const promptInput = screen.getByPlaceholderText('agents:editorV2.promptPlaceholder') as HTMLTextAreaElement
    expect(nameInput.value).toBe('agents:editorV2.templates.daily_brief.name')
    expect(promptInput.value).toBe('agents:creator.preset.scheduled.prompt')
    expect(saveBtn).not.toBeDisabled()

    // Customizing name/prompt preserves them when switching templates
    fireEvent.change(nameInput, { target: { value: 'My Custom Agent' } })
    fireEvent.change(promptInput, { target: { value: 'Custom prompt check.' } })
    fireEvent.click(screen.getByRole('button', { name: 'agents:editorV2.templates.meter_inspect.name' }))
    expect(nameInput.value).toBe('My Custom Agent')
    expect(promptInput.value).toBe('Custom prompt check.')

    // Dry Run runs pre-save preview with full draft and displays result
    fireEvent.click(screen.getByTestId('agent-editor-dry-run'))
    await waitFor(() => {
      expect(mockTestAgentPreview).toHaveBeenCalledTimes(1)
      expect(screen.getByText('Preview passed: all sensors healthy.')).toBeTruthy()
    })
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

  it('opens SourcePickerDialog above FullScreenDialog (z-[110]) and guards Escape from closing editor', async () => {
    const onOpenChange = vi.fn()
    render(<AgentEditor open onOpenChange={onOpenChange} onSave={vi.fn()} />)

    fireEvent.click(screen.getByRole('button', { name: 'editorV2.picker.addSource' }))

    const pickerTitle = await screen.findByText('editorV2.picker.title')
    const dialogEl = pickerTitle.closest('[role="dialog"]')
    expect(dialogEl).toBeTruthy()
    expect(dialogEl?.className).toContain('z-[110]')

    // Pressing Escape while picker is open closes only the picker, not AgentEditor
    fireEvent.keyDown(window, { key: 'Escape' })
    expect(onOpenChange).not.toHaveBeenCalled()
    await waitFor(() => {
      expect(screen.queryByText('editorV2.picker.title')).toBeNull()
    })
  })
})
