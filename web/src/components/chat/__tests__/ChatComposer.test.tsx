/// Interaction tests for ChatComposer — controlled input, send gating, and
/// the streaming cancel button.
import { describe, it, expect, vi } from 'vitest'
import { render, screen, fireEvent, waitFor, within } from '@testing-library/react'
import { ChatComposer } from '../ChatComposer'
import { Confirmer } from '@/components/ui/confirmer'

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

/// Context usage card actions — the manual compact/clear affordances added
/// with the context-compression control. The card lives inside a Radix
/// tooltip; open it via focus and drive the two buttons.
describe('ChatComposer context actions', () => {
  const usage = { used: 4000, max: 8000 }

  function setupCard(overrides: Partial<React.ComponentProps<typeof ChatComposer>> = {}) {
    const onCompact = vi.fn()
    const onClearContext = vi.fn()
    render(
      <>
        <ChatComposer
          value=""
          onChange={vi.fn()}
          onSend={vi.fn()}
          contextUsage={usage}
          onCompact={onCompact}
          onClearContext={onClearContext}
          {...overrides}
        />
        {/* The card actions go through the global Confirmer before firing —
            mount it so the confirmation dialog is reachable in these tests. */}
        <Confirmer />
      </>,
    )
    return { onCompact, onClearContext }
  }

  // Locale-agnostic: the t() defaultValues are English, an initialized i18n
  // instance would return the real translations — match either.
  // Match the aria-label by its stable shape — an i18n-backed run yields the
  // translated title, an uninitialized t() yields the key itself.
  const findUsageTrigger = () =>
    screen
      .getAllByRole('button')
      .find((b) => /context\.title|上下文占用/i.test(b.getAttribute('aria-label') || ''))!

  // Radix renders a hidden measurement clone of the tooltip content next
  // to the real one — collect every match and let callers use them as a set.
  async function compactButtons() {
    await screen.findAllByText(/compact|立即压缩/i, {}, { timeout: 2000 })
    return screen
      .getAllByText(/compact|立即压缩/i)
      .map((el) => el.closest('button'))
      .filter((b): b is HTMLButtonElement => !!b)
  }

  async function openCard() {
    const trigger = findUsageTrigger()
    expect(trigger).toBeTruthy()
    fireEvent.focus(trigger)
    const buttons = await compactButtons()
    expect(buttons.length).toBeGreaterThan(0)
    return buttons
  }

  /// The confirmation dialog's footer: [cancel, confirm]. Radix AlertDialog
  /// content carries role="alertdialog", which distinguishes it from the
  /// card buttons underneath.
  async function dialogButtons(): Promise<HTMLButtonElement[]> {
    const dlg = await screen.findByRole('alertdialog')
    return within(dlg)
      .getAllByRole('button')
      .map((b) => b as HTMLButtonElement)
  }

  it('fires onCompact only after the confirmation dialog is accepted', async () => {
    const { onCompact } = setupCard()
    const buttons = await openCard()
    buttons[0].click()
    // Dialog is up, action not fired yet.
    const [cancel, confirm] = await dialogButtons()
    expect(confirm).toBeTruthy()
    expect(onCompact).not.toHaveBeenCalled()
    confirm.click()
    await waitFor(() => expect(onCompact).toHaveBeenCalledTimes(1))
    expect(cancel).toBeTruthy()
  })

  it('does not fire onCompact when the confirmation dialog is dismissed', async () => {
    const { onCompact } = setupCard()
    const buttons = await openCard()
    buttons[0].click()
    const [cancel] = await dialogButtons()
    cancel.click()
    await waitFor(() => expect(screen.queryByRole('alertdialog')).toBeNull())
    expect(onCompact).not.toHaveBeenCalled()
  })

  it('fires onClearContext only after the confirmation dialog is accepted', async () => {
    const { onClearContext } = setupCard()
    await openCard() // waits for the card to be open
    const clear = await screen.findAllByText(/clear|清空对话/i)
    clear[0].closest('button')!.click()
    const [, confirm] = await dialogButtons()
    confirm.click()
    await waitFor(() => expect(onClearContext).toHaveBeenCalledTimes(1))
  })

  it('disables the compact button while compacting', async () => {
    const { onCompact } = setupCard({ compacting: true })
    const buttons = await openCard()
    // Every rendered copy (real + measurement clone) reflects the state.
    for (const b of buttons) {
      expect(b).toBeDisabled()
    }
    buttons[0].click()
    expect(onCompact).not.toHaveBeenCalled()
  })

  it('hides the actions row when no handlers are provided', async () => {
    render(
      <ChatComposer
        value=""
        onChange={vi.fn()}
        onSend={vi.fn()}
        contextUsage={usage}
      />,
    )
    const trigger = findUsageTrigger()
    fireEvent.focus(trigger)
    // Give Radix a beat; the row must never appear.
    await new Promise((r) => setTimeout(r, 250))
    expect(screen.queryByText(/compact|立即压缩/i)).toBeNull()
    expect(screen.queryByText(/clear|清空对话/i)).toBeNull()
  })
})
