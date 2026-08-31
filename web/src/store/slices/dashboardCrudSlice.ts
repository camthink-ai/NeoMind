/**
 * Dashboard CRUD Slice
 *
 * Dashboard lifecycle: fetch, create, update, delete.
 * Owns persistence (storage) and debounced sync.
 * scheduleSync/flushSync are exposed as slice methods for other slices.
 */

import type { StateCreator } from 'zustand'
import type {
  Dashboard,
  DashboardComponent,
  DashboardTemplate,
  DashboardLayout,
} from '@/types/dashboard'
import { createDashboardStorage, fromDashboardDTO, type DashboardStorage } from '../persistence'
import { hasRecentServerSyncFailure } from '../persistence/implementations'
import { logError } from '@/lib/errors'
import {
  generateId,
  cleanupAgentForComponent,
  updateDashboardInState,
} from './dashboardHelpers'
import type { DashboardStore } from './dashboardHelpers'

// ============================================================================
// Self-sync echo suppression
// ============================================================================
// When the frontend triggers a dashboard save (drag, config edit, etc.), the
// backend emits a DashboardUpdated SSE event back to us.  Without suppression
// this echo causes a full fetchDashboards() that overwrites in-progress edits.

const SELF_SYNC_ECHO_MS = 5000 // ignore echo for 5s after our own sync

const recentSelfSyncs: string[] = [] // dashboard IDs we recently synced
const recentSelfSyncTimestamps: number[] = [] // matching timestamps

/** Record that we are about to sync a dashboard. */
function recordSelfSync(dashboardId: string): void {
  const now = Date.now()
  recentSelfSyncs.push(dashboardId)
  recentSelfSyncTimestamps.push(now)
  // Prune stale entries
  const cutoff = now - SELF_SYNC_ECHO_MS
  while (recentSelfSyncTimestamps.length > 0 && recentSelfSyncTimestamps[0] < cutoff) {
    recentSelfSyncs.shift()
    recentSelfSyncTimestamps.shift()
  }
}

/** Any dashboard synced within the echo window? Used by the DataChanged
 *  listener to skip refetches that would overwrite in-progress edits. */
export function hasAnyRecentSelfSync(): boolean {
  const now = Date.now()
  return recentSelfSyncTimestamps.some((ts) => now - ts <= SELF_SYNC_ECHO_MS)
}

/** Should we ignore this DashboardUpdated SSE event (echo of our own sync)? */
export function isSelfSyncEcho(eventDashboardId: string): boolean {
  const now = Date.now()
  for (let i = recentSelfSyncs.length - 1; i >= 0; i--) {
    if (now - recentSelfSyncTimestamps[i] > SELF_SYNC_ECHO_MS) break
    if (recentSelfSyncs[i] === eventDashboardId) return true
  }
  return false
}

// ============================================================================
// Defaults
// ============================================================================

export const DEFAULT_LAYOUT: DashboardLayout = {
  columns: 12,
  rows: 'auto',
  breakpoints: { lg: 1200, md: 996, sm: 768, xs: 480 },
}

export const DEFAULT_TEMPLATES: DashboardTemplate[] = [
  {
    id: 'overview', name: 'Overview',
    description: 'System overview with devices, agents, and events',
    category: 'overview', icon: 'LayoutDashboard',
    layout: DEFAULT_LAYOUT, components: [],
    requiredResources: { devices: 1 },
  },
  {
    id: 'blank', name: 'Blank Canvas',
    description: 'Start from scratch with an empty dashboard',
    category: 'custom', icon: 'Square',
    layout: DEFAULT_LAYOUT, components: [],
  },
]

// ============================================================================
// Slice type
// ============================================================================

export interface DashboardCrudSlice {
  dashboards: Dashboard[]
  currentDashboard: Dashboard | null
  currentDashboardId: string | null
  templates: DashboardTemplate[]
  _fetchId: number | null
  dashboardsLoading: boolean

