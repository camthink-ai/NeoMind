/**
 * Frontend Component Slice
 *
 * Zustand slice for managing community marketplace dashboard components.
 * Handles fetching, installing, and uninstalling frontend components.
 *
 * Matches backend API: crates/neomind-api/src/handlers/frontend_components.rs
 */

import type { StateCreator } from 'zustand'
import type {
  FrontendComponentMeta,
  MarketComponentEntry,
} from '@/types/frontend-component'
import { api } from '@/lib/api'
import { communityRegistry } from '@/components/dashboard/registry/CommunityRegistry'
import { logError } from '@/lib/errors'

// ============================================================================
// State
// ============================================================================

export interface FrontendComponentState {
  installed: FrontendComponentMeta[]
  marketComponents: MarketComponentEntry[]
  marketLoading: boolean
  loading: boolean
  error: string | null
  fetchCache: Record<string, { timestamp: number; fetching?: boolean }>
  /** Map of component id → { current, latest } when a newer version exists on the marketplace. */
  updatesAvailable: Record<string, { current: string; latest: string }>
}

// ============================================================================
// Slice Interface
// ============================================================================

export interface FrontendComponentSlice extends FrontendComponentState {
  fetchInstalled: () => Promise<void>
  fetchMarket: () => Promise<void>
  installFromMarket: (componentId: string) => Promise<void>
  installManualZip: (zipFile: File) => Promise<FrontendComponentMeta>
  /** Install from a .zip that already sits on the server, inside its data directory. */
  installFromPath: (filePath: string) => Promise<FrontendComponentMeta>
  uninstall: (id: string) => Promise<void>
  refreshComponent: (id: string) => Promise<void>
  checkUpdates: () => Promise<void>
}

// ============================================================================
// Cache Helper
// ============================================================================

const CACHE_TTL = 10_000 // 10 seconds

function shouldFetch(cache: Record<string, { timestamp: number; fetching?: boolean }>, key: string): boolean {
  const entry = cache[key]
  if (!entry) return true
  if (entry.fetching) return false
  return Date.now() - entry.timestamp > CACHE_TTL
}

// ============================================================================
// Slice Creator
// ============================================================================

export const createFrontendComponentSlice: StateCreator<
  FrontendComponentSlice,
  [],
  [],
  FrontendComponentSlice
