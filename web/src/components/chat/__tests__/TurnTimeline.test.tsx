/// Tests for the round-based execution timeline: compact R prefixes, muted
/// inline intermediate notes, scannable tool rows, and structured delivery.
import { describe, it, expect } from 'vitest'
import { render, screen, fireEvent } from '@testing-library/react'
import { TurnTimeline } from '../TurnTimeline'
import type { ToolCall } from '@/types'

const toolCalls: ToolCall[] = [
  {
    id: 't1',
    name: 'shell',
    arguments: { command: 'neomind device list', description: '读取 INK 的设备信息' },
    result: 'ok',
    round: 1,
    durationMs: 36,
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
  it('expands into compact R prefixes with collapsed thinking and inline notes', () => {
    render(
      <TurnTimeline
        toolCalls={toolCalls}
        roundThinking={{ 1: '第一轮思考内容较长一些', 2: '第二轮思考' }}
        roundContents={{ 1: '我接下来会检查设备' }}
      />
    )
    expect(screen.getByText(/chat:timeline\.title/)).toBeTruthy()
    fireEvent.click(screen.getByRole('button', { name: /chat:timeline\.title/ }))
    expect(screen.getByText('R1')).toBeTruthy()
    expect(screen.getByText('R2')).toBeTruthy()
    // R labels sit beside the first row — not a lone dedicated button row.
    expect(screen.queryByRole('button', { name: /^R1/ })).toBeNull()
    // Thinking stays collapsed by default — body not visible, summary is.
    expect(screen.queryByText('第一轮思考内容较长一些')).toBeNull()
    expect(screen.getAllByText(/thinking\.inlineTitle/).length).toBeGreaterThanOrEqual(1)
    // Intermediate notes render directly (muted), not behind a disclosure.
    expect(screen.getByText('我接下来会检查设备')).toBeTruthy()
    expect(screen.getByTestId('round-intermediate').className).toContain('text-muted-foreground')
    expect(screen.queryByText('toolCall.intermediate')).toBeNull()
    // Tool rows prefer the readable description.
    expect(screen.getByText('读取 INK 的设备信息')).toBeTruthy()
    expect(screen.getByText('device.query_temperature')).toBeTruthy()
  })

  it('keeps a completed process collapsed and expands it from its summary header', () => {
    render(<TurnTimeline toolCalls={toolCalls} />)
    expect(screen.queryByText('R1')).toBeNull()
    fireEvent.click(screen.getByRole('button', { name: /chat:timeline\.title/ }))
    expect(screen.getByText('R1')).toBeTruthy()
  })

  it('shows a short failure reason on the tool row when available', () => {
    render(
      <TurnTimeline
        toolCalls={[
          {
            ...toolCalls[0],
            success: false,
            result: { error: 'device offline' },
          },
        ]}
      />
    )
    fireEvent.click(screen.getByRole('button', { name: /chat:timeline\.title/ }))
    expect(screen.getByText('device offline')).toBeTruthy()
  })

  it('shows structured delivery subtitle only when delivery fields exist', () => {
    render(
      <TurnTimeline
        toolCalls={[
          {
            id: 't-ctrl',
            name: 'shell',
            arguments: {
              command: 'neomind device control INK render_text',
              description: '向 INK 下发显示指令',
            },
            result: {
              exit_code: 0,
              stdout: JSON.stringify({
                sent: true,
                delivery: 'adapter_accepted',
                delivery_note: 'broker publish; device acknowledgment: none',
              }),
            },
            round: 1,
            durationMs: 120,
          },
        ]}
      />
    )
    fireEvent.click(screen.getByRole('button', { name: /chat:timeline\.title/ }))
    expect(screen.getByTestId('tool-delivery-subtitle')).toBeTruthy()
    expect(screen.getByText('devices:commandDelivery.summaryAdapter')).toBeTruthy()
  })

  it('does not invent a delivery subtitle from bare success', () => {
    render(
      <TurnTimeline
        toolCalls={[
          {
            id: 't-ok',
            name: 'shell',
            arguments: { command: 'neomind device list' },
            result: { exit_code: 0, stdout: '[]' },
            round: 1,
          },
        ]}
      />
    )
    fireEvent.click(screen.getByRole('button', { name: /chat:timeline\.title/ }))
    expect(screen.queryByTestId('tool-delivery-subtitle')).toBeNull()
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
    fireEvent.click(screen.getByRole('button', { name: /chat:timeline\.title/ }))
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
    fireEvent.click(screen.getByRole('button', { name: /chat:timeline\.title/ }))
    expect(screen.queryByText(/最终回答/)).toBeNull()
    expect(screen.queryByTestId('round-intermediate')).toBeNull()
  })
})
