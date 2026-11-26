/**
 * Race-safe AgentSpec v2 edit session for AgentsPage.
 *
 * Open edit clears any stale editingAgent, tracks the selected id, and only
 * applies GET /agents/v2/:id when it still matches. Cancel / reselect drops
 * late responses. Save is refused while loading or when the target id drifted.
 */
import { useCallback, useRef, useState } from 'react'
import type { AgentSpec, AgentSpecDraft, AgentSpecPatch } from '@/types'
import { draftToUpdatePatch, specToDraft } from '@/components/agents/editor-v2/defaults'

export interface AgentV2EditSessionApis {
  getAgentV2: (id: string) => Promise<{ agent: AgentSpec }>
  updateAgentV2: (id: string, patch: AgentSpecPatch) => Promise<unknown>
  createAgentV2: (draft: AgentSpecDraft) => Promise<unknown>
}

export interface UseAgentV2EditSessionOptions extends AgentV2EditSessionApis {
  onSaved?: () => void | Promise<void>
  onLoadError?: (error: unknown) => void
}

export interface AgentV2EditSession {
  open: boolean
  loading: boolean
  editingAgent: AgentSpec | undefined
  openCreate: () => void
  openEdit: (id: string) => Promise<void>
  close: () => void
  onOpenChange: (open: boolean) => void
  save: (draft: AgentSpecDraft) => Promise<void>
}

export function useAgentV2EditSession(
  options: UseAgentV2EditSessionOptions,
): AgentV2EditSession {
  const optionsRef = useRef(options)
  optionsRef.current = options

  const [open, setOpen] = useState(false)
  const [loading, setLoading] = useState(false)
  const [editingAgent, setEditingAgent] = useState<AgentSpec | undefined>(undefined)
  /** Currently selected edit target; null = create or closed. */
  const pendingIdRef = useRef<string | null>(null)

  const close = useCallback(() => {
    pendingIdRef.current = null
    setLoading(false)
    setEditingAgent(undefined)
    setOpen(false)
  }, [])

  const openCreate = useCallback(() => {
    pendingIdRef.current = null
    setLoading(false)
    setEditingAgent(undefined)
    setOpen(true)
  }, [])

  const openEdit = useCallback(async (id: string) => {
    pendingIdRef.current = id
    // Drop stale A immediately so save cannot PUT the previous id.
    setEditingAgent(undefined)
    setLoading(true)
    setOpen(true)
    try {
      const { agent } = await optionsRef.current.getAgentV2(id)
      if (pendingIdRef.current !== id) return
      setEditingAgent(agent)
    } catch (error) {
      if (pendingIdRef.current !== id) return
      pendingIdRef.current = null
      setEditingAgent(undefined)
      setOpen(false)
      optionsRef.current.onLoadError?.(error)
    } finally {
      if (pendingIdRef.current === id) {
        setLoading(false)
      }
    }
  }, [])

  const onOpenChange = useCallback(
    (next: boolean) => {
      if (!next) close()
      else setOpen(true)
    },
    [close],
  )

  const save = useCallback(async (draft: AgentSpecDraft) => {
    if (loading) {
      throw new Error('Agent edit still loading')
    }
    const { updateAgentV2, createAgentV2, onSaved } = optionsRef.current
    if (editingAgent) {
      if (pendingIdRef.current !== editingAgent.id) {
        throw new Error('Stale agent edit target')
      }
      const previous = specToDraft(editingAgent)
      await updateAgentV2(
        editingAgent.id,
        draftToUpdatePatch(draft, previous),
      )
    } else {
      if (pendingIdRef.current) {
        throw new Error('Agent edit still pending')
      }
      await createAgentV2(draft)
    }
    pendingIdRef.current = null
    setEditingAgent(undefined)
    setOpen(false)
    setLoading(false)
    await onSaved?.()
  }, [editingAgent, loading])

  return {
    open,
    loading,
    editingAgent,
    openCreate,
    openEdit,
    close,
    onOpenChange,
    save,
  }
}
