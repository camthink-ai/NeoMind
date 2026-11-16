/**
 * TipTap controlled echo + CJK IME: parent contentNodes must not bounce back
 * through setContent while the user is composing.
 */
import React, { useState } from 'react'
import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, waitFor, act, fireEvent } from '@testing-library/react'
import { InlineMentionEditor } from '../InlineMentionEditor'
import type { ChatContentNode } from '@/types'

function getEditor(): HTMLElement {
  const el = document.querySelector('.ProseMirror') as HTMLElement | null
  if (!el) throw new Error('TipTap ProseMirror not mounted')
  return el
}

function ControlledHost({
  onCompositionChange,
  injectRef,
}: {
  onCompositionChange?: (c: boolean) => void
  injectRef?: React.MutableRefObject<
    ((nodes: ChatContentNode[], value: string) => void) | null
  >
}) {
  const [value, setValue] = useState('')
  const [nodes, setNodes] = useState<ChatContentNode[]>([])
  if (injectRef) {
    injectRef.current = (nextNodes, nextValue) => {
      setNodes(nextNodes)
      setValue(nextValue)
    }
  }
  return (
    <InlineMentionEditor
      value={value}
      contentNodes={nodes}
      onPayloadChange={(p) => {
        setValue(p.message)
        setNodes(p.contentNodes)
      }}
      onSuggestionStart={vi.fn()}
      onSuggestionUpdate={vi.fn()}
      onSuggestionExit={vi.fn()}
      onCompositionChange={onCompositionChange}
    />
  )
}

describe('InlineMentionEditor IME / controlled sync', () => {
  beforeEach(() => {
    vi.clearAllMocks()
  })

  it('marks composition via onCompositionChange and ignores external setContent while composing', async () => {
    const onCompositionChange = vi.fn()
    const injectRef: React.MutableRefObject<
      ((nodes: ChatContentNode[], value: string) => void) | null
    > = { current: null }
    render(
      <ControlledHost
        onCompositionChange={onCompositionChange}
        injectRef={injectRef}
      />,
    )
    await waitFor(() => expect(getEditor()).toBeTruthy())
    const ed = getEditor()

    await act(async () => {
      fireEvent.compositionStart(ed)
    })
    expect(onCompositionChange).toHaveBeenCalledWith(true)

    // External draft restore while composing must not wipe the IME session.
    await act(async () => {
      injectRef.current?.(
        [{ kind: 'text', text: '外部注入应被忽略' }],
        '外部注入应被忽略',
      )
    })
    expect(ed.textContent ?? '').not.toContain('外部注入应被忽略')

    await act(async () => {
      fireEvent.compositionEnd(ed)
    })
    expect(onCompositionChange).toHaveBeenCalledWith(false)
  })

  it('controlled contentNodes seed + echo keep editor text', async () => {
    const injectRef: React.MutableRefObject<
      ((nodes: ChatContentNode[], value: string) => void) | null
    > = { current: null }
    render(<ControlledHost injectRef={injectRef} />)
    await waitFor(() => expect(getEditor()).toBeTruthy())
    const ed = getEditor()

    await act(async () => {
      injectRef.current?.(
        [{ kind: 'text', text: '你好世界' }],
        '你好世界',
      )
    })
    await waitFor(() => {
      expect(ed.textContent ?? '').toContain('你好世界')
    })

    // Echo the same controlled payload (onUpdate → parent → props).
    // Without lastNodesKey bookkeeping this path called setContent every time.
    await act(async () => {
      injectRef.current?.(
        [{ kind: 'text', text: '你好世界' }],
        '你好世界',
      )
    })
    expect(ed.textContent ?? '').toContain('你好世界')
  })
})