> = (set, get) => ({
  // Initial state
  installed: [],
  marketComponents: [],
  marketLoading: false,
  loading: false,
  error: null,
  fetchCache: {},
  updatesAvailable: {},

  // ========== Actions ==========

  /**
   * Fetch all installed frontend components
   * Backend: GET /api/frontend-components -> { components: FrontendComponentMeta[] }
   */
  fetchInstalled: async () => {
    const cache = get().fetchCache
    if (!shouldFetch(cache, 'installed')) return

    set({ loading: true, error: null, fetchCache: { ...cache, installed: { timestamp: cache.installed?.timestamp ?? 0, fetching: true } } })
    try {
      const res = await api.get<{ components: FrontendComponentMeta[] }>('/frontend-components')
      const components = res.components || []

      // Sync with community registry
      communityRegistry.syncFromApi(components)

      set({
        installed: components,
        fetchCache: { ...cache, installed: { timestamp: Date.now() } },
        error: null,
      })
    } catch (error) {
      logError(error, { operation: 'Fetch installed components' })
      set({
        installed: [],
        error: error instanceof Error ? error.message : 'Failed to fetch installed components',
        fetchCache: { ...cache, installed: { timestamp: Date.now() } },
      })
    } finally {
      set({ loading: false })
    }
  },

  /**
   * Fetch all available components from the marketplace
   * Backend: GET /api/frontend-components/market/list -> { components: MarketComponentEntry[] }
   */
  fetchMarket: async () => {
    set({ marketLoading: true, error: null })
    try {
      const res = await api.get<{ components: MarketComponentEntry[] }>('/frontend-components/market/list')
      const components = res.components || []

      set({
        marketComponents: components,
        error: null,
      })
    } catch (error) {
      logError(error, { operation: 'Fetch market components' })
      set({
        marketComponents: [],
        error: error instanceof Error ? error.message : 'Failed to fetch marketplace',
      })
    } finally {
      set({ marketLoading: false })
    }
  },

  /**
   * Install a component from the marketplace
   * Backend: POST /frontend-components/market/install -> { component } or { success: false, error }
   */
  installFromMarket: async (componentId) => {
    set({ loading: true, error: null })
    try {
      const res = await api.post<{ component?: FrontendComponentMeta; success?: boolean; error?: string }>(
        '/frontend-components/market/install',
        { component_id: componentId }
      )

      // Handle graceful error from backend (network issues etc.)
      if (res.success === false || !res.component) {
        const errMsg = res.error || 'Failed to install component'
        throw new Error(errMsg)
      }

      const component = res.component

      // Add to installed list
      set((state) => ({
        installed: [...state.installed, component],
      }))

      // Sync with community registry
      communityRegistry.syncFromApi([...get().installed, component])

      // Clear cache to force refresh
      set((state) => ({
        fetchCache: Object.fromEntries(
          Object.entries(state.fetchCache).filter(([key]) => key !== 'installed')
        ),
      }))

      // Refresh installed list
      await get().fetchInstalled()
    } catch (error) {
      logError(error, { operation: 'Install from marketplace', context: { componentId } })
      set({
        error: error instanceof Error ? error.message : 'Failed to install component',
      })
      throw error
    } finally {
      set({ loading: false })
    }
  },

  /**
   * Install a component from a ZIP package
   * Backend: POST /frontend-components (multipart, field: package)
   */
  installManualZip: async (zipFile) => {
    set({ loading: true, error: null })
    try {
      const formData = new FormData()
      formData.append('package', zipFile)

      const res = await api.post<{ component: FrontendComponentMeta }>(
        '/frontend-components',
        formData,
        {
          headers: {
            'Content-Type': 'multipart/form-data',
          },
        }
      )
      const component = res.component

      // Add to installed list
      set((state) => ({
        installed: [...state.installed, component],
      }))

      // Sync with community registry
      communityRegistry.syncFromApi([...get().installed, component])

      // Clear cache to force refresh
      set((state) => ({
        fetchCache: Object.fromEntries(
          Object.entries(state.fetchCache).filter(([key]) => key !== 'installed')
        ),
      }))

      return component
    } catch (error) {
      logError(error, { operation: 'Install ZIP component' })
      set({
        error: error instanceof Error ? error.message : 'Failed to install component',
      })
      throw error
    } finally {
      set({ loading: false })
    }
  },

  /**
   * Install a component from a ZIP package that already sits on the
   * server (inside its data directory).
   * Backend: POST /frontend-components/from-path
   */
  installFromPath: async (filePath) => {
    set({ loading: true, error: null })
    try {
      const res = await api.post<{ component: FrontendComponentMeta }>(
        '/frontend-components/from-path',
        { file_path: filePath },
      )
      const component = res.component

      // Add to installed list
      set((state) => ({
        installed: [...state.installed, component],
      }))

      // Sync with community registry
      communityRegistry.syncFromApi([...get().installed, component])

      // Clear cache to force refresh
      set((state) => ({
        fetchCache: Object.fromEntries(
          Object.entries(state.fetchCache).filter(([key]) => key !== 'installed')
        ),
      }))

      return component
    } catch (error) {
      logError(error, { operation: 'Install component from server path' })
      set({
        error: error instanceof Error ? error.message : 'Failed to install component',
      })
      throw error
    } finally {
      set({ loading: false })
    }
  },

  /**
   * Uninstall a component
   * Backend: DELETE /api/frontend-components/:id
   */
  uninstall: async (id) => {
    set({ loading: true, error: null })
    try {
      await api.delete(`/frontend-components/${id}`)

      // Remove from installed list
      set((state) => ({
        installed: state.installed.filter((c) => c.id !== id),
      }))

      // Unregister from community registry
      communityRegistry.unregister(id)

      // Clear cache to force refresh
      set((state) => ({
        fetchCache: Object.fromEntries(
          Object.entries(state.fetchCache).filter(([key]) => key !== 'installed')
        ),
      }))
    } catch (error) {
      logError(error, { operation: 'Uninstall component', context: { id } })
      set({
        error: error instanceof Error ? error.message : 'Failed to uninstall component',
      })
      throw error
    } finally {
      set({ loading: false })
    }
  },

  /**
   * Refresh a component so the latest bundle is loaded.
   *
   * - Always clears the in-browser community registry cache (window[globalName],
   *   injected <script> tags, loadedModules) so the IIFE re-executes on next mount.
   * - For marketplace components, additionally POSTs to the install endpoint so the
   *   backend re-downloads the latest manifest + bundle from GitHub, overwriting the
   *   frozen local copy. (The install endpoint is idempotent via atomic_write.)
   * - For locally-installed components (manual ZIP / AI regeneration), the bundle on
   *   disk is already current, so only the registry-cache reset is needed.
   *
   * On success the authoritative installed list is re-fetched and the id is cleared
   * from `updatesAvailable`. On failure the error is re-thrown so the UI can toast.
   */
  refreshComponent: async (id) => {
    // 1. Clear community registry caches for this component
    communityRegistry.refreshComponent(id)

    // 2. For marketplace components, trigger backend re-download from GitHub
    const comp = get().installed.find((c) => c.id === id)
    const isMarketplace = comp?.source === 'marketplace'
    if (isMarketplace) {
      const res = await api.post<{ success?: boolean; error?: string }>(
        '/frontend-components/market/install',
        { component_id: id }
      )
      if (res.success === false) {
        throw new Error(res.error || 'Failed to reinstall component')
      }
    }

    // 3. Force re-fetch by clearing cache, then pull authoritative list
    set((state) => ({
      fetchCache: Object.fromEntries(
        Object.entries(state.fetchCache).filter(([key]) => key !== 'installed')
      ),
    }))
    await get().fetchInstalled()

    // 4. Clear update badge for this component
    set((state) => {
      if (!state.updatesAvailable[id]) return state
      const next = { ...state.updatesAvailable }
      delete next[id]
      return { updatesAvailable: next }
    })
  },

  /**
   * Check the marketplace for newer versions of installed community components.
   * Backend: GET /api/frontend-components/updates -> { updates: [{ id, current_version, latest_version }] }
   *
   * Network-tolerant: failures are swallowed (no error toast) — update badges
   * are best-effort and must never block the UI.
   */
  checkUpdates: async () => {
    try {
      const res = await api.get<{
        updates?: Array<{ id: string; current_version: string; latest_version: string }>
        error?: string
      }>('/frontend-components/updates')

      // Backend returns 200 with { error: "network_error" } on failure.
      if (res.error) return

      const map: Record<string, { current: string; latest: string }> = {}
      for (const u of res.updates || []) {
        map[u.id] = { current: u.current_version, latest: u.latest_version }
      }
      set({ updatesAvailable: map })
    } catch (error) {
      // Silent — update detection is best-effort.
      logError(error, { operation: 'Check component updates' })
    }
  },
})
