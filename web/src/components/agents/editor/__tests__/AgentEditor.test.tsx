import { describe, expect, it, vi } from 'vitest'
import { fireEvent, render, screen } from '@testing-library/react'
import { AgentEditor } from '../AgentEditor'

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
})
