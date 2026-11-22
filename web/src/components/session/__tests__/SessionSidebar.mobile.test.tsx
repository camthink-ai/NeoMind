/// Mobile workspace drawer — 4 icon tiles, recent list, full-width glass avatar bar.
import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, screen, fireEvent } from '@testing-library/react'
import { MemoryRouter } from 'react-router-dom'
import { SessionSidebar } from '@/components/session/SessionSidebar'
import { useStore } from '@/store'

const navigateMock = vi.fn()

vi.mock('react-router-dom', async () => {
  const actual = await vi.importActual<typeof import('react-router-dom')>('react-router-dom')
  return {
    ...actual,
    useNavigate: () => navigateMock,
    useParams: () => ({ sessionId: 's1' }),
  }
})

vi.mock('@/lib/api/sessions', () => ({
  sessionsApi: {
    listPendingSessions: vi.fn(async () => ({ sessionIds: [] })),
  },
}))

describe('SessionSidebar mobile workspace drawer', () => {
  beforeEach(() => {
    navigateMock.mockReset()
    useStore.setState({
      sessions: [
        {
          sessionId: 's1',
          title: '温室巡检',
          preview: '',
          createdAt: Date.now(),
          updatedAt: Date.now(),
          messageCount: 3,
        },
      ] as never,
      sessionsHasMore: false,
      sessionsLoading: false,
      sessionId: 's1',
      createSession: vi.fn(),
      deleteSession: vi.fn(),
      updateSessionTitle: vi.fn(),
      loadMoreSessions: vi.fn(),
      user: { username: 'uiadmin', role: 'admin' } as never,
    } as never)
  })

  it('renders 4 icon tiles and avatar bar with All destinations (no Settings)', () => {
    const onClose = vi.fn()
    render(
      <MemoryRouter>
        <SessionSidebar open onClose={onClose} isDesktop={false} />
      </MemoryRouter>,
    )

    const drawer = screen.getByTestId('session-workspace-drawer')
    expect(drawer.className).toContain('w-full')
    expect(drawer.style.height).toContain('--app-height')
    expect(screen.getByText('NeoMind')).toBeTruthy()
    expect(screen.getByText('温室巡检')).toBeTruthy()
    for (const id of ['devices', 'agents', 'dashboard', 'messages']) {
      const tile = screen.getByTestId(`session-drawer-shortcut-${id}`)
      expect(tile.querySelectorAll('svg').length).toBe(1)
      expect(tile.className).toMatch(/flex-col/)
    }
    expect(screen.queryByTestId('session-drawer-shortcut-automation')).toBeNull()
    expect(screen.queryByTestId('session-drawer-new-chat')).toBeNull()
    expect(screen.queryByTestId('session-drawer-search')).toBeNull()
    expect(screen.queryByTestId('session-drawer-settings')).toBeNull()

    const bar = screen.getByTestId('session-drawer-userbar')
    expect(bar.className).toMatch(/bg-surface-glass|backdrop-blur/)
    expect(bar.className).not.toMatch(/border-t/)
    expect(bar.textContent).toMatch(/uiadmin/)
    expect(bar.textContent).not.toMatch(/Settings|设置|nav\.settings/)
    expect(bar.querySelector('[class*="bg-primary"]')).toBeTruthy()

    const workspace = screen.getByTestId('session-drawer-workspace')
    expect(workspace.textContent).toMatch(/全部入口|All destinations|system\.moreEntries/)
    expect(bar.contains(workspace)).toBe(true)
  })

  it('navigates from a top shortcut after closing the drawer', () => {
    const onClose = vi.fn()
    render(
      <MemoryRouter>
        <SessionSidebar open onClose={onClose} isDesktop={false} />
      </MemoryRouter>,
    )

    fireEvent.click(screen.getByTestId('session-drawer-shortcut-devices'))
    expect(onClose).toHaveBeenCalledTimes(1)
    expect(navigateMock).toHaveBeenCalledWith('/devices')
  })

  it('opens the full workspace hub from All destinations on the avatar bar', () => {
    const onClose = vi.fn()
    render(
      <MemoryRouter>
        <SessionSidebar open onClose={onClose} isDesktop={false} />
      </MemoryRouter>,
    )

    fireEvent.click(screen.getByTestId('session-drawer-workspace'))
    expect(onClose).toHaveBeenCalledTimes(1)
    expect(navigateMock).toHaveBeenCalledWith('/system')
  })
})
