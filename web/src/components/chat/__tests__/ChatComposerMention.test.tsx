/// Interaction tests for the composer's structured @-mention picker
/// (iteration 3): trigger detection, Chinese filtering, keyboard selection,
/// chip removal, backspace chip deletion, and legacy compatibility when the
/// host passes no reference handlers.
import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, screen, fireEvent, waitFor } from '@testing-library/react'
import { ChatComposer } from '../ChatComposer'

const MENTION_HITS = [
  { type: 'device' as const, id: 'sensor-001', label: '温湿度传感器' },
  { type: 'device' as const, id: 'sensor-002', label: '温湿度传感器备用' },
  { type: 'rule' as const, id: 'rule-1', label: '高温报警' },
  { type: 'transform' as const, id: 't-1', label: '温度均值' },
  { type: 'agent' as const, id: 'a-1', label: '巡检代理' },
]

vi.mock('@/lib/api/mentions', () => ({
  searchMentionOptions: vi.fn(async (q: string) =>
    q ? MENTION_HITS.filter((o) => o.label.includes(q) || o.id.includes(q)) : MENTION_HITS,
  ),
  MAX_REFERENCES: 20,
}))

import { searchMentionOptions } from '@/lib/api/mentions'
const searchMock = vi.mocked(searchMentionOptions)

function setup(overrides: Partial<React.ComponentProps<typeof ChatComposer>> = {}) {
  const onChange = vi.fn()
  const onReferencesChange = vi.fn()
  const refs: import('@/types').ChatReference[] = []
  render(
    <ChatComposer
      value=""
      onChange={onChange}
      onSend={vi.fn()}
      references={refs}
      onReferencesChange={onReferencesChange}
      {...overrides}
    />,
  )
  return { onChange, onReferencesChange }
}

const getTextarea = () => screen.getByRole('textbox') as HTMLTextAreaElement

/** Controlled composer with real state — mirrors how hosts drive it. */
function statefulSetup(opts: { onReferencesChange?: ReturnType<typeof vi.fn> } = {}) {
  const onReferencesChange = opts.onReferencesChange ?? vi.fn()
  let value = ''
  let refs: import('@/types').ChatReference[] = []
  const view = render(
    <ChatComposer
      value={value}
      onChange={(v) => {
        value = v
        rerender()
      }}
      onSend={vi.fn()}
      references={refs}
      onReferencesChange={(r) => {
        refs = r
        onReferencesChange(r)
        rerender()
      }}
    />,
  )
  function rerender() {
    view.rerender(
      <ChatComposer
        value={value}
        onChange={(v) => {
          value = v
          rerender()
        }}
        onSend={vi.fn()}
        references={refs}
        onReferencesChange={(r) => {
          refs = r
          onReferencesChange(r)
          rerender()
        }}
      />,
    )
  }
  return { onReferencesChange, rerenderWith: true }
}

