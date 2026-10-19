/// Rendering tests for ThinkingBlock — the collapsible reasoning surface and
/// its warm card tokens. The global i18n mock returns raw keys, so assertions
/// target keys rather than translated copy.
import { describe, it, expect } from 'vitest'
import { render, screen, fireEvent } from '@testing-library/react'
import { ThinkingBlock } from '../ThinkingBlock'

describe('ThinkingBlock', () => {
  it('renders as a token-backed collapsible card', () => {
    render(<ThinkingBlock thinking="分析设备状态" />)
    const trigger = screen.getByRole('button')
    expect(trigger.getAttribute('aria-expanded')).toBe('false')
    expect(trigger.parentElement?.className).toContain('bg-muted-20')
    expect(trigger.parentElement?.className).toContain('border-chat-card-border')
    expect(trigger.parentElement?.className).not.toContain('bg-chat-thinking-bg')
    expect(trigger.parentElement?.className).not.toContain('border-l-')
    // Collapsed content is unmounted by Radix.
    expect(screen.queryByText('分析设备状态')).toBeNull()
  })

  it('expands and collapses with aria-expanded reflecting state', () => {
    render(<ThinkingBlock thinking="分析设备状态" />)
    const trigger = screen.getByRole('button')
    fireEvent.click(trigger)
    expect(trigger.getAttribute('aria-expanded')).toBe('true')
    expect(screen.getByText('分析设备状态')).toBeTruthy()
    fireEvent.click(trigger)
    expect(trigger.getAttribute('aria-expanded')).toBe('false')
  })

  it('shows the live thinking label while streaming', () => {
    render(<ThinkingBlock thinking="分析设备状态" isStreaming />)
    expect(screen.getByText('thinking.thinking')).toBeTruthy()
    expect(screen.queryByText('chat:thinking.characters')).toBeNull()
  })

  it('renders per-round labels when multiple rounds exist', () => {
    render(<ThinkingBlock roundThinking={{ 1: '第一轮', 2: '第二轮' }} />)
    fireEvent.click(screen.getByRole('button'))
    expect(screen.getByText('R1')).toBeTruthy()
    expect(screen.getByText('R2')).toBeTruthy()
  })

  it('does not draw an internal divider in inline (timeline) mode', () => {
    render(<ThinkingBlock thinking="思考内容" inline />)
    const trigger = screen.getByRole('button')
    expect(trigger.parentElement?.className).not.toContain('border-b')
    expect(trigger.parentElement?.className).not.toContain('border-t')
  })
})
