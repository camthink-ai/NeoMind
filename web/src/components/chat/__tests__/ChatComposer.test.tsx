/// Interaction tests for ChatComposer — controlled input, send gating, and
/// the streaming cancel button.
import { describe, it, expect, vi } from 'vitest'
import { render, screen, fireEvent } from '@testing-library/react'
import { ChatComposer } from '../ChatComposer'

function setup(overrides: Partial<React.ComponentProps<typeof ChatComposer>> = {}) {
  const onSend = vi.fn()
  const onCancel = vi.fn()
  const onChange = vi.fn()
  render(
    <ChatComposer
      value=""
      onChange={onChange}
      onSend={onSend}
      onCancel={onCancel}
      {...overrides}
    />,
  )
  return { onSend, onCancel, onChange }
}

describe('ChatComposer', () => {
  it('renders a controlled textarea wired to onChange', () => {
    const { onChange } = setup({ value: 'hello' })
    const ta = screen.getByRole('textbox') as HTMLTextAreaElement
    expect(ta.value).toBe('hello')
    fireEvent.change(ta, { target: { value: 'x' } })
    expect(onChange).toHaveBeenCalledWith('x')
  })

  it('shows the send button when idle and fires onSend', () => {
    const { onSend } = setup({ value: 'go' })
    // The send button is the last enabled icon button; find by ArrowUp svg parent
    const btn = screen.getAllByRole('button').find((b) => b.querySelector('svg'))!
    expect(btn).toBeTruthy()
    btn.click()
    // value non-empty -> canSend true -> fired
    expect(onSend).toHaveBeenCalledTimes(1)
  })

  it('switches to the cancel control while streaming and fires onCancel', () => {
    const { onSend, onCancel } = setup({ value: 'go', isStreaming: true })
    const btn = screen.getAllByRole('button').find((b) => b.querySelector('svg'))!
    btn.click()
    expect(onCancel).toHaveBeenCalledTimes(1)
    expect(onSend).not.toHaveBeenCalled()
  })
})