describe('ChatComposer @-mentions', () => {
  beforeEach(() => {
    searchMock.mockClear()
  })

  it('opens the picker after typing @ and lists options', async () => {
    setup()
    const ta = getTextarea()
    fireEvent.change(ta, { target: { value: '帮我看下 @' } })
    // selectionStart defaults to end of typed text in the DOM event
    await waitFor(() => expect(screen.getByRole('listbox')).toBeTruthy())
    await waitFor(() => expect(screen.getAllByRole('option').length).toBeGreaterThan(0))
    expect(searchMock).toHaveBeenCalled()
  })

  it('filters by a Chinese query', async () => {
    setup()
    const ta = getTextarea()
    fireEvent.change(ta, { target: { value: '检查 @温湿度' } })
    await waitFor(() => expect(searchMock).toHaveBeenCalledWith('温湿度'))
    const options = await screen.findAllByRole('option')
    expect(options.length).toBe(2)
    expect(options[0].textContent).toContain('温湿度传感器')
  })

  it('keyboard-selects the active option: strips the trigger text and adds a chip', async () => {
    const onReferencesChange = vi.fn()
    const { rerenderWith } = statefulSetup({ onReferencesChange })
    const ta = getTextarea()
    fireEvent.change(ta, { target: { value: '看看 @温湿度' } })
    const options = await screen.findAllByRole('option')
    expect(options.length).toBeGreaterThan(0)
    fireEvent.keyDown(ta, { key: 'Enter' })
    // The "@温湿度" trigger is stripped from the message text...
    expect(ta.value).toBe('看看 ')
    // ...and the chip state carries the authoritative id.
    expect(onReferencesChange).toHaveBeenCalledWith([
      expect.objectContaining({ type: 'device', id: 'sensor-001' }),
    ])
    expect(rerenderWith).toBeTruthy()
  })

  it('Escape closes the picker without selecting', async () => {
    const { onReferencesChange } = setup({ value: '' })
    const ta = getTextarea()
    fireEvent.change(ta, { target: { value: '@' } })
    await screen.findAllByRole('option')
    fireEvent.keyDown(ta, { key: 'Escape' })
    expect(screen.queryByRole('listbox')).toBeNull()
    expect(onReferencesChange).not.toHaveBeenCalled()
  })

  it('renders chips and removes one via its remove button', () => {
    const onReferencesChange = vi.fn()
    render(
      <ChatComposer
        value=""
        onChange={vi.fn()}
        onSend={vi.fn()}
        references={[
          { type: 'rule', id: 'rule-1', label: '高温报警' },
          { type: 'device', id: 'sensor-002', label: '备用' },
        ]}
        onReferencesChange={onReferencesChange}
      />,
    )
    // Same-name different-type chips both render, distinguished by type.
    // The test setup mocks t() to return i18n keys.
    expect(screen.getAllByLabelText('chat:mention.referencesLabel').length).toBeGreaterThan(0)
    const removeButtons = screen.getAllByLabelText('移除引用')
    expect(removeButtons.length).toBe(2)
    removeButtons[0].click()
    expect(onReferencesChange).toHaveBeenCalledWith([
      { type: 'device', id: 'sensor-002', label: '备用' },
    ])
  })

  it('backspace on an empty input deletes the last chip', () => {
    const onReferencesChange = vi.fn()
    render(
      <ChatComposer
        value=""
        onChange={vi.fn()}
        onSend={vi.fn()}
        references={[{ type: 'agent', id: 'a-1', label: '巡检代理' }]}
        onReferencesChange={onReferencesChange}
      />,
    )
    fireEvent.keyDown(getTextarea(), { key: 'Backspace' })
    expect(onReferencesChange).toHaveBeenCalledWith([])
  })

  it('does not open for email-like text (word char before @)', async () => {
    setup()
    const ta = getTextarea()
    fireEvent.change(ta, { target: { value: 'contact me at a@b.com please' } })
    await new Promise((r) => setTimeout(r, 250))
    expect(screen.queryByRole('listbox')).toBeNull()
    expect(searchMock).not.toHaveBeenCalled()
  })

  it('opens after a space, line start, CJK char, or punctuation', async () => {
    for (const text of ['看看 @温', '@设备', '设备@温', '!!!@温']) {
      const view = render(
        <ChatComposer
          value=""
          onChange={vi.fn()}
          onSend={vi.fn()}
          references={[]}
          onReferencesChange={vi.fn()}
        />,
      )
      const ta = screen.getByRole('textbox') as HTMLTextAreaElement
      fireEvent.change(ta, { target: { value: text } })
      await screen.findAllByRole('listbox')
      view.unmount()
    }
  })

  it('deduplicates a repeated selection by type+id', async () => {
    const onReferencesChange = vi.fn()
    render(
      <ChatComposer
        value=""
        onChange={vi.fn()}
        onSend={vi.fn()}
        references={[{ type: 'device', id: 'sensor-001', label: '温湿度传感器' }]}
        onReferencesChange={onReferencesChange}
      />,
    )
    const ta = getTextarea()
    fireEvent.change(ta, { target: { value: '再选一次 @温湿度' } })
    const options = await screen.findAllByRole('option')
    fireEvent.keyDown(ta, { key: 'Enter' })
    expect(onReferencesChange).not.toHaveBeenCalled()
    expect(screen.queryByRole('listbox')).toBeNull()
    expect(options.length).toBeGreaterThan(0)
  })

  it('blocks the picker entirely at the 20-reference cap', async () => {
    const many = Array.from({ length: 20 }, (_, i) => ({
      type: 'device' as const,
      id: `sensor-${i}`,
      label: `传感器${i}`,
    }))
    setup({ references: many })
    const ta = getTextarea()
    fireEvent.change(ta, { target: { value: '试试 @传感器0' } })
    await new Promise((r) => setTimeout(r, 250))
    expect(screen.queryByRole('listbox')).toBeNull()
    expect(searchMock).not.toHaveBeenCalled()
  })

  it('does not open while an IME composition is in flight', async () => {
    setup()
    const ta = getTextarea()
    fireEvent.compositionStart(ta)
    fireEvent.change(ta, { target: { value: '输入中 @温' } })
    await new Promise((r) => setTimeout(r, 250))
    expect(screen.queryByRole('listbox')).toBeNull()
    // Composition ends -> the same text now triggers detection on next change.
    fireEvent.compositionEnd(ta)
    fireEvent.change(ta, { target: { value: '输入中 @温湿' } })
    await waitFor(() => expect(screen.getByRole('listbox')).toBeTruthy())
  })

  it('legacy mode (no reference handlers) never opens the picker', async () => {
    const { onChange } = setup({ references: undefined, onReferencesChange: undefined })
    const ta = getTextarea()
    fireEvent.change(ta, { target: { value: '普通消息 @某物' } })
    // Give the debounced effect a chance; it must not fetch or render.
    await new Promise((r) => setTimeout(r, 250))
    expect(searchMock).not.toHaveBeenCalled()
    expect(screen.queryByRole('listbox')).toBeNull()
    expect(onChange).toHaveBeenCalledWith('普通消息 @某物')
  })
})
