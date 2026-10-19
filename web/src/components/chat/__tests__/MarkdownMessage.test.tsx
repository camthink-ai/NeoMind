/// Rendering tests for MarkdownMessage chrome: code block controls, long-code
/// expansion, variant-scoped inline code, and table overflow wrapping.
import { describe, it, expect } from 'vitest'
import { render, screen, fireEvent } from '@testing-library/react'
import { MarkdownMessage } from '../MarkdownMessage'

describe('MarkdownMessage', () => {
  it('renders a code block with language, wrap, and copy controls', () => {
    render(<MarkdownMessage content={'```ts\nconst x = 1\n```'} />)
    expect(screen.getByText('ts')).toBeTruthy()
    expect(screen.getByRole('button', { name: 'code.wrap' })).toBeTruthy()
    expect(screen.getByRole('button', { name: 'code.copy' })).toBeTruthy()
  })

  it('offers expand/collapse for a long code block', () => {
    const code = Array.from({ length: 20 }, (_, i) => `line ${i}`).join('\n')
    render(<MarkdownMessage content={'```\n' + code + '\n```'} />)
    const expand = screen.getByRole('button', { name: 'code.expand' })
    fireEvent.click(expand)
    expect(screen.getByRole('button', { name: 'code.collapse' })).toBeTruthy()
  })

  it('uses the variant-scoped inline code surface', () => {
    const { container } = render(
      <MarkdownMessage content={'run `neomind` now'} variant="user" />
    )
    const code = container.querySelector('code')
    expect(code?.className).toContain('bg-[var(--inline-code-bg)]')
    expect(code?.className).not.toContain('bg-muted')
  })

  it('keeps prose on the same column measure as the process card', () => {
    const { container } = render(<MarkdownMessage content={'一段回答'} />)
    const prose = container.querySelector('.prose')
    // A paragraph-level max-width made the answer narrower than the full-width
    // execution card above it.
    expect(prose?.className).not.toContain('prose-p:max-w-prose')
    expect(prose?.className).not.toContain('prose-li:max-w-prose')
  })

  it('wraps tables for horizontal scrolling without boxing them', () => {
    const { container } = render(
      <MarkdownMessage content={'| a | b |\n| --- | --- |\n| 1 | 2 |'} />
    )
    const wrapper = container.querySelector('table')?.parentElement
    expect(wrapper?.className).toContain('overflow-x-auto')
    expect(wrapper?.className).not.toContain('rounded-xl')
    expect(wrapper?.className).not.toContain('border')

    const table = container.querySelector('table')
    // Row separation is carried by the horizontal rules, not zebra stripes.
    expect(table?.className).not.toContain('nth-child')

    const prose = container.querySelector('.prose')
    expect(prose?.className).toContain('prose-th:border-b')
    expect(prose?.className).not.toContain('prose-th:border-b-2')
    expect(prose?.className).toContain('prose-td:border-border')
  })

  it('renders a streaming caret only while streaming', () => {
    const { container, rerender } = render(<MarkdownMessage content="回答" isStreaming />)
    expect(container.querySelector('.animate-blink')).toBeTruthy()
    rerender(<MarkdownMessage content="回答" />)
    expect(container.querySelector('.animate-blink')).toBeNull()
  })
})
