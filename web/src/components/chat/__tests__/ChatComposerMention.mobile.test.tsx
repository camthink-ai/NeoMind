/// Mobile @-mention: anchored panel (not a focus-trapping Sheet).
import React, { useState } from 'react'
import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, screen, waitFor, act, fireEvent } from '@testing-library/react'
import userEvent from '@testing-library/user-event'

vi.mock('@/hooks/useMobile', () => ({
  useIsMobile: () => true,
}))

const MENTION_HITS = [
  { type: 'device' as const, id: 'sensor-001', label: '温湿度传感器' },
]

vi.mock('@/lib/api/mentions', async () => {
  const actual = await vi.importActual<typeof import('@/lib/api/mentions')>('@/lib/api/mentions')
  return {
    ...actual,
    searchMentions: vi.fn(async () => ({
      items: MENTION_HITS,
      total: MENTION_HITS.length,
      limit: 20,
      offset: 0,
      hasMore: false,
      nextOffset: null,
      typeErrors: {},
    })),
    MAX_REFERENCES: 20,
  }
})

import { ChatComposer } from '../ChatComposer'
import { searchMentions } from '@/lib/api/mentions'

const searchMock = vi.mocked(searchMentions)

function Host() {
  const [value, setValue] = useState('')
  const [refs, setRefs] = useState<import('@/types').ChatReference[]>([])
  const [nodes, setNodes] = useState<import('@/types').ChatContentNode[]>([])
  return (
    <ChatComposer
      value={value}
      onChange={setValue}
      onSend={vi.fn()}
      references={refs}
      contentNodes={nodes}
      onReferencesChange={setRefs}
      onContentNodesChange={setNodes}
    />
  )
}

describe('ChatComposer mobile @-mentions', () => {
  beforeEach(() => {
    searchMock.mockClear()
  })

  it('opens an anchored listbox (not a dialog/sheet) and keeps it after toolbar @', async () => {
    render(<Host />)
    await waitFor(() => expect(document.querySelector('.ProseMirror')).toBeTruthy())
    const btn = await screen.findByRole('button', { name: /插入引用|chat:mention.insert/i })
    await userEvent.click(btn)
    const listbox = await screen.findByRole('listbox')
    expect(listbox).toHaveAttribute('data-mention-menu')
    // Radix Sheet/Dialog would expose role=dialog — we must not use it on mobile.
    expect(screen.queryByRole('dialog')).toBeNull()
    await waitFor(() => expect(screen.getAllByRole('option').length).toBeGreaterThan(0))
    // Still open after a frame (no open→immediate-close flicker).
    await act(async () => {
      await new Promise((r) => setTimeout(r, 50))
    })
    expect(screen.getByRole('listbox')).toBeTruthy()
  })

  const pickedDevice = (onRefs: ReturnType<typeof vi.fn>) =>
    onRefs.mock.calls.some(
      (call) => Array.isArray(call[0]) && call[0].some((r: { id?: string }) => r?.id === 'sensor-001'),
    )

  // jsdom lacks PointerEvent — synthesize a bubbling Event with pointer fields.
  const dispatchWindowPointer = (
    type: 'pointermove' | 'pointerup' | 'pointercancel',
    init: { pointerId: number; clientX: number; clientY: number },
  ) => {
    const ev = new Event(type, { bubbles: true })
    Object.assign(ev, init)
    window.dispatchEvent(ev)
  }

  it('selects on pointerup when the gesture did not scroll', async () => {
    const onRefs = vi.fn()
    function SelectHost() {
      const [value, setValue] = useState('')
      const [refs, setRefs] = useState<import('@/types').ChatReference[]>([])
      const [nodes, setNodes] = useState<import('@/types').ChatContentNode[]>([])
      return (
        <ChatComposer
          value={value}
          onChange={setValue}
          onSend={vi.fn()}
          references={refs}
          contentNodes={nodes}
          onReferencesChange={(r) => {
            onRefs(r)
            setRefs(r)
          }}
          onContentNodesChange={setNodes}
        />
      )
    }
    render(<SelectHost />)
    await waitFor(() => expect(document.querySelector('.ProseMirror')).toBeTruthy())
    const btn = await screen.findByRole('button', { name: /插入引用|chat:mention.insert/i })
    await userEvent.click(btn)
    const options = await screen.findAllByRole('option')
    await act(async () => {
      // Mobile must NOT select on pointerdown alone — that blocked scrolling.
      fireEvent.pointerDown(options[0], { pointerId: 1, clientX: 20, clientY: 40 })
    })
    expect(pickedDevice(onRefs)).toBe(false)
    await act(async () => {
      dispatchWindowPointer('pointerup', { pointerId: 1, clientX: 22, clientY: 42 })
    })
    await waitFor(() => expect(pickedDevice(onRefs)).toBe(true))
  })

  it('keeps suppress armed across listbox bubble + frames during a mobile gesture', async () => {
    // Regression: container onPointerDown used to releaseMentionSuppress after
    // 2 rAF even when an option gesture was active, so TipTap onExit could
    // clear the command before pointerup.
    render(<Host />)
    await waitFor(() => expect(document.querySelector('.ProseMirror')).toBeTruthy())
    const btn = await screen.findByRole('button', { name: /插入引用|chat:mention.insert/i })
    await userEvent.click(btn)
    const options = await screen.findAllByRole('option')
    const listbox = screen.getByRole('listbox')
    await act(async () => {
      fireEvent.pointerDown(options[0], { pointerId: 3, clientX: 10, clientY: 10 })
      // Bubble reaches the listbox (same as real touch).
      fireEvent.pointerDown(listbox, { pointerId: 3, clientX: 10, clientY: 10 })
    })
    await act(async () => {
      await new Promise((r) => requestAnimationFrame(() => requestAnimationFrame(r)))
    })
    // Gesture still open — menu must stay (exit suppress not released).
    expect(screen.getByRole('listbox')).toBeTruthy()
    await act(async () => {
      dispatchWindowPointer('pointerup', { pointerId: 3, clientX: 12, clientY: 12 })
    })
  })

  it('does not select when the pointer moved like a scroll gesture', async () => {
    const onRefs = vi.fn()
    function SelectHost() {
      const [value, setValue] = useState('')
      const [refs, setRefs] = useState<import('@/types').ChatReference[]>([])
      const [nodes, setNodes] = useState<import('@/types').ChatContentNode[]>([])
      return (
        <ChatComposer
          value={value}
          onChange={setValue}
          onSend={vi.fn()}
          references={refs}
          contentNodes={nodes}
          onReferencesChange={(r) => {
            onRefs(r)
            setRefs(r)
          }}
          onContentNodesChange={setNodes}
        />
      )
    }
    render(<SelectHost />)
    await waitFor(() => expect(document.querySelector('.ProseMirror')).toBeTruthy())
    const btn = await screen.findByRole('button', { name: /插入引用|chat:mention.insert/i })
    await userEvent.click(btn)
    const options = await screen.findAllByRole('option')
    await act(async () => {
      fireEvent.pointerDown(options[0], { pointerId: 7, clientX: 20, clientY: 40 })
      dispatchWindowPointer('pointermove', { pointerId: 7, clientX: 20, clientY: 80 })
      dispatchWindowPointer('pointerup', { pointerId: 7, clientX: 20, clientY: 120 })
    })
    expect(pickedDevice(onRefs)).toBe(false)
    expect(screen.getByRole('listbox')).toBeTruthy()
  })
})
