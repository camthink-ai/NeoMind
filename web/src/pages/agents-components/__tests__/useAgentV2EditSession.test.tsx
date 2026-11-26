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
})
