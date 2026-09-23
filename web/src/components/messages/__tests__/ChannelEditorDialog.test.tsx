/// Render tests for the channel editor.
///
/// This dialog earns a render test because the messages page mounts it on
/// every render: anything that throws inside it is caught by the page's error
/// boundary and the whole page collapses to "Something went wrong". That is
/// exactly what the vendor-doc lookup did when it was derived above the
/// `channelType` state it reads — the read sits inside a `.find()` callback,
/// which `tsc` does not flag as use-before-declaration, but which runs during
/// render while the `const` is still uninitialized (temporal dead zone, which
/// Vite surfaces as "Cannot access uninitialized variable").
import { describe, it, expect, vi } from 'vitest'
import { render, screen, fireEvent } from '@testing-library/react'
import { ChannelEditorDialog } from '@/components/messages/ChannelEditorDialog'

function renderEditor() {
  return render(
    <ChannelEditorDialog open onOpenChange={vi.fn()} editingChannel={null} onSaved={vi.fn()} />,
  )
}

describe('ChannelEditorDialog', () => {
  it('renders without throwing — the messages page mounts it unconditionally', () => {
    expect(() => renderEditor()).not.toThrow()
  })

  it('shows the vendor guide for the selected channel type', () => {
    renderEditor()
    // The default type is webhook, whose guide is the NeoMind notifications page.
    // Asserted by i18n key: the global react-i18next mock returns keys (see
    // src/test/setup.ts), including for the string-`defaultValue` call form.
    const link = screen.getByRole('link', { name: 'messages.channels.howToGet' })
    expect(link.getAttribute('href')).toContain('wiki.camthink.ai')
    expect(link).toHaveAttribute('target', '_blank')
    expect(link).toHaveAttribute('rel', expect.stringContaining('noopener'))
  })

  /// The three robots whose console only shows a full webhook address must ask
  /// for exactly that. They used to ask for a "Hook ID"/"key"/"access_token"
  /// that appears nowhere in the vendor's UI, so the only copyable value was
  /// silently rejected at send time.
  it('asks for the whole webhook address for the robot channel types', () => {
    renderEditor()
    const cases = [
      ['messages.channels.typeFeishu', 'open.feishu.cn'],
      ['messages.channels.typeWeCom', 'qyapi.weixin.qq.com'],
      ['messages.channels.typeDingTalk', 'oapi.dingtalk.com'],
    ]
    for (const [typeLabel, host] of cases) {
      fireEvent.click(screen.getByRole('button', { name: new RegExp(typeLabel) }))
      expect(
        screen.getByPlaceholderText(new RegExp(host)),
        `${typeLabel} must ask for the address the console shows`,
      ).toBeInTheDocument()
    }
  })
})
