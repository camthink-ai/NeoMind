/// Tests for the shared chat stream state machine (useChatStream) — the
/// single WS-event interpreter both chat surfaces consume. Covers the full
/// turn lifecycle, multi-round semantics, the session filter (the fix for
/// page/panel cross-talk), cancellation, and token-usage reporting.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, act, cleanup } from '@testing-library/react'
import { useChatStream, type StreamEndResult, type UseChatStreamOptions } from '../useChatStream'
import type { ServerMessage } from '@/types'

// ── ws singleton mock: capture the subscriber the hook registers ──
type Listener = (data: ServerMessage) => void
const listeners = new Set<Listener>()
vi.mock('@/lib/websocket', () => ({
  ws: {
    onMessage: (fn: Listener) => {
      listeners.add(fn)
      return () => listeners.delete(fn)
    },
  },
}))
const emit = (data: Partial<ServerMessage> & { type: string }) => {
  listeners.forEach((l) => l(data as ServerMessage))
}
const nextFrame = () => new Promise((r) => requestAnimationFrame(() => r(null)))

// ── harness: expose the hook's live return value ──
// eslint-disable-next-line @typescript-eslint/no-explicit-any
let latest: any = null
function Harness(props: Partial<UseChatStreamOptions>) {
  latest = useChatStream({
    onEnd: () => {},
    ...props,
  })
  return null
}

