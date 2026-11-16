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

  it('selects via pointerdown on mobile anchored options', async () => {
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
      fireEvent.pointerDown(options[0])
    })
    await waitFor(() => {
      expect(onRefs).toHaveBeenCalledWith([
        expect.objectContaining({ type: 'device', id: 'sensor-001' }),
      ])
    })
  })
})
