/// Smoke tests for the AppSidebar rail — the app's primary navigation.
/// Guards the chrome contract: 8 named nav buttons, aria-current on the
/// active route, fixed rail surface.
import { describe, it, expect } from 'vitest'
import { render, screen } from '@testing-library/react'
import { MemoryRouter } from 'react-router-dom'
import { AppSidebar } from '@/components/layout/AppSidebar'
import { ThemeProvider } from '@/components/ui/theme'
import { useStore } from '@/store'

function renderSidebar(path = '/devices') {
  return render(
    <ThemeProvider>
      <MemoryRouter initialEntries={[path]}>
        <AppSidebar />
      </MemoryRouter>
    </ThemeProvider>
  )
}

describe('AppSidebar', () => {
  it('renders all 8 nav items as labeled buttons', () => {
    renderSidebar()
    for (const label of ['nav.dashboard', 'nav.agents', 'nav.devices', 'nav.visual-dashboard',
                         'nav.automation', 'nav.data', 'nav.messages', 'nav.extensions']) {
      expect(screen.getByRole('button', { name: label })).toBeInTheDocument()
    }
  })

  it('marks the active route with aria-current="page" (only one)', () => {
    renderSidebar('/devices')
    const active = screen.getAllByRole('button', { name: 'nav.devices' })[0]
    expect(active).toHaveAttribute('aria-current', 'page')
    const currentCount = document.querySelectorAll('[aria-current="page"]').length
    expect(currentCount).toBe(1)
  })

  it('prefix routes count as active (detail views)', () => {
    renderSidebar('/devices/verify-cam')
    const active = screen.getAllByRole('button', { name: 'nav.devices' })[0]
    expect(active).toHaveAttribute('aria-current', 'page')
  })

  it('renders the footer utilities (settings + avatar) when logged in', () => {
    useStore.setState({ user: { username: 'tester', role: 'admin' } as never })
    renderSidebar()
    expect(screen.getByRole('button', { name: 'nav.settings' })).toBeInTheDocument()
    expect(screen.getByRole('button', { name: 'tester' })).toBeInTheDocument()
  })

  it('exports the fixed rail width for fixed surfaces', () => {
    renderSidebar()
    expect(document.documentElement.style.getPropertyValue('--app-sidebar-width')).toBe('72px')
  })
})
