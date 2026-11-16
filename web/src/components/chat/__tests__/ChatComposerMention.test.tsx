/// TipTap inline-@ production composer tests (plan §12 C).
import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, screen, fireEvent, waitFor, act } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { ChatComposer } from '../ChatComposer'

const MENTION_HITS = [
  { type: 'device' as const, id: 'sensor-001', label: '温湿度传感器' },
  { type: 'device' as const, id: 'sensor-002', label: '温湿度传感器备用' },
  { type: 'rule' as const, id: 'rule-1', label: '高温报警' },
  { type: 'transform' as const, id: 't-1', label: '温度均值' },
  { type: 'agent' as const, id: 'a-1', label: '巡检代理' },
]

vi.mock('@/lib/api/mentions', async () => {
  const actual = await vi.importActual<typeof import('@/lib/api/mentions')>('@/lib/api/mentions')
  return {
    ...actual,
    searchMentions: vi.fn(async (params: { query?: string }) => {
      const q = params.query ?? ''
      const items = q
        ? MENTION_HITS.filter((o) => o.label.includes(q) || o.id.includes(q))
        : MENTION_HITS
      return {
        items,
        total: items.length,
        limit: 20,
        offset: 0,
        hasMore: false,
        nextOffset: null,
        typeErrors: {},
      }
    }),
    MAX_REFERENCES: 20,
  }
})

import { searchMentions } from '@/lib/api/mentions'
const searchMock = vi.mocked(searchMentions)

function getEditor(): HTMLElement {
  const el = document.querySelector('.ProseMirror') as HTMLElement | null
  if (!el) throw new Error('TipTap ProseMirror not mounted')
  return el
}

async function openMentionViaToolbar() {
  const btn = await screen.findByRole('button', { name: /插入引用|chat:mention.insert/i })
  await userEvent.click(btn)
}

type HostState = {
  value: string
  refs: import('@/types').ChatReference[]
  nodes: import('@/types').ChatContentNode[]
}

function Host(props: {
  initial?: Partial<HostState>
  invalidRefs?: Record<string, string>
  onKeyDown?: (e: React.KeyboardEvent) => void
  onReferencesChange?: ReturnType<typeof vi.fn>
  onContentNodesChange?: ReturnType<typeof vi.fn>
  onChange?: ReturnType<typeof vi.fn>
}) {
  const [value, setValue] = React.useState(props.initial?.value ?? '')
  const [refs, setRefs] = React.useState(props.initial?.refs ?? [])
  const [nodes, setNodes] = React.useState(props.initial?.nodes ?? [])
  return (
    <ChatComposer
      value={value}
      onChange={(v) => {
        props.onChange?.(v)
        setValue(v)
      }}
      onSend={vi.fn()}
      onKeyDown={props.onKeyDown}
      references={refs}
      contentNodes={nodes}
      invalidRefs={props.invalidRefs}
      onReferencesChange={(r) => {
        props.onReferencesChange?.(r)
        setRefs(r)
      }}
      onContentNodesChange={(n) => {
        props.onContentNodesChange?.(n)
        setNodes(n)
      }}
    />
  )
}

// React import for Host
import React from 'react'

function setup(
  overrides: {
    initial?: Partial<HostState>
    invalidRefs?: Record<string, string>
    onKeyDown?: (e: React.KeyboardEvent) => void
  } = {},
) {
  const onChange = vi.fn()
  const onReferencesChange = vi.fn()
  const onContentNodesChange = vi.fn()
  render(
    <Host
      {...overrides}
      onChange={onChange}
      onReferencesChange={onReferencesChange}
      onContentNodesChange={onContentNodesChange}
    />,
  )
  return { onChange, onReferencesChange, onContentNodesChange }
}

