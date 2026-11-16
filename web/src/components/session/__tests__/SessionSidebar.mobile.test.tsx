/// Mobile workspace drawer — wider panel with chats + System hub exit.
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

  it('renders a wide workspace drawer with recent chats and a System exit', () => {
    const onClose = vi.fn()
    render(
      <MemoryRouter>
        <SessionSidebar open onClose={onClose} isDesktop={false} />
      </MemoryRouter>,
    )

    const drawer = screen.getByTestId('session-workspace-drawer')
    expect(drawer.className).toContain('w-[min(28rem,calc(100vw-2.75rem))]')
    expect(screen.getByText('NeoMind')).toBeTruthy()
    expect(screen.getByText('温室巡检')).toBeTruthy()
    expect(screen.getByTestId('session-drawer-workspace')).toBeTruthy()
  })

  it('closes then navigates to /system from the workspace footer', () => {
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
