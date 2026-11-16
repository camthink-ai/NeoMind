/**
 * TipTap controlled echo + CJK IME: parent contentNodes must not bounce back
 * through setContent while the user is composing.
 */
import React, { useState } from 'react'
import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, waitFor, act, fireEvent } from '@testing-library/react'
import type { Editor } from '@tiptap/react'
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
  editorRef,
}: {
  onCompositionChange?: (c: boolean) => void
  injectRef?: React.MutableRefObject<
    ((nodes: ChatContentNode[], value: string) => void) | null
  >
  editorRef?: React.MutableRefObject<Editor | null>
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
      editorRef={editorRef}
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
    // Empty/cancelled compose: deferred external restore still applies.
    await waitFor(() =>
      expect(ed.textContent ?? '').toContain('外部注入应被忽略'),
    )
  })

  it('prefers a committed composition over a mid-compose external draft wipe', async () => {
    const injectRef: React.MutableRefObject<
      ((nodes: ChatContentNode[], value: string) => void) | null
    > = { current: null }
    const editorRef: React.MutableRefObject<Editor | null> = { current: null }
    render(<ControlledHost injectRef={injectRef} editorRef={editorRef} />)
    await waitFor(() => expect(editorRef.current).toBeTruthy())
    const ed = getEditor()

    // Seed a non-empty external draft so a later clear actually changes props
    // (identical '' → '' would skip React re-render and never park pendingSync).
    await act(async () => {
      injectRef.current?.([], 'parked-draft')
    })
    await waitFor(() => expect(ed.textContent ?? '').toContain('parked-draft'))

    await act(async () => {
      fireEvent.compositionStart(ed)
    })
    // Advance the TipTap doc while composing (onUpdate emit is suppressed).
    await act(async () => {
      editorRef.current?.chain().focus('end').insertContent('你好').run()
    })
    await waitFor(() => expect(ed.textContent ?? '').toContain('你好'))
    await act(async () => {
      injectRef.current?.([], '')
    })
    // Mid-compose clear must not wipe the in-flight IME text.
    expect(ed.textContent ?? '').toContain('你好')

    await act(async () => {
      fireEvent.compositionEnd(ed)
    })
    // Parked clear must not overwrite the just-committed CJK.
    expect(ed.textContent ?? '').toContain('你好')
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
