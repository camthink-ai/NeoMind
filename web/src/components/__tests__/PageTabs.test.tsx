/// Smoke tests for PageTabsBar — guards the toolbar-row contract:
/// underline styling (no card surface, no full-width divider — the active
/// tab's own underline is the only line), text + underline active tab,
/// tab content left-aligned.
import { describe, it, expect, vi } from 'vitest'
import { render, screen, fireEvent } from '@testing-library/react'
import { PageTabsBar } from '@/components/shared/PageTabs'
import { ThemeProvider } from '@/components/ui/theme'

vi.mock('@/hooks/useMobile', () => ({
  useIsMobile: () => false,
  useSafeAreaInsets: () => ({ top: 0, bottom: 0, left: 0, right: 0 }),
}))

const tabs = [
  { value: 'a', label: 'Tab A' },
  { value: 'b', label: 'Tab B' },
]

function renderBar(active = 'a') {
  return render(
    <ThemeProvider>
      <PageTabsBar tabs={tabs} activeTab={active} onTabChange={vi.fn()} />
    </ThemeProvider>
  )
}

describe('PageTabsBar (desktop)', () => {
  it('renders every tab label as a tab', () => {
    renderBar()
    expect(screen.getByRole('tab', { name: 'Tab A' })).toBeInTheDocument()
    expect(screen.getByRole('tab', { name: 'Tab B' })).toBeInTheDocument()
    // APG: exactly one tab stop — the active tab
    expect(screen.getByRole('tab', { name: 'Tab A' }).tabIndex).toBe(0)
    expect(screen.getByRole('tab', { name: 'Tab B' }).tabIndex).toBe(-1)
  })

  it('tab strip drops the capsule card surface (underline style)', () => {
    const { container } = renderBar()
    const strip = container.querySelector('.overflow-x-auto')
    expect(strip).toBeTruthy()
    expect(strip!.className).not.toContain('bg-card')
    expect(strip!.className).not.toContain('bg-muted-30')
  })

  it('the toolbar row carries no full-width divider', () => {
    const { container } = renderBar()
    const row = stripRow(container)
    expect(row).toBeTruthy()
    expect(row!.className).not.toContain('border-b')
  })

  it('active tab is text + solid underline (no pill background)', () => {
    renderBar('b')
    const active = screen.getByRole('tab', { name: 'Tab B' })
    expect(active.className).toContain('text-foreground')
    expect(active.className).toContain('after:bg-foreground')
    // the pill background is gone (after:bg-foreground is the underline bar,
    // not the button's own background — match on the class boundary)
    expect(active.className).not.toMatch(/(^|\s)bg-foreground/)
  })

  it('inactive tab has a transparent underline', () => {
    renderBar('b')
    const inactive = screen.getByRole('tab', { name: 'Tab A' })
    expect(inactive.className).toContain('after:bg-transparent')
  })

  it('tab buttons left-align their content', () => {
    renderBar()
    const btn = screen.getByRole('tab', { name: 'Tab A' })
    expect(btn.className).toContain('justify-start')
  })

  it('ArrowRight activates the next tab (APG automatic activation)', () => {
    // Controlled component: assert the activation callback, not state
    const onTabChange = vi.fn()
    render(
      <ThemeProvider>
        <PageTabsBar tabs={tabs} activeTab="a" onTabChange={onTabChange} />
      </ThemeProvider>
    )
    fireEvent.keyDown(screen.getByRole('tab', { name: 'Tab A' }), { key: 'ArrowRight' })
    expect(onTabChange).toHaveBeenCalledWith('b')
  })
})

function stripRow(container: HTMLElement) {
  // the strip's parent is the toolbar row
  const strip = container.querySelector('.overflow-x-auto')
  return strip ? strip.parentElement : null
}
