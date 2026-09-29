/**
 * LLM Backend Slice
 *
 * Manages multiple LLM backend instances with CRUD operations,
 * activation switching, and connection testing.
 */

import type { StateCreator } from 'zustand'
import type {
  LlmBackendInstance,
  BackendTypeDefinition,
  CreateLlmBackendRequest,
  UpdateLlmBackendRequest,
  BackendTestResult,
} from '@/types'
import { api } from '@/lib/api'
import { logError } from '@/lib/errors'
import { fetchCache } from '@/lib/utils/async'

export interface LlmBackendState {
  // State
  llmBackends: LlmBackendInstance[]
  activeBackendId: string | null
  backendTypes: BackendTypeDefinition[]
  llmBackendLoading: boolean
  error: string | null

  // Test results cache (backend_id -> result)
  testResults: Record<string, BackendTestResult>
}

export interface LlmBackendSlice extends LlmBackendState {
  // CRUD operations
  loadBackends: () => Promise<void>
  loadBackendTypes: () => Promise<void>
  createBackend: (backend: CreateLlmBackendRequest) => Promise<string>
  updateBackend: (id: string, updates: UpdateLlmBackendRequest) => Promise<boolean>
  deleteBackend: (id: string) => Promise<boolean>

  // Activation
  activateBackend: (id: string) => Promise<boolean>
  getActiveBackend: () => LlmBackendInstance | null

  // Testing
  testBackend: (id: string) => Promise<BackendTestResult>
  clearTestResult: (id: string) => void

  // UI state
  setError: (error: string | null) => void
  clearError: () => void
}

export const createLlmBackendSlice: StateCreator<
  LlmBackendSlice,
  [],
  [],
  LlmBackendSlice
> = (set, get) => ({
  // Initial state
  llmBackends: [],
  activeBackendId: null,
  backendTypes: [],
  llmBackendLoading: false,
  error: null,
  testResults: {},

  // Load all backends
  loadBackends: async () => {
    if (!fetchCache.shouldFetch('llmBackends')) return
    fetchCache.markFetching('llmBackends')
    set({ llmBackendLoading: true, error: null })
    try {
      const data = await api.listLlmBackends()
      const backends = data.backends || []
      // Persist whether this install has backends at all. Chat's setup-guide
      // gate reads the marker synchronously on first paint — without it, a
      // configured install flashes the setup guide until this fetch lands.
      if (backends.length > 0) localStorage.setItem('neomind_has_llm_backends', '1')
      else localStorage.removeItem('neomind_has_llm_backends')
      set({
        llmBackends: backends,
        activeBackendId: data.active_id,
        llmBackendLoading: false,
      })
      fetchCache.markFetched('llmBackends')
    } catch (err) {
      const message = err instanceof Error ? err.message : 'Unknown error'
      set({ error: message, llmBackendLoading: false })
      fetchCache.invalidate('llmBackends')
    }
  },

  // Load available backend type definitions
  loadBackendTypes: async () => {
    if (!fetchCache.shouldFetch('llmBackendTypes')) return
    fetchCache.markFetching('llmBackendTypes')
    try {
      const data = await api.listLlmBackendTypes()
      set({ backendTypes: data.types || [] })
      fetchCache.markFetched('llmBackendTypes')
    } catch (err) {
      logError(err, { operation: 'Load backend types' })
      fetchCache.invalidate('llmBackendTypes')
    }
  },

  // Create a new backend
  createBackend: async (backend) => {
    set({ llmBackendLoading: true, error: null })
    try {
      const result = await api.createLlmBackend(backend)
      // Reload backends to get the full list
      fetchCache.invalidate('llmBackends')
      await get().loadBackends()
      set({ llmBackendLoading: false })
      return result.id
    } catch (err) {
      const message = err instanceof Error ? err.message : 'Unknown error'
      set({ error: message, llmBackendLoading: false })
      throw err
    }
  },

  // Update an existing backend
  updateBackend: async (id, updates) => {
    set({ llmBackendLoading: true, error: null })
    try {
      await api.updateLlmBackend(id, updates)
      // Update local state
      set((state) => ({
        llmBackends: state.llmBackends.map((b) =>
          b.id === id
            ? { ...b, ...updates, updated_at: Date.now() / 1000 }
            : b
        ),
        llmBackendLoading: false,
      }))
      return true
    } catch (err) {
      const message = err instanceof Error ? err.message : 'Unknown error'
      set({ error: message, llmBackendLoading: false })
      return false
    }
  },

  // Delete a backend
  deleteBackend: async (id) => {
    set({ llmBackendLoading: true, error: null })
    try {
      await api.deleteLlmBackend(id)
      // Update local state
      set((state) => ({
        llmBackends: state.llmBackends.filter((b) => b.id !== id),
        activeBackendId: state.activeBackendId === id ? null : state.activeBackendId,
        llmBackendLoading: false,
      }))
      // Clear test result
      get().clearTestResult(id)
      return true
    } catch (err) {
      const message = err instanceof Error ? err.message : 'Unknown error'
      set({ error: message, llmBackendLoading: false })
      return false
    }
  },

  // Set a backend as active
  activateBackend: async (id) => {
    set({ llmBackendLoading: true, error: null })
    try {
      await api.activateLlmBackend(id)
      // Update local state
      set({
        activeBackendId: id,
        llmBackends: get().llmBackends.map((b) => ({
          ...b,
          is_active: b.id === id,
        })),
        llmBackendLoading: false,
      })
      return true
    } catch (err) {
      const message = err instanceof Error ? err.message : 'Unknown error'
      set({ error: message, llmBackendLoading: false })
      return false
    }
  },

  // Get the currently active backend
  getActiveBackend: () => {
    const { llmBackends, activeBackendId } = get()
    return llmBackends.find((b) => b.id === activeBackendId) || null
  },

  // Test a backend connection
  testBackend: async (id) => {
    set((state) => ({
      testResults: {
        ...state.testResults,
        [id]: { success: false },  // Pending state
      },
    }))
    try {
      const response = await api.testLlmBackend(id)
      const result = response.result
      // Cache result
      set((state) => ({
        testResults: {
          ...state.testResults,
          [id]: result,
        },
        // Update healthy status on the backend
        llmBackends: state.llmBackends.map((b) =>
          b.id === id
            ? { ...b, healthy: result.success }
            : b
        ),
      }))
      return result
    } catch (err) {
      const result: BackendTestResult = {
        success: false,
        error: err instanceof Error ? err.message : 'Unknown error',
      }
      set((state) => ({
        testResults: {
          ...state.testResults,
          [id]: result,
        },
      }))
      return result
    }
  },

  // Clear a test result from cache
  clearTestResult: (id) => {
    set((state) => {
      const newResults = { ...state.testResults }
      delete newResults[id]
      return { testResults: newResults }
    })
  },

  // Set error
  setError: (error) => set({ error }),

  // Clear error
  clearError: () => set({ error: null }),
})