  fetchDashboards: () => Promise<void>
  createDashboard: (dashboard: Omit<Dashboard, 'id' | 'createdAt' | 'updatedAt'>) => Promise<string>
  updateDashboard: (id: string, updates: Partial<Dashboard>) => Promise<void>
  deleteDashboard: (id: string) => Promise<void>
  duplicateDashboard: (id: string) => Promise<string | null>
  setCurrentDashboard: (id: string | null) => void
  setDefaultDashboard: (id: string) => Promise<void>
  /** Persist a new manual order (index 0 = top). Rolls back on failure. */
  reorderDashboards: (newOrder: string[]) => Promise<void>
  clearDashboards: () => void
  persistDashboard: (id?: string) => Promise<void>
  fetchTemplates: () => Promise<void>

  // Sync methods used by other slices
  scheduleSync: (dashboard: Dashboard) => void
  flushSync: () => Promise<void>
}

// ============================================================================
// Slice factory
// ============================================================================

export const createDashboardCrudSlice: StateCreator<
  DashboardStore, [], [], DashboardCrudSlice
> = (set, get) => {
  const storage: DashboardStorage = createDashboardStorage({ type: 'hybrid', cacheEnabled: true })

  // Debounced sync — captured in closure
  let syncDebounceTimer: ReturnType<typeof setTimeout> | null = null
  // Bumped by clearDashboards (logout): in-flight syncs/flushes compare it
  // after their awaits and skip handleIdChange — a late set() would
  // resurrect the previous account's dashboard into the cleared store.
  let dashboardEpoch = 0
  function handleIdChange(dash: Dashboard, result: { data: Dashboard | null }): void {
    if (result.data && result.data.id !== dash.id) {
      // Also record the server-assigned ID so the SSE echo is suppressed
      recordSelfSync(result.data.id)

      const { dashboards: currentDashboards } = get()
      const activeDashboardId = get().currentDashboardId
      const newDashboards = currentDashboards.map((d: Dashboard) =>
        d.id === dash.id ? result.data! : d,
      )
      set({
        dashboards: newDashboards,
        ...(activeDashboardId === dash.id
          ? { currentDashboard: result.data, currentDashboardId: result.data.id }
          : {}),
      })
    }
  }

  let syncVersion = 0
  let pendingSyncDashboard: Dashboard | null = null
  function scheduleSync(dashboard: Dashboard): void {
    const version = ++syncVersion
    pendingSyncDashboard = dashboard
    if (syncDebounceTimer) clearTimeout(syncDebounceTimer)
    syncDebounceTimer = setTimeout(async () => {
      syncDebounceTimer = null
      // Only sync if no newer schedule call has been made
      if (version !== syncVersion) return
      recordSelfSync(dashboard.id)
      const entryEpoch = dashboardEpoch
      try {
        const result = await storage.sync(dashboard)
        if (entryEpoch !== dashboardEpoch) return // logged out mid-sync
        // Clear AFTER the sync resolves: while the request is in flight
        // hasUnflushedLocalEdits() must stay true, or a server refresh
        // during a slow sync could flash the dashboard back to the
        // pre-sync state (the old code cleared before sending — the exact
        // window the guard existed to close).
        if (version === syncVersion) {
          pendingSyncDashboard = null
        }
        handleIdChange(dashboard, result)
      } catch (err) {
        // Keep pending set so the guard still protects the unsynced edits,
        // but BOUNDED: if even the next schedule/flush can't clear it (e.g.
        // localStorage quota full — common on embedded devices), release
        // the guard after 30s so server refreshes aren't blocked forever
        // (the old failure-window semantics in the persistence layer).
        console.warn('[DashboardCrudSlice] sync failed:', err)
        const failVersion = version
        setTimeout(() => {
          if (pendingSyncDashboard?.id === dashboard.id && failVersion === syncVersion - 0) {
            // Only release if nothing newer was scheduled meanwhile.
            if (!syncDebounceTimer) pendingSyncDashboard = null
          }
        }, 30_000)
      }
    }, 500)
  }

  async function flushSync(): Promise<void> {
    if (syncDebounceTimer) {
      clearTimeout(syncDebounceTimer)
      syncDebounceTimer = null
      // Execute the pending sync immediately instead of discarding it.
      // pending stays set until the flush resolves — clearing before the
      // await leaves the same unguarded in-flight window the debounce path
      // used to have.
      const dashboard = pendingSyncDashboard
      if (dashboard) {
        const flushVersion = ++syncVersion
        const entryEpoch = dashboardEpoch
        recordSelfSync(dashboard.id)
        try {
          const result = await storage.sync(dashboard)
          if (entryEpoch !== dashboardEpoch) return // logged out mid-flush
          if (flushVersion === syncVersion) {
            pendingSyncDashboard = null
          }
          handleIdChange(dashboard, result)
        } catch (err) {
          console.warn('[DashboardCrudSlice] flush sync failed:', err)
        }
      }
    }
  }

  // Flush a pending debounced sync when the page is hidden or being unloaded.
  // The 500ms debounce otherwise silently drops the last edit on close.
  // visibilitychange(hidden) also covers mobile backgrounding, where pagehide
  // is unreliable.
  if (typeof window !== 'undefined') {
    const flushOnLeave = () => { void flushSync() }
    window.addEventListener('pagehide', flushOnLeave)
    document.addEventListener('visibilitychange', () => {
      if (document.visibilityState === 'hidden') flushOnLeave()
    })
  }

  /**
   * True while a local edit is still in flight: the debounce timer hasn't
   * fired yet, or a previous sync attempt recently failed against the server
   * (its retries are still running). Applying a server snapshot in either
   * state would overwrite newer local edits.
   */
  function hasUnflushedLocalEdits(): boolean {
    return syncDebounceTimer !== null ||
      pendingSyncDashboard !== null ||
      hasRecentServerSyncFailure()
  }

  return {
    // --- Initial state ---
    dashboards: [],
    currentDashboard: null,
    currentDashboardId: null,
    templates: DEFAULT_TEMPLATES,
    _fetchId: null,
    dashboardsLoading: false,

    // --- Sync methods (used by layout/config slices) ---
    scheduleSync,
    flushSync,

    // --- CRUD ---

    fetchDashboards: async () => {
      const fetchId = Date.now()
      set({ _fetchId: fetchId, dashboardsLoading: true })
      try {
        const result = await storage.load()
        if (get()._fetchId !== fetchId) return

        if (result.error) {
          set({ dashboards: [], dashboardsLoading: false })
        } else if (result.data) {
          // Migration: dataSource from config → component level
          const migrated = result.data.map((d: Dashboard) => ({
            ...d,
            components: d.components.map((comp) => {
              const rawConfig = comp.config
              const config = rawConfig && typeof rawConfig === 'object' && !Array.isArray(rawConfig) ? rawConfig as Record<string, unknown> : {}
              if ('dataSource' in config && config.dataSource) {
                const { dataSource, ...rest } = config
                return { ...comp, config: rest, dataSource } as DashboardComponent
              }
              return comp
            }),
          })) as Dashboard[]

          // Note: We intentionally do NOT filter out components with invalid data sources
          // on load. Previous behavior silently deleted components whose device/extension
          // was temporarily unavailable — this caused permanent data loss.
          // Instead, invalid data sources are handled at the UI layer (component shows
          // a "device unavailable" state) so user config is preserved.

          // A pending (or recently failed) local sync means the in-memory state
          // is newer than the snapshot the server just returned. Skip applying
          // it — otherwise a DataChanged/SSE-triggered refresh would silently
          // discard the user's edits ("change flashed back to old value").
          if (hasUnflushedLocalEdits()) {
            console.warn('[DashboardCrudSlice] Skipping server snapshot — local edits not yet synced')
            if (get()._fetchId !== fetchId) return
            set({ dashboardsLoading: false })
            return
          }

          if (get()._fetchId !== fetchId) return
          set({ dashboards: migrated })

          const savedId = (storage as any).getCurrentDashboardId?.()
          const currentState = get()
          let target: Dashboard | null = null
          if (savedId && migrated.length > 0) {
            target = migrated.find((d: Dashboard) => d.id === savedId) || null
          }
          if (!target && !currentState.currentDashboardId && migrated.length > 0) {
            target = migrated.find((d: Dashboard) => d.isDefault) || migrated[0]
          }
          if (target) {
            // Always update currentDashboard so live edits (AI, other clients) are reflected
            set({
              currentDashboardId: target.id,
              currentDashboard: target,
            })
          } else if (currentState.currentDashboardId) {
            // Current dashboard was deleted — switch to first available
            const fallback = migrated.find((d: Dashboard) => d.isDefault) || migrated[0]
            if (fallback) {
              set({ currentDashboardId: fallback.id, currentDashboard: fallback })
            } else {
              set({ currentDashboardId: null, currentDashboard: null })
            }
          }
        } else {
          set({ dashboards: [] })
        }
      } catch (err) {
        logError(err, { operation: 'Load dashboards' })
        if (get()._fetchId === fetchId) set({ dashboards: [] })
      } finally {
        if (get()._fetchId === fetchId) set({ dashboardsLoading: false })
      }
    },

    createDashboard: async (dashboard) => {
      const local: Dashboard = { ...dashboard, id: generateId(), createdAt: Date.now(), updatedAt: Date.now() }
      set((s) => ({
        dashboards: [...s.dashboards, local],
        currentDashboardId: local.id,
        currentDashboard: local,
      }))
      scheduleSync(local)
      return local.id
    },

    updateDashboard: async (id, updates) => {
      const { dashboards, currentDashboardId } = get()
      const updated = { ...updates, updatedAt: Date.now() } as Partial<Dashboard>
      const { dashboards: newDashboards, currentDashboard } = updateDashboardInState(dashboards, id, updated, currentDashboardId)
      set({ dashboards: newDashboards, currentDashboard: currentDashboardId === id ? currentDashboard : get().currentDashboard })
      const target = newDashboards.find((d: Dashboard) => d.id === id)
      if (target) scheduleSync(target)
    },

    deleteDashboard: async (id) => {
      const { dashboards, currentDashboardId } = get()
      const dashboard = dashboards.find((d: Dashboard) => d.id === id)
      if (dashboard) (dashboard.components as DashboardComponent[]).forEach(cleanupAgentForComponent)
      const updated = dashboards.filter((d: Dashboard) => d.id !== id)
      const newCurrentId = currentDashboardId === id ? (updated[0]?.id || null) : currentDashboardId
      set({
        dashboards: updated,
        currentDashboardId: newCurrentId,
        currentDashboard: updated.find((d: Dashboard) => d.id === newCurrentId) || null,
        ...(currentDashboardId === id ? {
          editMode: false,
          selectedComponent: null,
          configComponentId: null,
          configPanelOpen: false,
        } : {}),
      })
      await storage.delete(id)
    },

    duplicateDashboard: async (id) => {
      try {
        const { api } = await import('@/lib/api')
        const dto = await api.duplicateDashboard(id)
        const newDash = fromDashboardDTO(dto)
        set((s) => ({
          dashboards: [...s.dashboards, newDash],
        }))
        recordSelfSync(newDash.id)
        return newDash.id
      } catch (err) {
        logError(err, { operation: 'Duplicate dashboard', context: { dashboardId: id } })
        return null
      }
    },

    setCurrentDashboard: async (id) => {
      await flushSync()
      const { dashboards } = get()
      const dashboard = id ? dashboards.find((d: Dashboard) => d.id === id) || null : dashboards[0] || null
      set({ currentDashboardId: id, currentDashboard: dashboard, editMode: false, selectedComponent: null })
      ;(storage as any).setCurrentDashboardId?.(id)
    },

    setDefaultDashboard: async (id) => {
      const { dashboards, currentDashboardId } = get()
      const updated = dashboards.map((d: Dashboard) => ({ ...d, isDefault: d.id === id }))
      const updatedCurrent = currentDashboardId ? updated.find((d: Dashboard) => d.id === currentDashboardId) || null : null
      set({ dashboards: updated, ...(updatedCurrent ? { currentDashboard: updatedCurrent } : {}) })
      // Record self-sync for all dashboards since save() bulk-syncs to API
      for (const d of updated) recordSelfSync(d.id)
      await storage.save(updated)
    },

    reorderDashboards: async (newOrder) => {
      const { dashboards } = get()
      const map = new Map(dashboards.map((d: Dashboard) => [d.id, d]))
      // Build the reordered + reindexed list. Dashboards not in newOrder
      // (defensive) keep their relative position appended at the end.
      const inOrder: Dashboard[] = []
      newOrder.forEach((id, i) => {
        const d = map.get(id)
        if (d) inOrder.push({ ...d, sortOrder: i })
      })
      const inOrderIds = new Set(newOrder)
      const leftovers: Dashboard[] = dashboards
        .filter((d: Dashboard) => !inOrderIds.has(d.id))
        .map((d: Dashboard, i: number) => ({ ...d, sortOrder: inOrder.length + i }))
      const reordered: Dashboard[] = [...inOrder, ...leftovers]

      // Optimistic update
      set({ dashboards: reordered })

      // Suppress SSE echo for every affected dashboard before the API call
      reordered.forEach((d: Dashboard) => recordSelfSync(d.id))

      try {
        const result = await storage.reorder?.(newOrder)
        if (result?.error) throw result.error
      } catch (err) {
        // Roll back to the pre-reorder state and surface the error
        set({ dashboards })
        logError(err, { operation: 'Reorder dashboards' })
      }
    },

    clearDashboards: () => {
      const { dashboards } = get()
      dashboards.forEach((d: Dashboard) => (d.components as DashboardComponent[]).forEach(cleanupAgentForComponent))
      // Cancel any pending debounced sync: firing ~500ms after logout it
      // would re-persist the previous account's dashboard into localStorage,
      // and the next login's local-only merge would adopt it as its own.
      if (syncDebounceTimer !== null) {
        clearTimeout(syncDebounceTimer)
        syncDebounceTimer = null
      }
      pendingSyncDashboard = null
      dashboardEpoch++
      storage.clear()
      set({
        dashboards: [],
        currentDashboard: null,
        currentDashboardId: null,
        editMode: false,
        selectedComponent: null,
        configComponentId: null,
        configPanelOpen: false,
        componentLibraryOpen: false,
        templateDialogOpen: false,
      })
    },

    persistDashboard: async (id) => {
      const { currentDashboard, dashboards } = get()
      const target = id ? dashboards.find((d: Dashboard) => d.id === id) : currentDashboard
      if (!target) { console.warn('[persistDashboard] no dashboard'); return }
      recordSelfSync(target.id)
      await storage.sync(target)
    },

    fetchTemplates: async () => {
      try {
        const { api } = await import('@/lib/api')
        const data = await api.getDashboardTemplates()
        const valid = (data || []).filter((t: { category: string }) =>
          ['overview', 'monitoring', 'automation', 'agents', 'custom'].includes(t.category),
        ) as DashboardTemplate[]
        set({ templates: [...DEFAULT_TEMPLATES, ...valid] })
      } catch {
        set({ templates: DEFAULT_TEMPLATES })
      }
    },
  }
}
