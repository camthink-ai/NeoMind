/// Smoke tests for PageTabsBar — guards the toolbar-row contract:
/// capsule styling (border + bg-card, no wrapping), black active tab,
/// tab content left-aligned.
import { describe, it, expect, vi } from 'vitest'
import { render, screen } from '@testing-library/react'
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
  it('renders every tab label as a button', () => {
    renderBar()
    expect(screen.getByRole('button', { name: 'Tab A' })).toBeInTheDocument()
    expect(screen.getByRole('button', { name: 'Tab B' })).toBeInTheDocument()
  })

  it('capsule keeps the bordered card surface (no muted-30 regression)', () => {
    const { container } = renderBar()
    const capsule = container.querySelector('.overflow-x-auto')
    expect(capsule).toBeTruthy()
    expect(capsule!.className).toContain('border')
    expect(capsule!.className).toContain('bg-card')
    expect(capsule!.className).not.toContain('bg-muted-30')
  })

  it('the toolbar row carries no bottom border (fewer-lines rule)', () => {
    const { container } = renderBar()
    const row = capsuleRow(container)
    expect(row).toBeTruthy()
    expect(row!.className).not.toContain('border-b')
  })

  it('active tab is the black-on-white state', () => {
    renderBar('b')
    const active = screen.getByRole('button', { name: 'Tab B' })
    expect(active.className).toContain('bg-foreground')
    expect(active.className).toContain('text-background')
  })

  it('tab buttons left-align their content', () => {
    renderBar()
    const btn = screen.getByRole('button', { name: 'Tab A' })
    expect(btn.className).toContain('justify-start')
  })
})

function capsuleRow(container: HTMLElement) {
  // the capsule's parent is the toolbar row
  const capsule = container.querySelector('.overflow-x-auto')
  return capsule ? capsule.parentElement : null
}