describe('useChatStream', () => {
  beforeEach(() => {
    listeners.clear()
    latest = null
  })
  afterEach(cleanup)

  it('runs a complete turn: chunks accumulate, end delivers the assembled message', async () => {
    const onEnd = vi.fn()
    render(<Harness onEnd={onEnd} />)

    act(() => latest.beginTurn())
    expect(latest.isStreaming).toBe(true)
    expect(latest.streamingMessageId).toBeTruthy()

    await act(async () => {
      emit({ type: 'Thinking', content: '思考', sessionId: 's1' })
      emit({ type: 'Content', content: '你好', sessionId: 's1' })
      emit({ type: 'Content', content: '世界', sessionId: 's1' })
      await nextFrame() // frame-throttled content flush
    })
    expect(latest.streamingContent).toBe('你好世界')
    expect(latest.streamingThinking).toBe('思考')

    await act(async () => {
      emit({ type: 'ToolCallStart', tool: 'shell', arguments: { command: 'ls' }, sessionId: 's1' })
      emit({ type: 'ToolCallEnd', tool: 'shell', result: 'ok', sessionId: 's1' })
      emit({ type: 'end', sessionId: 's1' })
    })
    // onEnd fired exactly once with the assembled turn
    expect(onEnd).toHaveBeenCalledTimes(1)
    const result = onEnd.mock.calls[0][0] as StreamEndResult
    expect(result.content).toBe('你好世界')
    expect(result.thinking).toBe('思考')
    expect(result.toolCalls).toHaveLength(1)
    expect(result.toolCalls[0]).toMatchObject({ name: 'shell', result: 'ok' })
    // stream state fully reset
    expect(latest.isStreaming).toBe(false)
    expect(latest.streamingContent).toBe('')
    expect(latest.streamingToolCalls).toHaveLength(0)
    expect(latest.streamingMessageId).toBeNull()
  })

  it('multi-round: intermediate rounds go to roundContents, final round becomes content', async () => {
    const onEnd = vi.fn()
    render(<Harness onEnd={onEnd} />)

    act(() => latest.beginTurn())
    await act(async () => {
      emit({ type: 'Content', content: 'round-1-body', sessionId: 's1' })
      emit({ type: 'IntermediateEnd', sessionId: 's1' })
      emit({ type: 'Content', content: 'final-body', sessionId: 's1' })
      emit({ type: 'end', sessionId: 's1' })
    })

    const result = onEnd.mock.calls[0][0] as StreamEndResult
    expect(result.content).toBe('final-body')
    expect(result.roundContents).toEqual({ 1: 'round-1-body' })
    // thinking carries across rounds
    expect(result.thinking).toBeUndefined()
  })

  it('session filter: events from another session are dropped; control-plane events pass', async () => {
    const onEnd = vi.fn()
    const onRawEvent = vi.fn()
    render(<Harness sessionId="mine" onEnd={onEnd} onRawEvent={onRawEvent} />)

    act(() => latest.beginTurn())
    await act(async () => {
      // Another surface's turn (e.g. the side panel on another session)
      emit({ type: 'Content', content: 'panel-noise', sessionId: 'theirs' })
      emit({ type: 'end', sessionId: 'theirs' })
      await nextFrame()
    })
    expect(latest.streamingContent).toBe('') // dropped
    expect(onEnd).not.toHaveBeenCalled() // their end never lands here

    // Control-plane event with a foreign sessionId is EXEMPT (it names the
    // session to switch TO — dropping it would break cross-tab switching)
    await act(async () => {
      emit({ type: 'session_switched', sessionId: 'next-session' } as unknown as ServerMessage)
    })
    expect(onRawEvent).toHaveBeenCalledTimes(1)

    // Own session still streams normally
    await act(async () => {
      emit({ type: 'Content', content: 'mine-body', sessionId: 'mine' })
      emit({ type: 'end', sessionId: 'mine' })
    })
    expect(onEnd).toHaveBeenCalledTimes(1)
    expect((onEnd.mock.calls[0][0] as StreamEndResult).content).toBe('mine-body')
  })

  it('cancelled resets all stream state without an end event', async () => {
    const onEnd = vi.fn()
    render(<Harness onEnd={onEnd} />)

    act(() => latest.beginTurn())
    await act(async () => {
      emit({ type: 'Content', content: 'partial', sessionId: 's1' })
      emit({ type: 'cancelled', sessionId: 's1' })
    })
    expect(latest.isStreaming).toBe(false)
    expect(onEnd).not.toHaveBeenCalled()
  })

  it('end reports token usage when the backend provides it', async () => {
    const onTokenUsage = vi.fn()
    render(<Harness onEnd={vi.fn()} onTokenUsage={onTokenUsage} />)

    act(() => latest.beginTurn())
    await act(async () => {
      emit({
        type: 'end',
        sessionId: 's1',
        tokenUsage: { promptTokens: 120, systemPromptTokens: 30, toolTokens: 10 },
      } as ServerMessage)
    })
    expect(onTokenUsage).toHaveBeenCalledWith(
      { promptTokens: 120, systemPromptTokens: 30, toolTokens: 10 },
      's1',
    )
  })

  // ── mid-turn interruption (session switch / detach-restore) ──

  it('a mid-turn hardReset marks the end result interrupted (view takes server history)', async () => {
    const onEnd = vi.fn()
    render(<Harness onEnd={onEnd} sessionId="s1" />)

    act(() => latest.beginTurn())
    await act(async () => {
      emit({ type: 'Content', content: '前半段', sessionId: 's1' })
      await nextFrame()
    })

    // User switches away mid-stream: the session-switch effect hard-resets.
    act(() => latest.hardReset())

    await act(async () => {
      emit({ type: 'Content', content: '后半段', sessionId: 's1' })
      await nextFrame()
      emit({ type: 'end', sessionId: 's1' })
    })

    expect(onEnd).toHaveBeenCalledTimes(1)
    const result = onEnd.mock.calls[0][0] as StreamEndResult
    // The local assembly holds only the post-switch tail...
    expect(result.content).toBe('后半段')
    // ...so it is flagged: the view must finalize from server history.
    expect(result.interrupted).toBe(true)
  })

  it('a normal uninterrupted turn does NOT set interrupted', async () => {
    const onEnd = vi.fn()
    render(<Harness onEnd={onEnd} sessionId="s1" />)
    act(() => latest.beginTurn())
    await act(async () => {
      emit({ type: 'Content', content: '完整', sessionId: 's1' })
      emit({ type: 'end', sessionId: 's1' })
    })
    const result = onEnd.mock.calls[0][0] as StreamEndResult
    expect(result.interrupted).toBeUndefined()
  })

  it('an interrupted turn with an EMPTY accumulator still fires onEnd (reload signal)', async () => {
    const onEnd = vi.fn()
    render(<Harness onEnd={onEnd} sessionId="s1" />)
    act(() => latest.beginTurn())
    await act(async () => {
      emit({ type: 'Content', content: '全部内容在切走前', sessionId: 's1' })
      await nextFrame()
    })
    act(() => latest.hardReset())
    await act(async () => {
      emit({ type: 'end', sessionId: 's1' })
    })
    // Empty content + interrupted → onEnd MUST still fire so the view
    // reloads the server's persisted message.
    expect(onEnd).toHaveBeenCalledTimes(1)
    expect(onEnd.mock.calls[0][0].interrupted).toBe(true)
  })

  it('restore() marks the reattached turn interrupted', async () => {
    const onEnd = vi.fn()
    render(<Harness onEnd={onEnd} sessionId="s1" />)
    act(() => latest.restore('服务端快照前缀', '思考前缀'))
    expect(latest.isStreaming).toBe(true)
    await act(async () => {
      emit({ type: 'Content', content: '+增量', sessionId: 's1' })
      await nextFrame()
      emit({ type: 'end', sessionId: 's1' })
    })
    const result = onEnd.mock.calls[0][0] as StreamEndResult
    expect(result.content).toBe('服务端快照前缀+增量')
    expect(result.interrupted).toBe(true)
  })

  it('restore with extras brings back the tool process and round narration', async () => {
    render(<Harness sessionId="s1" />)
    act(() => latest.restore('正文', '', {
      toolCalls: [{ name: 'shell', arguments: { command: 'ls' }, result: 'ok', round: 1 }],
      roundContents: { 1: '第一轮旁白' },
    }))
    expect(latest.streamingToolCalls).toHaveLength(1)
    expect(latest.streamingToolCalls[0]).toMatchObject({ name: 'shell', result: 'ok' })
    expect(latest.roundContents[1]).toBe('第一轮旁白')
  })

  it('restoreIfBehind adopts on tool-count growth alone (slow tools grow no text)', async () => {
    render(<Harness sessionId="s1" />)
    act(() => latest.restore('正文', ''))
    // Same text, but the snapshot now has a tool call — the detached page's
    // only aliveness signal during a long tool execution.
    act(() => latest.restoreIfBehind('正文', '', {
      toolCalls: [{ name: 'shell', arguments: {}, result: 'ok', round: 1 }],
    }))
    expect(latest.streamingToolCalls).toHaveLength(1)
  })

  it('restoreIfBehind adopts the server snapshot only when it is ahead of local', async () => {
    render(<Harness sessionId="s1" />)
    act(() => latest.restore('短快照', ''))
    // Server ahead → adopt
    act(() => latest.restoreIfBehind('长一些的服务端快照', ''))
    expect(latest.streamingContent).toBe('长一些的服务端快照')
    // Local ahead (live events fresher than the ≤2s-lagged snapshot) → keep local
    act(() => latest.restoreIfBehind('长一些的服务端快', ''))
    expect(latest.streamingContent).toBe('长一些的服务端快照')
  })
})
