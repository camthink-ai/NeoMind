/// Rendering tests for ChatMessages — the shared message list both chat
/// surfaces render. Covers the user bubble, the assistant three-layer
/// layout (thinking / tool process / final answer), the streaming synthetic
/// message, and the load-earlier pagination affordance.
import { describe, it, expect, vi } from 'vitest'
import { render, screen } from '@testing-library/react'
import { ChatMessages } from '../ChatMessages'
import type { Message } from '@/types'

const _noop = () => {}

describe('ChatMessages', () => {
  it('renders a user message bubble with its content', () => {
    const messages: Message[] = [
      { id: 'u1', role: 'user', content: '打开客厅的灯', timestamp: 1 },
    ]
    render(
      <ChatMessages
        messages={messages}
        user={null}
        isStreaming={false}
        streamingContent=""
        streamingThinking=""
        streamingRoundThinking={{}}
        streamingToolCalls={[]}
        roundContents={{}}
        currentRound={1}
      />,
    )
    expect(screen.getByText('打开客厅的灯')).toBeTruthy()
  })

  it('renders an assistant message with tool calls (process block + final answer)', () => {
    const messages: Message[] = [
      {
        id: 'a1',
        role: 'assistant',
        content: '灯已打开。',
        timestamp: 2,
        tool_calls: [{ id: 't1', name: 'shell', arguments: { command: 'neomind device control lamp on' }, result: 'ok' }],
        round_contents: { 1: '检查设备列表' },
      },
    ]
    render(
      <ChatMessages
        messages={messages}
        user={null}
        isStreaming={false}
        streamingContent=""
        streamingThinking=""
        streamingRoundThinking={{}}
        streamingToolCalls={[]}
        roundContents={{}}
        currentRound={1}
      />,
    )
    expect(screen.getByText('灯已打开。')).toBeTruthy()
  })

  it('renders the streaming synthetic message while a turn is in flight', () => {
    render(
      <ChatMessages
        messages={[]}
        user={null}
        isStreaming
        streamingContent="正在生成"
        streamingThinking=""
        streamingRoundThinking={{}}
        streamingToolCalls={[]}
        roundContents={{}}
        currentRound={1}
        streamingMessageId="__streaming__"
      />,
    )
    expect(screen.getByText('正在生成')).toBeTruthy()
  })

  it('while streaming, hides the in-flight turn fragments that the history reload brought in', () => {
    // Re-entering a session mid-turn loads the LIVE agent history, whose
    // tail contains the running turn's fragments. They must not render
    // next to the streaming bubble (double display: an extra Thinking
    // block whose content then "restarts" below it).
    render(
      <ChatMessages
        messages={[
          { id: 'u1', role: 'user', content: '先分析系统', timestamp: 1 },
          { id: 'a1', role: 'assistant', content: '上一次的完整回答', timestamp: 2 },
          { id: 'u2', role: 'user', content: '更新记忆文件', timestamp: 3 },
          {
            id: 'a2', role: 'assistant', content: '', timestamp: 4,
            thinking: '进行中回合的思考分片',
            tool_calls: [{ name: 'shell', arguments: { command: 'ls' }, result: 'ok' }],
          },
        ] as unknown as Message[]}
        user={null}
        isStreaming
        streamingContent="恢复后继续的正文"
        streamingThinking=""
        streamingRoundThinking={{}}
        streamingToolCalls={[]}
        roundContents={{}}
        currentRound={1}
        streamingMessageId="__streaming__"
      />,
    )
    // The completed prior turn stays visible...
    expect(screen.getByText('上一次的完整回答')).toBeTruthy()
    expect(screen.getByText('更新记忆文件')).toBeTruthy()
    // ...the in-flight fragment is hidden (the bubble owns the display)...
    expect(screen.queryByText('进行中回合的思考分片')).toBeNull()
    // ...and the streaming bubble shows.
    expect(screen.getByText('恢复后继续的正文')).toBeTruthy()
  })

  it('shows the load-earlier affordance only when earlier history exists', () => {
    const props = {
      messages: [{ id: 'u1', role: 'user', content: 'hi', timestamp: 1 }] as Message[],
      user: null,
      isStreaming: false,
      streamingContent: '',
      streamingThinking: '',
      streamingRoundThinking: {},
      streamingToolCalls: [] as never[],
      roundContents: {},
      currentRound: 1,
    }
    // Without earlier history: no button
    const { unmount } = render(<ChatMessages {...props} />)
    expect(screen.queryByText('loadEarlier')).toBeNull()
    unmount()

    // With earlier history + handler: button appears and fires the loader
    const onLoadEarlier = vi.fn()
    render(<ChatMessages {...props} hasEarlierHistory loadingEarlier={false} onLoadEarlier={onLoadEarlier} />)
    const btn = screen.getByText('loadEarlier')
    btn.click()
    expect(onLoadEarlier).toHaveBeenCalledTimes(1)
  })
})
