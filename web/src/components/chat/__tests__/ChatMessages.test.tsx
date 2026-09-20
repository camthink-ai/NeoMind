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
