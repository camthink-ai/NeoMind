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

  it('renders a log region with hover-revealed copy and no per-message scroll button', () => {
    render(
      <ChatMessages
        messages={[{ id: 'a1', role: 'assistant', content: '回答', timestamp: 2 }]}
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
    const log = screen.getByTestId('messages-container')
    expect(log.getAttribute('role')).toBe('log')
    expect(log.getAttribute('aria-busy')).toBe('false')
    expect(screen.getByTestId('assistant-message')).toBeTruthy()
    expect(screen.getByRole('button', { name: 'copyMessage' })).toBeTruthy()
    // The jump-to-bottom control is host-level now, not per message.
    expect(screen.queryByRole('button', { name: 'scrollToBottom' })).toBeNull()
  })

  it('announces generation start and completion without announcing every chunk', () => {
    const props = {
      messages: [] as Message[],
      user: null,
      streamingContent: '生成中',
      streamingThinking: '',
      streamingRoundThinking: {},
      streamingToolCalls: [] as never[],
      roundContents: {},
      currentRound: 1,
      streamingMessageId: '__streaming__',
    }
    const { rerender } = render(<ChatMessages {...props} isStreaming />)
    expect(screen.getByText('a11y.generating')).toBeTruthy()
    rerender(<ChatMessages {...props} isStreaming={false} />)
    expect(screen.getByText('a11y.answerReady')).toBeTruthy()
  })

  it('redraws history user messages as inline chips from contentNodes', () => {
    const messages: Message[] = [
      {
        id: 'u1',
        role: 'user',
        content: '把 @温室A 复制给 @温室B',
        timestamp: 1,
        contentNodes: [
          { kind: 'text', text: '把 ' },
          { kind: 'reference', type: 'device', id: 'green-01', labelSnapshot: '温室A' },
          { kind: 'text', text: ' 复制给 ' },
          { kind: 'reference', type: 'device', id: 'green-02', labelSnapshot: '温室B' },
        ],
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
    const root = document.querySelector('[data-user-content-nodes]')
    expect(root).toBeTruthy()
    const chips = root!.querySelectorAll('.nm-mention')
    expect(chips).toHaveLength(2)
    expect(chips[0].getAttribute('data-id')).toBe('green-01')
    expect(chips[1].getAttribute('data-id')).toBe('green-02')
    expect(chips[0].textContent).toContain('温室A')
    // No chip fill — type is color-only; name inherits bubble/composer text.
    expect(chips[0].className).not.toMatch(/\bbg-card\b|\bbg-muted\b/)
    const typeEl = chips[0].querySelector('span')
    expect(typeEl?.className ?? '').toContain('text-accent-cyan')
    expect(typeEl?.className ?? '').not.toMatch(/bg-accent-/)
    // Plain markdown path must not swallow the ordered nodes.
    expect(screen.queryByText('把 @温室A 复制给 @温室B')).toBeNull()
  })

  it('shows in-thread compacting status instead of relying on toasts', () => {
    render(
      <ChatMessages
        messages={[]}
        user={null}
        isStreaming={false}
        streamingContent=""
        streamingThinking=""
        streamingRoundThinking={{}}
        streamingToolCalls={[]}
        roundContents={{}}
        currentRound={1}
        contextAction={{ phase: 'running' }}
      />,
    )
    const status = document.querySelector('[data-context-action="running"]')
    expect(status).toBeTruthy()
    expect(status!.textContent).toMatch(/Compacting|压缩|context\.compacting/i)
  })

  it('reuse button fills the composer via onReuseUserMessage', () => {
    const onReuse = vi.fn()
    const message: Message = {
      id: 'u1',
      role: 'user',
      content: '检查 @温湿度',
      timestamp: 1,
      contentNodes: [
        { kind: 'text', text: '检查 ' },
        { kind: 'reference', type: 'device', id: 'sensor-001', labelSnapshot: '温湿度' },
      ],
    }
    render(
      <ChatMessages
        messages={[message]}
        user={null}
        isStreaming={false}
        streamingContent=""
        streamingThinking=""
        streamingRoundThinking={{}}
        streamingToolCalls={[]}
        roundContents={{}}
        currentRound={1}
        onReuseUserMessage={onReuse}
      />,
    )
    const btn = screen.getByRole('button', { name: /reuseInComposer|填回输入框/i })
    btn.click()
    expect(onReuse).toHaveBeenCalledTimes(1)
    expect(onReuse.mock.calls[0][0].contentNodes).toHaveLength(2)
  })

  it('renders system goalStatus callout; not_applicable is not a success badge', () => {
    // Separate turns (user between) so mergeMessagesForDisplay keeps both.
    const messages: Message[] = [
      { id: 'u1', role: 'user', content: 'q1', timestamp: 1 },
      {
        id: 'a1',
        role: 'assistant',
        content: '已推送成功',
        timestamp: 2,
        goalStatus: { status: 'not_applicable' },
        endReason: 'normal',
      },
      { id: 'u2', role: 'user', content: 'q2', timestamp: 3 },
      {
        id: 'a2',
        role: 'assistant',
        content: '配置好了',
        timestamp: 4,
        goalStatus: {
          status: 'incomplete',
          reason: 'config only',
          level: 'config_success',
          task_complete_ok: false,
          physical_ok: false,
        },
        endReason: 'normal',
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
    const na = document.querySelector('[data-goal-status="not_applicable"]')
    expect(na).toBeTruthy()
    expect(na!.className).not.toMatch(/success/)
    expect(document.querySelector('[data-goal-status="incomplete"]')).toBeTruthy()
    // Model text left intact — not rewritten from goalStatus.
    expect(screen.getByText('已推送成功')).toBeTruthy()
    expect(screen.getByText('配置好了')).toBeTruthy()
  })

  it('normalizes snake_case content_nodes from history into chips', () => {
    const messages = [
      {
        id: 'u1',
        role: 'user',
        content: '看设备',
        timestamp: 1,
        content_nodes: [
          { kind: 'text', text: '看 ' },
          { kind: 'reference', type: 'device', id: 'd1', label_snapshot: '摄像头' },
        ],
      },
    ] as unknown as Message[]
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
    const chip = document.querySelector('.nm-mention[data-id="d1"]')
    expect(chip).toBeTruthy()
    expect(chip!.textContent).toContain('摄像头')
  })
})
