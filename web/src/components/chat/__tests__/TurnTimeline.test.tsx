/// Tests for the round-based execution timeline: rounds keep their own
/// thinking/output/tools, collapse independently, and expose failures and
/// live status.
import { describe, it, expect } from 'vitest'
import { render, screen, fireEvent } from '@testing-library/react'
import { TurnTimeline } from '../TurnTimeline'
import type { ToolCall } from '@/types'

const toolCalls: ToolCall[] = [
  {
    id: 't1',
    name: 'shell',
    arguments: { command: 'neomind device list' },
    result: 'ok',
    round: 1,
  },
  {
    id: 't2',
    name: 'device.query_temperature',
    arguments: { device: 'temp-sensor-03', window: '1h' },
    result: 'ok',
    round: 2,
  },
]

describe('TurnTimeline', () => {
  it('groups thinking, output, and tools by round', () => {
    render(
      <TurnTimeline
        toolCalls={toolCalls}
        roundThinking={{ 1: '第一轮思考', 2: '第二轮思考' }}
        roundContents={{ 1: '中间输出内容' }}
      />
    )
    expect(screen.getByText(/chat:timeline\.title/)).toBeTruthy()
    expect(screen.getByText('R1')).toBeTruthy()
    expect(screen.getByText('R2')).toBeTruthy()
    expect(screen.getByText('第一轮思考')).toBeTruthy()
    expect(screen.getByText('第二轮思考')).toBeTruthy()
    expect(screen.getByText(/中间输出内容/)).toBeTruthy()
    // Intermediate output is content, not metadata: match the tool-name size.
    expect(screen.getByText(/中间输出内容/).className).toContain('text-sm')
    expect(screen.getByText('device list')).toBeTruthy()
    expect(screen.getByText('device.query_temperature')).toBeTruthy()
  })

  it('collapses a round without collapsing the whole process', () => {
    render(
      <TurnTimeline
        toolCalls={toolCalls}
        roundThinking={{ 1: '第一轮思考', 2: '第二轮思考' }}
      />
    )
    const round1 = screen.getByRole('button', { name: /R1/ })
    expect(round1.getAttribute('aria-expanded')).toBe('true')
    fireEvent.click(round1)
    expect(round1.getAttribute('aria-expanded')).toBe('false')
    expect(screen.queryByText('device list')).toBeNull()
    // The other round is still visible.
    expect(screen.getByText('device.query_temperature')).toBeTruthy()
  })

  it('collapses the whole process from its summary header', () => {
    render(<TurnTimeline toolCalls={toolCalls} />)
    fireEvent.click(screen.getByRole('button', { name: /chat:timeline\.title/ }))
    expect(screen.queryByText('R1')).toBeNull()
  })

  it('shows a failed tool inside its round', () => {
    render(
      <TurnTimeline
        toolCalls={[
          {
            ...toolCalls[0],
            success: false,
            result: { error: 'boom' },
          },
        ]}
      />
    )
    expect(screen.getByText('toolCall.status.failed')).toBeTruthy()
  })

  it('caps expanded JSON blocks so long base64 values cannot grow the timeline', () => {
    render(
      <TurnTimeline
        toolCalls={[
          {
            id: 't-long',
            name: 'vision.inspect',
            arguments: { image: `data:image/jpeg;base64,${'A'.repeat(2000)}` },
            result: 'ok',
            round: 1,
          },
        ]}
      />
    )
    const toolButton = screen
      .getAllByText('vision.inspect')
      .map((el) => el.closest('button'))
      .find((btn) => /toggleDetails|Toggle details|展开\/收起详情/i.test(btn?.getAttribute('aria-label') || ''))
    expect(toolButton).toBeTruthy()
    fireEvent.click(toolButton!)
    const blocks = document.querySelectorAll('pre')
    expect(blocks).toHaveLength(2)
    blocks.forEach((pre) => {
      expect(pre.className).toContain('max-h-40')
      expect(pre.className).toContain('overflow-y-auto')
      expect(pre.className).toContain('[overflow-wrap:anywhere]')
      expect(pre.className).not.toContain('break-words')
    })
  })

  it('shows the running status in the active streaming round', () => {
    render(
      <TurnTimeline
        toolCalls={[{ ...toolCalls[0], result: null }]}
        isStreaming
        currentRound={1}
      />
    )
    expect(screen.getByText('toolCall.status.running')).toBeTruthy()
  })

  it('does not echo the final answer as an intermediate round output', () => {
    render(
      <TurnTimeline
        toolCalls={toolCalls}
        roundContents={{ 1: '最终回答' }}
        finalContent="最终回答"
      />
    )
    expect(screen.queryByText(/最终回答/)).toBeNull()
  })
})
