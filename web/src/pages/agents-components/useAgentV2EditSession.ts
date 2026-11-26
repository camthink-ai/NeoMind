/**
 * Race-safe AgentSpec v2 edit session for AgentsPage.
 *
 * Open edit clears any stale editingAgent and bumps a per-open sequence so
 * only the current open's GET may seed / close / clear loading. Same-id
 * cancel→reopen must not let GET₁ failure close GET₂, or GET₁ success
 * overwrite GET₂'s draft. Save is refused while loading or when the target
 * id drifted.
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
  /** Monotonic per-open token; cancel / create / new openEdit invalidate in-flight GETs. */
  const loadSeqRef = useRef(0)

  const invalidateInFlight = useCallback(() => {
    loadSeqRef.current += 1
  }, [])

  const close = useCallback(() => {
    invalidateInFlight()
    pendingIdRef.current = null
    setLoading(false)
    setEditingAgent(undefined)
    setOpen(false)
  }, [invalidateInFlight])

  const openCreate = useCallback(() => {
    invalidateInFlight()
    pendingIdRef.current = null
    setLoading(false)
    setEditingAgent(undefined)
    setOpen(true)
  }, [invalidateInFlight])

  const openEdit = useCallback(async (id: string) => {
    const seq = ++loadSeqRef.current
    pendingIdRef.current = id
    // Drop stale target immediately so save cannot PUT a previous id.
    setEditingAgent(undefined)
    setLoading(true)
    setOpen(true)
    try {
      const { agent } = await optionsRef.current.getAgentV2(id)
      if (loadSeqRef.current !== seq) return
      setEditingAgent(agent)
    } catch (error) {
      if (loadSeqRef.current !== seq) return
      pendingIdRef.current = null
      setEditingAgent(undefined)
      setOpen(false)
      optionsRef.current.onLoadError?.(error)
    } finally {
      if (loadSeqRef.current === seq) {
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
    invalidateInFlight()
    pendingIdRef.current = null
    setEditingAgent(undefined)
    setOpen(false)
    setLoading(false)
    await onSaved?.()
  }, [editingAgent, loading, invalidateInFlight])

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
