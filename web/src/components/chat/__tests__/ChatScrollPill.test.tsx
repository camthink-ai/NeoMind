/// Rendering tests for the floating scroll-to-latest pill.
import { describe, it, expect, vi } from 'vitest'
import { render, screen, fireEvent } from '@testing-library/react'
import { ChatScrollPill } from '../ChatScrollPill'

describe('ChatScrollPill', () => {
  it('is hidden from keyboard navigation when not visible', () => {
    render(<ChatScrollPill visible={false} onClick={() => {}} />)
    const button = screen.getByRole('button', { name: 'scrollToBottom' })
    expect(button.getAttribute('tabindex')).toBe('-1')
    expect(button.className).toContain('pointer-events-none')
  })

  it('announces new content and fires the scroll callback', () => {
    const onClick = vi.fn()
    render(<ChatScrollPill visible hasNewContent onClick={onClick} />)
    const button = screen.getByRole('button', { name: 'newContent' })
    expect(button.className).toContain('opacity-100')
    fireEvent.click(button)
    expect(onClick).toHaveBeenCalledTimes(1)
  })
})
