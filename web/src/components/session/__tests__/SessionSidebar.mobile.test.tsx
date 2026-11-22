/// Mobile workspace drawer — shortcuts on top, chats middle, search/new bottom.
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
    } as never)
  })

  it('renders top workspace shortcuts and bottom search/new controls', () => {
    const onClose = vi.fn()
    render(
      <MemoryRouter>
        <SessionSidebar open onClose={onClose} isDesktop={false} />
      </MemoryRouter>,
    )

    const drawer = screen.getByTestId('session-workspace-drawer')
    expect(drawer.className).toContain('w-full')
    expect(drawer.style.height).toContain('--app-height')
    expect(screen.getByText('温室巡检')).toBeTruthy()
    expect(screen.getByTestId('session-drawer-shortcut-devices')).toBeTruthy()
    expect(screen.getByTestId('session-drawer-shortcut-agents')).toBeTruthy()
    expect(screen.getByTestId('session-drawer-shortcut-dashboard')).toBeTruthy()
    expect(screen.getByTestId('session-drawer-shortcut-messages')).toBeTruthy()
    expect(screen.queryByTestId('session-drawer-shortcut-automation')).toBeNull()
    expect(screen.queryByTestId('session-drawer-shortcut-data')).toBeNull()
    expect(screen.queryByTestId('session-drawer-shortcut-extensions')).toBeNull()
    // Original four tiles keep Lucide icons.
    for (const id of ['devices', 'agents', 'dashboard', 'messages']) {
      expect(screen.getByTestId(`session-drawer-shortcut-${id}`).querySelectorAll('svg').length).toBe(1)
    }
    const workspace = screen.getByTestId('session-drawer-workspace')
    expect(workspace).toBeTruthy()
    // Avatar header absorbs All destinations (initials/NM + title + chevron).
    expect(workspace.textContent).toMatch(/NM|Workspace|工作台|全部入口|All destinations/)
    expect(workspace.querySelectorAll('svg').length).toBe(1)
    expect(screen.getByTestId('session-drawer-new-chat')).toBeTruthy()
    expect(screen.getByPlaceholderText(/search|搜索/i)).toBeTruthy()
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

  it('opens the full workspace hub from the avatar header', () => {
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

  it('keeps the search input mounted across successive edits (no remount)', () => {
    const onClose = vi.fn()
    render(
      <MemoryRouter>
        <SessionSidebar open onClose={onClose} isDesktop={false} />
      </MemoryRouter>,
    )

    const input = screen.getByTestId('session-drawer-search') as HTMLInputElement
    fireEvent.change(input, { target: { value: '温' } })
    expect(screen.getByTestId('session-drawer-search')).toBe(input)
    fireEvent.change(input, { target: { value: '温室' } })
    expect(screen.getByTestId('session-drawer-search')).toBe(input)
    expect(input.value).toBe('温室')
    fireEvent.change(input, { target: { value: '温' } })
    expect(screen.getByTestId('session-drawer-search')).toBe(input)
    expect(input.value).toBe('温')
  })
})
