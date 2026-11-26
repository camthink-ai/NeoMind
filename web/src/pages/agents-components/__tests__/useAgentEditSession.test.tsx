/**
 * Race: edit A → cancel (stale id must not linger) → edit B with delayed GET.
 * Save must not PUT until B loads; then exactly one PUT to B.
 */
import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, screen, fireEvent, waitFor, act } from '@testing-library/react'
import type { AgentSpec, AgentSpecDraft } from '@/types'
import { AgentEditor } from '@/components/agents/editor'
import { useAgentEditSession } from '../useAgentEditSession'

vi.mock('react-i18next', async (importOriginal) => {
  const actual = await importOriginal<typeof import('react-i18next')>()
  return {
    ...actual,
    useTranslation: () => ({
      t: (key: string) => key,
      i18n: { language: 'en' },
    }),
  }
})

vi.mock('@/hooks/useMobile', () => ({
  useIsMobile: () => false,
  useSafeAreaInsets: () => ({ top: 0, bottom: 0, left: 0, right: 0 }),
}))

function makeSpec(id: string, name: string): AgentSpec {
  return {
    id,
    name,
    description: `${name}-desc`,
    user_prompt: `prompt for ${name}`,
    trigger: { kind: 'manual' },
    pacing: { failure_threshold: 3 },
    input: {
      requirements: [
        {
          key: 'sensor',
          source: { kind: 'whole_device', device_id: `${id}-dev` },
          params: {},
          timeout_secs: 10,
          result_type: 'structured_json',
          freshness: { kind: 'any' },
          on_missing: 'report_stale',
        },
      ],
    },
    tool_policy: 'disabled',
    output: {
      fields: [{ name: 'verdict', depends_on: ['sensor'], description: 'ok' }],
      publish: false,
    },
    notify: {
      channels: ['ch-1'],
      on_complete: false,
      on_alert: false,
      alert_depends_on: [],
      on_failure: true,
      pending_confirmation: false,
    },
    controlled_actions: [],
    config_version: 1,
    created_at: 1,
    updated_at: 1,
  }
}

function Harness({
  getAgent,
  updateAgent,
  createAgent,
}: {
  getAgent: (id: string) => Promise<AgentSpec>
  updateAgent: (id: string, patch: unknown) => Promise<unknown>
  createAgent: (draft: AgentSpecDraft) => Promise<unknown>
}) {
  const session = useAgentEditSession({
    getAgent,
    updateAgent,
    createAgent,
  })

  return (
    <div>
      <button type="button" onClick={() => void session.openEdit('A')}>
        edit-A
      </button>
      <button type="button" onClick={() => void session.openEdit('B')}>
        edit-B
      </button>
      <button type="button" onClick={() => session.close()}>
        cancel-session
      </button>
      <span data-testid="edit-session-agent-name">
        {session.editingAgent?.name ?? ''}
      </span>
      <span data-testid="edit-session-loading">
        {session.loading ? '1' : '0'}
      </span>
      <AgentEditor
        open={session.open}
        onOpenChange={session.onOpenChange}
        agent={session.editingAgent}
        loading={session.loading}
        onSave={session.save}
      />
    </div>
  )
}

