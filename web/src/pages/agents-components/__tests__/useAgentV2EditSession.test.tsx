/**
 * Race: edit A → cancel (stale id must not linger) → edit B with delayed GET.
 * Save must not PUT until B loads; then exactly one PUT to B.
 */
import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, screen, fireEvent, waitFor, act } from '@testing-library/react'
import type { AgentSpec, AgentSpecDraft } from '@/types'
import { AgentEditorV2 } from '@/components/agents/editor-v2'
import { useAgentV2EditSession } from '../useAgentV2EditSession'

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
  getAgentV2,
  updateAgentV2,
  createAgentV2,
}: {
  getAgentV2: (id: string) => Promise<{ agent: AgentSpec }>
  updateAgentV2: (id: string, patch: unknown) => Promise<unknown>
  createAgentV2: (draft: AgentSpecDraft) => Promise<unknown>
}) {
  const session = useAgentV2EditSession({
    getAgentV2,
    updateAgentV2,
    createAgentV2,
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
      <AgentEditorV2
        open={session.open}
        onOpenChange={session.onOpenChange}
        agent={session.editingAgent}
        loading={session.loading}
        onSave={session.save}
      />
    </div>
  )
}

describe('useAgentV2EditSession race with AgentEditorV2', () => {
  beforeEach(() => {
    vi.clearAllMocks()
  })

  it('delayed GET for B: zero PUT before load; after load exactly one PUT to B', async () => {
    const specA = makeSpec('A', 'agent-a')
    const specB = makeSpec('B', 'agent-b')

    let resolveB!: (value: { agent: AgentSpec }) => void
    const getAgentV2 = vi.fn(async (id: string) => {
      if (id === 'A') return { agent: specA }
      return new Promise<{ agent: AgentSpec }>((resolve) => {
        resolveB = resolve
      })
    })
    const updateAgentV2 = vi.fn(async () => ({ ok: true }))
    const createAgentV2 = vi.fn(async () => ({ ok: true }))

    render(
      <Harness
        getAgentV2={getAgentV2}
        updateAgentV2={updateAgentV2}
        createAgentV2={createAgentV2}
      />,
    )

    const dialog = () => screen.getByRole('dialog', { hidden: true })

    // Open A, wait until save is enabled (loaded).
    fireEvent.click(screen.getByRole('button', { name: 'edit-A' }))
    await waitFor(() => {
      expect(dialog()).not.toHaveClass('hidden')
      expect(screen.getByTestId('agent-editor-v2-save')).not.toBeDisabled()
    })
    expect(getAgentV2).toHaveBeenCalledWith('A')

    // Cancel — must clear editingAgent so a later save cannot PUT A.
    // FullScreenDialog keeps children mounted with `hidden` when closed.
    fireEvent.click(screen.getByRole('button', { name: 'common:cancel' }))
    await waitFor(() => {
      expect(dialog()).toHaveClass('hidden')
    })

    // Open B with delayed GET.
    fireEvent.click(screen.getByRole('button', { name: 'edit-B' }))
    await waitFor(() => {
      expect(dialog()).not.toHaveClass('hidden')
      expect(screen.getByTestId('agent-editor-v2-loading')).toBeInTheDocument()
    })
    const saveWhileLoading = screen.getByTestId('agent-editor-v2-save')
    expect(saveWhileLoading).toBeDisabled()
    fireEvent.click(saveWhileLoading)
    expect(updateAgentV2).not.toHaveBeenCalled()

    // B arrives — editor opens for B only.
    await act(async () => {
      resolveB({ agent: specB })
    })
    await waitFor(() => {
      expect(screen.queryByTestId('agent-editor-v2-loading')).toBeNull()
      expect(screen.getByTestId('agent-editor-v2-save')).not.toBeDisabled()
    })
    expect(updateAgentV2).not.toHaveBeenCalled()

    fireEvent.click(screen.getByTestId('agent-editor-v2-save'))
    await waitFor(() => {
      expect(updateAgentV2).toHaveBeenCalledTimes(1)
    })
    expect(updateAgentV2).toHaveBeenCalledWith('B', expect.any(Object))
    const putIds = (updateAgentV2.mock.calls as unknown as [string, unknown][]).map(
      ([id]) => id,
    )
    expect(putIds).toEqual(['B'])
    expect(createAgentV2).not.toHaveBeenCalled()
  })

  it('ignores late GET after cancel during B load', async () => {
    const specB = makeSpec('B', 'agent-b')
    let resolveB!: (value: { agent: AgentSpec }) => void
    const getAgentV2 = vi.fn(async (id: string) => {
      if (id !== 'B') throw new Error(`unexpected id ${id}`)
      return new Promise<{ agent: AgentSpec }>((resolve) => {
        resolveB = resolve
      })
    })
    const updateAgentV2 = vi.fn(async () => ({ ok: true }))
    const createAgentV2 = vi.fn(async () => ({ ok: true }))

    render(
      <Harness
        getAgentV2={getAgentV2}
        updateAgentV2={updateAgentV2}
        createAgentV2={createAgentV2}
      />,
    )

    const dialog = () => screen.getByRole('dialog', { hidden: true })

    fireEvent.click(screen.getByRole('button', { name: 'edit-B' }))
    await waitFor(() => {
      expect(screen.getByTestId('agent-editor-v2-loading')).toBeInTheDocument()
      expect(dialog()).not.toHaveClass('hidden')
    })

    fireEvent.click(screen.getByRole('button', { name: 'cancel-session' }))
    await waitFor(() => {
      expect(dialog()).toHaveClass('hidden')
    })

    await act(async () => {
      resolveB({ agent: specB })
    })

    // Late response must not reopen the editor.
    expect(dialog()).toHaveClass('hidden')
    expect(updateAgentV2).not.toHaveBeenCalled()
  })

  it('same-id reopen: stale GET₁ failure must not close while GET₂ pending', async () => {
    const specB = makeSpec('B', 'agent-b-fresh')
    const pending: Array<{
      resolve: (v: { agent: AgentSpec }) => void
      reject: (e: unknown) => void
    }> = []
    const getAgentV2 = vi.fn(async (id: string) => {
      if (id !== 'B') throw new Error(`unexpected id ${id}`)
      return new Promise<{ agent: AgentSpec }>((resolve, reject) => {
        pending.push({ resolve, reject })
      })
    })
    const updateAgentV2 = vi.fn(async () => ({ ok: true }))
    const createAgentV2 = vi.fn(async () => ({ ok: true }))
    const onLoadError = vi.fn()

    function HarnessWithError() {
      const session = useAgentV2EditSession({
        getAgentV2,
        updateAgentV2,
        createAgentV2,
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
          <AgentEditorV2
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
      pending[1]!.resolve({ agent: specB })
    })
    await waitFor(() => {
      expect(screen.getByTestId('edit-session-loading')).toHaveTextContent('0')
      expect(screen.getByTestId('agent-editor-v2-save')).not.toBeDisabled()
    })
    expect(dialog()).not.toHaveClass('hidden')
  })

  it('same-id reopen: stale GET₁ success must not overwrite GET₂ draft', async () => {
    const stale = makeSpec('B', 'agent-b-stale')
    const fresh = makeSpec('B', 'agent-b-fresh')
    fresh.config_version = 2
    fresh.user_prompt = 'fresh prompt from GET₂'
    const pending: Array<{
      resolve: (v: { agent: AgentSpec }) => void
      reject: (e: unknown) => void
    }> = []
    const getAgentV2 = vi.fn(async (id: string) => {
      if (id !== 'B') throw new Error(`unexpected id ${id}`)
      return new Promise<{ agent: AgentSpec }>((resolve, reject) => {
        pending.push({ resolve, reject })
      })
    })

    render(
      <Harness
        getAgentV2={getAgentV2}
        updateAgentV2={vi.fn(async () => ({ ok: true }))}
        createAgentV2={vi.fn(async () => ({ ok: true }))}
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
      pending[0]!.resolve({ agent: stale })
    })
    expect(screen.getByTestId('edit-session-agent-name')).toHaveTextContent('')
    expect(screen.getByTestId('edit-session-loading')).toHaveTextContent('1')
    expect(dialog()).not.toHaveClass('hidden')

    // GET₂ seeds the fresh draft only.
    await act(async () => {
      pending[1]!.resolve({ agent: fresh })
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