describe('ChatComposer TipTap @-mentions', () => {
  beforeEach(() => {
    searchMock.mockClear()
  })

  it('mounts TipTap and opens the picker from the @ toolbar', async () => {
    setup()
    await waitFor(() => expect(getEditor()).toBeTruthy())
    await openMentionViaToolbar()
    await waitFor(() => expect(screen.getByRole('listbox')).toBeTruthy())
    await waitFor(() => expect(screen.getAllByRole('option').length).toBeGreaterThan(0))
    expect(searchMock).toHaveBeenCalled()
  })

  it('filters by a Chinese query typed after @', async () => {
    setup()
    await waitFor(() => expect(getEditor()).toBeTruthy())
    await userEvent.click(getEditor())
    await userEvent.keyboard('@温湿度')
    await waitFor(() =>
      expect(searchMock).toHaveBeenCalledWith(
        expect.objectContaining({ query: expect.stringMatching(/温湿度/) }),
      ),
    )
  })

  it('pointerdown selects a mention (mobile-safe; does not wait for click after blur)', async () => {
    const { onReferencesChange } = setup()
    await waitFor(() => expect(getEditor()).toBeTruthy())
    await openMentionViaToolbar()
    const options = await screen.findAllByRole('option')
    await act(async () => {
      fireEvent.pointerDown(options[0])
    })
    await waitFor(() => {
      expect(onReferencesChange).toHaveBeenCalledWith([
        expect.objectContaining({ type: 'device', id: 'sensor-001' }),
      ])
    })
    expect(screen.queryByRole('listbox')).toBeNull()
  })

  it('keyboard-selects a mention into an inline atom (contentNodes)', async () => {
    const { onReferencesChange, onContentNodesChange } = setup()
    await waitFor(() => expect(getEditor()).toBeTruthy())
    await openMentionViaToolbar()
    await screen.findAllByRole('option')
    await act(async () => {
      fireEvent.keyDown(getEditor(), { key: 'Enter' })
    })
    await waitFor(() => {
      expect(onReferencesChange).toHaveBeenCalledWith([
        expect.objectContaining({ type: 'device', id: 'sensor-001' }),
      ])
    })
    await waitFor(() => {
      expect(onContentNodesChange).toHaveBeenCalledWith(
        expect.arrayContaining([
          expect.objectContaining({
            kind: 'reference',
            type: 'device',
            id: 'sensor-001',
          }),
        ]),
      )
    })
    expect(document.querySelector('.nm-mention')).toBeTruthy()
  })

  it('Escape closes the picker without selecting', async () => {
    const { onReferencesChange } = setup()
    await waitFor(() => expect(getEditor()).toBeTruthy())
    await openMentionViaToolbar()
    await screen.findAllByRole('option')
    await userEvent.keyboard('{Escape}')
    await waitFor(() => expect(screen.queryByRole('listbox')).toBeNull())
    // TipTap may emit empty reference projections on sync; never a selection.
    expect(
      onReferencesChange.mock.calls.filter((c) => (c[0] as unknown[]).length > 0),
    ).toHaveLength(0)
  })

  /// Regression (suppressor stuck): after a POINTER pick, the pick's own
  /// pointerdown bubbles into the listbox container, which re-armed
  /// suppressMentionExitClose AFTER selectMention's finally had reset it —
  /// nothing ever cleared it again, so every LATER suggestion exit (typing a
  /// space after `@`, deleting the trigger, send clearing the doc) was
  /// blocked and the menu floated forever. The container handler now
  /// self-releases; a subsequent exit must close the menu.
  it('suggestion exit still closes the menu after a pointer pick', async () => {
    vi.useRealTimers()
    setup()
    await waitFor(() => expect(getEditor()).toBeTruthy())
    await openMentionViaToolbar()
    const options = await screen.findAllByRole('option')
    await act(async () => {
      fireEvent.pointerDown(options[0])
    })
    await waitFor(() => expect(screen.queryByRole('listbox')).toBeNull())

    // New picker session: type @, wait for options, then exit WITHOUT
    // picking — a space kills the suggestion range → onExit must close.
    await userEvent.click(getEditor())
    await userEvent.keyboard('@')
    await screen.findAllByRole('option')
    await userEvent.keyboard(' ')
    await waitFor(
      () => expect(screen.queryByRole('listbox')).toBeNull(),
      { timeout: 3000 },
    )
  }, 15000)

  it('allows the same entity twice in-sentence (no chip dedupe)', async () => {
    const { onContentNodesChange } = setup({
      initial: {
        nodes: [
          { kind: 'text', text: '把 ' },
          {
            kind: 'reference',
            type: 'device',
            id: 'sensor-001',
            labelSnapshot: '温湿度传感器',
          },
          { kind: 'text', text: ' 复制给 ' },
        ],
        refs: [{ type: 'device', id: 'sensor-001', label: '温湿度传感器' }],
      },
    })
    await waitFor(() => expect(document.querySelector('.nm-mention')).toBeTruthy())
    await openMentionViaToolbar()
    await screen.findAllByRole('option')
    await act(async () => {
      fireEvent.keyDown(getEditor(), { key: 'Enter' })
    })
    await waitFor(() => {
      const calls = onContentNodesChange.mock.calls
      const last = calls[calls.length - 1]?.[0] as
        | Array<{ kind: string }>
        | undefined
      const refs = (last ?? []).filter((n) => n.kind === 'reference')
      expect(refs.length).toBeGreaterThanOrEqual(2)
    })
  })

  it('does not open for email-like text (word char before @)', async () => {
    setup()
    await waitFor(() => expect(getEditor()).toBeTruthy())
    await userEvent.click(getEditor())
    await userEvent.keyboard('a@b.com')
    await new Promise((r) => setTimeout(r, 300))
    expect(screen.queryByRole('listbox')).toBeNull()
  })

  it('ArrowDown/ArrowUp move aria-selected and Enter selects', async () => {
    const { onReferencesChange } = setup()
    await waitFor(() => expect(getEditor()).toBeTruthy())
    await openMentionViaToolbar()
    const options = await screen.findAllByRole('option')
    expect(options[0]).toHaveAttribute('aria-selected', 'true')
    fireEvent.keyDown(getEditor(), { key: 'ArrowDown' })
    await waitFor(() => {
      expect(screen.getAllByRole('option')[1]).toHaveAttribute('aria-selected', 'true')
    })
    await act(async () => {
      fireEvent.keyDown(getEditor(), { key: 'Enter' })
    })
    await waitFor(() => {
      expect(onReferencesChange).toHaveBeenCalledWith([
        expect.objectContaining({ type: 'device', id: 'sensor-002' }),
      ])
    })
  })

  it('IME Enter (keyCode 229) does not select or forward to onKeyDown', async () => {
    const onKeyDown = vi.fn()
    const { onReferencesChange } = setup({ onKeyDown })
    await waitFor(() => expect(getEditor()).toBeTruthy())
    await openMentionViaToolbar()
    await screen.findAllByRole('option')
    fireEvent.keyDown(getEditor(), { key: 'Enter', keyCode: 229, isComposing: true })
    expect(
      onReferencesChange.mock.calls.filter((c) => (c[0] as unknown[]).length > 0),
    ).toHaveLength(0)
    expect(onKeyDown).not.toHaveBeenCalled()
  })

  it('exposes listbox a11y: activedescendant and filter tabs', async () => {
    setup()
    await waitFor(() => expect(getEditor()).toBeTruthy())
    await openMentionViaToolbar()
    const listbox = await screen.findByRole('listbox')
    await screen.findAllByRole('option')
    expect(listbox).toHaveAttribute('aria-activedescendant', 'chat-mention-opt-0')
    expect(screen.getByRole('tablist')).toBeTruthy()
    expect(screen.getAllByRole('tab').length).toBe(5)
  })

  it('invalidRefs mark inline mention atoms with data-invalid', async () => {
    setup({
      initial: {
        nodes: [
          { kind: 'text', text: '' },
          {
            kind: 'reference',
            type: 'device',
            id: 'ghost',
            labelSnapshot: '幽灵',
          },
        ],
        refs: [{ type: 'device', id: 'ghost', label: '幽灵' }],
      },
      invalidRefs: { 'device:ghost': 'no device with this id' },
    })
    await waitFor(() => {
      const chip = document.querySelector('.nm-mention[data-invalid="true"]')
      expect(chip).toBeTruthy()
      expect(chip?.getAttribute('title')).toContain('no device')
    })
  })

  it('shows shortId/hint for duplicateName options (not color-only)', async () => {
    searchMock.mockResolvedValueOnce({
      items: [
        {
          type: 'device',
          id: 'sensor-001',
          label: '温湿度传感器',
          shortId: 'sensor-0',
          hint: 'temp_humidity · mqtt',
          duplicateName: true,
        },
        {
          type: 'device',
          id: 'sensor-002',
          label: '温湿度传感器',
          shortId: 'sensor-0',
          hint: 'temp_humidity · mqtt',
          duplicateName: true,
        },
      ],
      total: 2,
      limit: 20,
      offset: 0,
      hasMore: false,
      nextOffset: null,
      typeErrors: {},
    })
    setup()
    await waitFor(() => expect(getEditor()).toBeTruthy())
    await openMentionViaToolbar()
    await userEvent.keyboard('温湿度')
    const options = await screen.findAllByRole('option')
    expect(options[0].textContent).toContain('sensor-0')
    expect(options[0].textContent).toContain('temp_humidity')
  })

  it('legacy mode (no reference handlers) keeps a textarea and never opens the picker', async () => {
    const onChange = vi.fn()
    render(<ChatComposer value="" onChange={onChange} onSend={vi.fn()} />)
    const ta = screen.getByRole('textbox') as HTMLTextAreaElement
    expect(ta.tagName).toBe('TEXTAREA')
    fireEvent.change(ta, { target: { value: '普通消息 @某物' } })
    await new Promise((r) => setTimeout(r, 250))
    expect(searchMock).not.toHaveBeenCalled()
    expect(screen.queryByRole('listbox')).toBeNull()
    expect(onChange).toHaveBeenCalledWith('普通消息 @某物')
  })

  it('loadMore discards a stale page that lands after the query changed (E17)', async () => {
    type SearchResult = Awaited<ReturnType<typeof searchMentions>>
    const result = (
      items: Array<{ type: 'device' | 'rule'; id: string; label: string }>,
      hasMore = false,
      nextOffset: number | null = null,
    ): SearchResult => ({
      items,
      total: items.length,
      limit: 20,
      offset: 0,
      hasMore,
      nextOffset,
      typeErrors: {},
    })
    let resolveStale!: (v: SearchResult) => void
    searchMock
      // Page 1 of the initial (empty) query — hasMore true so 加载更多 shows.
      .mockImplementationOnce(async () =>
        result(
          [
            { type: 'device', id: 'sensor-001', label: '设备甲' },
            { type: 'device', id: 'sensor-002', label: '设备乙' },
          ],
          true,
          20,
        ),
      )
      // Page 2 of the SAME query — held pending to simulate a slow response.
      .mockImplementationOnce(() => new Promise<SearchResult>((res) => { resolveStale = res }))
      // Fresh search for the retyped query.
      .mockImplementationOnce(async () => result([{ type: 'rule', id: 'rule-9', label: '规则丙' }]))
    setup()
    await waitFor(() => expect(getEditor()).toBeTruthy())
    await openMentionViaToolbar()
    await screen.findAllByRole('option')
    const more = await screen.findByRole('button', { name: /加载更多|load more|mention\.loadMore/i })
    await act(async () => {
      fireEvent.click(more)
    })
    // Retype while page 2 is in flight — the picker now belongs to 规则丙.
    await userEvent.keyboard('规则')
    await waitFor(() => {
      const options = screen.getAllByRole('option')
      expect(options).toHaveLength(1)
      expect(options[0].textContent).toContain('规则丙')
    })
    // The stale page-2 response lands NOW — it must NOT merge into the new
    // query's list nor corrupt its pagination.
    await act(async () => {
      resolveStale(result([{ type: 'device', id: 'sensor-003', label: '设备丁' }]))
    })
    await new Promise((r) => setTimeout(r, 60))
    const options = screen.getAllByRole('option')
    expect(options).toHaveLength(1)
    expect(options[0].textContent).toContain('规则丙')
  })
})
