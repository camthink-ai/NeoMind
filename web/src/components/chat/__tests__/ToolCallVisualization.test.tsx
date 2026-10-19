/// Rendering tests for the tool-process card: summary text, live status,
/// failure styling, and the token-backed surface.
import { describe, it, expect } from 'vitest'
import { render, screen, fireEvent } from '@testing-library/react'
import { ToolProcessBlock } from '../ToolCallVisualization'
import type { ToolCall } from '@/types'

const completedCall: ToolCall = {
  id: 't1',
  name: 'shell',
  arguments: { command: 'neomind device list' },
  result: 'ok',
  round: 1,
}

describe('ToolProcessBlock', () => {
  it('renders a token-backed card with a plural-aware summary', () => {
    render(<ToolProcessBlock toolCalls={[completedCall]} />)
    const trigger = screen.getByRole('button', { name: /chat:toolCall\.callsCount/i })
    expect(trigger.parentElement?.className).toContain('bg-muted-20')
    expect(trigger.parentElement?.className).toContain('border-chat-card-border')
    expect(trigger.parentElement?.className).not.toContain('bg-chat-tool-bg')
    expect(trigger.parentElement?.className).not.toContain('border-l-')
    expect(screen.getByText('device list')).toBeTruthy()
  })

  it('names the running action while streaming', () => {
    render(
      <ToolProcessBlock
        toolCalls={[{ ...completedCall, result: null }]}
        isStreaming
      />
    )
    expect(screen.getByText(/chat:toolCall\.runningWith/)).toBeTruthy()
  })

  it('marks failed calls and exposes the details toggle', () => {
    render(
      <ToolProcessBlock
        toolCalls={[{ ...completedCall, success: false, result: { error: 'boom' } }]}
      />
    )
    expect(screen.getByText('toolCall.status.failed')).toBeTruthy()
    const details = screen.getByRole('button', { name: 'toolCall.toggleDetails' })
    expect(details.getAttribute('aria-expanded')).toBe('false')
    fireEvent.click(details)
    expect(details.getAttribute('aria-expanded')).toBe('true')
    expect(screen.getByText('toolCall.result')).toBeTruthy()
  })
})