describe('useAgentEditSession race with AgentEditor', () => {
  beforeEach(() => {
    vi.clearAllMocks()
  })

  it('delayed GET for B: zero PUT before load; after load exactly one PUT to B', async () => {
    const specA = makeSpec('A', 'agent-a')
    const specB = makeSpec('B', 'agent-b')

    let resolveB!: (value: AgentSpec) => void
    const getAgent = vi.fn(async (id: string) => {
      if (id === 'A') return specA
      return new Promise<AgentSpec>((resolve) => {
        resolveB = resolve
      })
    })
    const updateAgent = vi.fn(async () => ({ ok: true }))
    const createAgent = vi.fn(async () => ({ ok: true }))

    render(
      <Harness
        getAgent={getAgent}
        updateAgent={updateAgent}
        createAgent={createAgent}
      />,
    )

    const dialog = () => screen.getByRole('dialog', { hidden: true })

    // Open A, wait until save is enabled (loaded).
    fireEvent.click(screen.getByRole('button', { name: 'edit-A' }))
    await waitFor(() => {
      expect(dialog()).not.toHaveClass('hidden')
      expect(screen.getByTestId('agent-editor-save')).not.toBeDisabled()
    })
    expect(getAgent).toHaveBeenCalledWith('A')

    // Cancel — must clear editingAgent so a later save cannot PUT A.
    fireEvent.click(screen.getByRole('button', { name: 'common:cancel' }))
    await waitFor(() => {
      expect(dialog()).toHaveClass('hidden')
    })

    // Open B with delayed GET.
    fireEvent.click(screen.getByRole('button', { name: 'edit-B' }))
    await waitFor(() => {
      expect(dialog()).not.toHaveClass('hidden')
      expect(screen.getByTestId('agent-editor-loading')).toBeInTheDocument()
    })
    const saveWhileLoading = screen.getByTestId('agent-editor-save')
    expect(saveWhileLoading).toBeDisabled()
    fireEvent.click(saveWhileLoading)
    expect(updateAgent).not.toHaveBeenCalled()

    // B arrives — editor opens for B only.
    await act(async () => {
      resolveB(specB)
    })
    await waitFor(() => {
      expect(screen.queryByTestId('agent-editor-loading')).toBeNull()
      expect(screen.getByTestId('agent-editor-save')).not.toBeDisabled()
    })
    expect(updateAgent).not.toHaveBeenCalled()

    fireEvent.click(screen.getByTestId('agent-editor-save'))
    await waitFor(() => {
      expect(updateAgent).toHaveBeenCalledTimes(1)
    })
    expect(updateAgent).toHaveBeenCalledWith('B', expect.any(Object))
    const putIds = (updateAgent.mock.calls as unknown as [string, unknown][]).map(
      ([id]) => id,
    )
    expect(putIds).toEqual(['B'])
    expect(createAgent).not.toHaveBeenCalled()
  })

  it('ignores late GET after cancel during B load', async () => {
    const specB = makeSpec('B', 'agent-b')
    let resolveB!: (value: AgentSpec) => void
    const getAgent = vi.fn(async (id: string) => {
      if (id !== 'B') throw new Error(`unexpected id ${id}`)
      return new Promise<AgentSpec>((resolve) => {
        resolveB = resolve
      })
    })
    const updateAgent = vi.fn(async () => ({ ok: true }))
    const createAgent = vi.fn(async () => ({ ok: true }))

    render(
      <Harness
        getAgent={getAgent}
        updateAgent={updateAgent}
        createAgent={createAgent}
      />,
    )

    const dialog = () => screen.getByRole('dialog', { hidden: true })

    fireEvent.click(screen.getByRole('button', { name: 'edit-B' }))
    await waitFor(() => {
      expect(screen.getByTestId('agent-editor-loading')).toBeInTheDocument()
      expect(dialog()).not.toHaveClass('hidden')
    })

    fireEvent.click(screen.getByRole('button', { name: 'cancel-session' }))
    await waitFor(() => {
      expect(dialog()).toHaveClass('hidden')
    })

    await act(async () => {
      resolveB(specB)
    })

    // Late response must not reopen the editor.
    expect(dialog()).toHaveClass('hidden')
    expect(updateAgent).not.toHaveBeenCalled()
  })

  it('same-id reopen: stale GET₁ failure must not close while GET₂ pending', async () => {
    const specB = makeSpec('B', 'agent-b-fresh')
    const pending: Array<{
      resolve: (v: AgentSpec) => void
      reject: (e: unknown) => void
    }> = []
    const getAgent = vi.fn(async (id: string) => {
      if (id !== 'B') throw new Error(`unexpected id ${id}`)
      return new Promise<AgentSpec>((resolve, reject) => {
        pending.push({ resolve, reject })
      })
    })
    const updateAgent = vi.fn(async () => ({ ok: true }))
    const createAgent = vi.fn(async () => ({ ok: true }))
    const onLoadError = vi.fn()

    function HarnessWithError() {
      const session = useAgentEditSession({
        getAgent,
        updateAgent,
        createAgent,
        onLoadError,
      })
      return (
        <div>
          <button type="button" onClick={() => void session.openEdit('B')}>
            edit-B
          </button>
          <button type="button" onClick={() => session.close()}>
            cancel-session
          </button>
          <span data-testid="edit-session-loading">
            {session.loading ? '1' : '0'}
          </span>
          <AgentEditor
            open={session.open}
            onOpenChange={session.onOpenChange}
            agent={session.editingAgent}
            loading={session.loading}
            onSave={session.save}
          />
        </div>
      )
    }

    render(<HarnessWithError />)
    const dialog = () => screen.getByRole('dialog', { hidden: true })

    // GET₁
    fireEvent.click(screen.getByRole('button', { name: 'edit-B' }))
    await waitFor(() => {
      expect(pending).toHaveLength(1)
      expect(screen.getByTestId('edit-session-loading')).toHaveTextContent('1')
    })

    // Cancel while GET₁ pending, then reopen → GET₂
    fireEvent.click(screen.getByRole('button', { name: 'cancel-session' }))
    await waitFor(() => expect(dialog()).toHaveClass('hidden'))
    fireEvent.click(screen.getByRole('button', { name: 'edit-B' }))
    await waitFor(() => {
      expect(pending).toHaveLength(2)
      expect(dialog()).not.toHaveClass('hidden')
      expect(screen.getByTestId('edit-session-loading')).toHaveTextContent('1')
    })

    // Stale GET₁ fails — must not close GET₂'s waiting editor.
    await act(async () => {
      pending[0]!.reject(new Error('stale GET₁ failed'))
    })
    expect(dialog()).not.toHaveClass('hidden')
    expect(screen.getByTestId('edit-session-loading')).toHaveTextContent('1')
    expect(onLoadError).not.toHaveBeenCalled()

    // GET₂ succeeds — editor loads B.
    await act(async () => {
      pending[1]!.resolve(specB)
    })
    await waitFor(() => {
      expect(screen.getByTestId('edit-session-loading')).toHaveTextContent('0')
      expect(screen.getByTestId('agent-editor-save')).not.toBeDisabled()
    })
    expect(dialog()).not.toHaveClass('hidden')
  })

  it('same-id reopen: stale GET₁ success must not overwrite GET₂ draft', async () => {
    const stale = makeSpec('B', 'agent-b-stale')
    const fresh = makeSpec('B', 'agent-b-fresh')
    fresh.config_version = 2
    fresh.user_prompt = 'fresh prompt from GET₂'
    const pending: Array<{
      resolve: (v: AgentSpec) => void
      reject: (e: unknown) => void
    }> = []
    const getAgent = vi.fn(async (id: string) => {
      if (id !== 'B') throw new Error(`unexpected id ${id}`)
      return new Promise<AgentSpec>((resolve, reject) => {
        pending.push({ resolve, reject })
      })
    })

    render(
      <Harness
        getAgent={getAgent}
        updateAgent={vi.fn(async () => ({ ok: true }))}
        createAgent={vi.fn(async () => ({ ok: true }))}
      />,
    )
    const dialog = () => screen.getByRole('dialog', { hidden: true })

    fireEvent.click(screen.getByRole('button', { name: 'edit-B' }))
    await waitFor(() => expect(pending).toHaveLength(1))

    fireEvent.click(screen.getByRole('button', { name: 'cancel-session' }))
    await waitFor(() => expect(dialog()).toHaveClass('hidden'))
    fireEvent.click(screen.getByRole('button', { name: 'edit-B' }))
    await waitFor(() => {
      expect(pending).toHaveLength(2)
      expect(screen.getByTestId('edit-session-loading')).toHaveTextContent('1')
    })

    // Stale GET₁ success must not seed the editor.
    await act(async () => {
      pending[0]!.resolve(stale)
    })
    expect(screen.getByTestId('edit-session-agent-name')).toHaveTextContent('')
    expect(screen.getByTestId('edit-session-loading')).toHaveTextContent('1')
    expect(dialog()).not.toHaveClass('hidden')

    // GET₂ seeds the fresh draft only.
    await act(async () => {
      pending[1]!.resolve(fresh)
    })
    await waitFor(() => {
      expect(screen.getByTestId('edit-session-agent-name')).toHaveTextContent(
        'agent-b-fresh',
      )
      expect(screen.getByTestId('edit-session-loading')).toHaveTextContent('0')
    })
    expect(screen.getByTestId('edit-session-agent-name')).not.toHaveTextContent(
      'agent-b-stale',
    )
  })
})
