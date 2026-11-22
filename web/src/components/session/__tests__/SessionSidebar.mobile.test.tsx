/// Mobile workspace drawer — flat destinations, recent list, floating glass Settings.
import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, screen, fireEvent } from '@testing-library/react'
import { MemoryRouter } from 'react-router-dom'
import { SessionSidebar } from '@/components/session/SessionSidebar'
import { useStore } from '@/store'

const navigateMock = vi.fn()
const openSettingsMock = vi.fn()

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
    openSettingsMock.mockReset()
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
      openSettings: openSettingsMock,
    } as never)
  })

  it('renders flat destinations and floating avatar Settings (no search/new)', () => {
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
    expect(screen.getByTestId('session-drawer-shortcut-devices')).toBeTruthy()
    expect(screen.getByTestId('session-drawer-shortcut-agents')).toBeTruthy()
    expect(screen.getByTestId('session-drawer-shortcut-dashboard')).toBeTruthy()
    expect(screen.getByTestId('session-drawer-shortcut-messages')).toBeTruthy()
    expect(screen.queryByTestId('session-drawer-shortcut-automation')).toBeNull()
    for (const id of ['devices', 'agents', 'dashboard', 'messages']) {
      expect(screen.getByTestId(`session-drawer-shortcut-${id}`).querySelectorAll('svg').length).toBe(1)
    }
    expect(screen.getByTestId('session-drawer-workspace')).toBeTruthy()
    expect(screen.queryByTestId('session-drawer-new-chat')).toBeNull()
    expect(screen.queryByTestId('session-drawer-search')).toBeNull()
    const settings = screen.getByTestId('session-drawer-settings')
    expect(settings).toBeTruthy()
    expect(settings.textContent).toMatch(/uiadmin/)
    expect(settings.className).toMatch(/backdrop-blur|bg-surface-glass|glass/)
    expect(settings.className).not.toMatch(/border-t/)
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

  it('opens the full workspace hub from the All destinations row', () => {
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

  it('opens Settings from the avatar footer and closes the drawer', () => {
    const onClose = vi.fn()
    render(
      <MemoryRouter>
        <SessionSidebar open onClose={onClose} isDesktop={false} />
      </MemoryRouter>,
    )

    fireEvent.click(screen.getByTestId('session-drawer-settings'))
    expect(onClose).toHaveBeenCalledTimes(1)
    expect(openSettingsMock).toHaveBeenCalledTimes(1)
  })
})
