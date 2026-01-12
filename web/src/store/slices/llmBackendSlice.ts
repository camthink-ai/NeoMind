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
    set({ llmBackendLoading: true, error: null })
    try {
      const data = await api.listLlmBackends()
      set({
        llmBackends: data.backends,
        activeBackendId: data.active_id,
        llmBackendLoading: false,
      })
    } catch (err) {
      const message = err instanceof Error ? err.message : 'Unknown error'
      set({ error: message, llmBackendLoading: false })
    }
  },

  // Load available backend type definitions
  loadBackendTypes: async () => {
    try {
      const data = await api.listLlmBackendTypes()
      set({ backendTypes: data.types })
    } catch (err) {
      console.error('Failed to load backend types:', err)
      // Don't set error state for this - it's not critical
    }
  },

  // Create a new backend
  createBackend: async (backend) => {
    set({ llmBackendLoading: true, error: null })
    try {
      const result = await api.createLlmBackend(backend)
      // Reload backends to get the full list
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
