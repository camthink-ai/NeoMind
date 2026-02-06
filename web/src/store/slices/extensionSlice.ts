/**
 * Extension Slice
 *
 * Handles extension state, registration, and management.
 *
 * Matches backend API: crates/api/src/handlers/extensions.rs
 *
 * This replaces the legacy plugin system for dynamically loaded code modules.
 * Device adapters are built-in (mqtt, http, webhook) and managed via the devices API.
 */

import type { StateCreator } from 'zustand'
import type { Extension, ExtensionStatsDto, ExtensionTypeDto, ExtensionDiscoveryResult } from '@/types'
import { api } from '@/lib/api'
import { logError } from '@/lib/errors'

export interface ExtensionState {
  extensions: Extension[]
  selectedExtension: Extension | null
  extensionsLoading: boolean
  extensionDialogOpen: boolean
  discovering: boolean
  extensionStats: Record<string, ExtensionStatsDto>
  extensionTypes: ExtensionTypeDto[]
}

export interface ExtensionSlice extends ExtensionState {
  // Dialog actions
  setSelectedExtension: (extension: Extension | null) => void
  setExtensionDialogOpen: (open: boolean) => void

  // Extension actions
  fetchExtensions: (params?: { extension_type?: string; state?: string }) => Promise<void>
  getExtension: (id: string) => Promise<Extension | null>
  registerExtension: (extension: { file_path: string; auto_start?: boolean }) => Promise<boolean>
  unregisterExtension: (id: string) => Promise<boolean>
  startExtension: (id: string) => Promise<boolean>
  stopExtension: (id: string) => Promise<boolean>
  getExtensionStats: (id: string) => Promise<ExtensionStatsDto | null>
  getExtensionHealth: (id: string) => Promise<{ healthy: boolean } | null>
  discoverExtensions: () => Promise<{ discovered: number; results: ExtensionDiscoveryResult[] }>
  fetchExtensionTypes: () => Promise<void>
  executeExtensionCommand: (id: string, command: string, args?: Record<string, unknown>) => Promise<{ success: boolean; result?: unknown; message?: string }>
}

export const createExtensionSlice: StateCreator<
  ExtensionSlice,
  [],
  [],
  ExtensionSlice
> = (set, get) => ({
  // Initial state
  extensions: [],
  selectedExtension: null,
  extensionsLoading: false,
  extensionDialogOpen: false,
  discovering: false,
  extensionStats: {},
  extensionTypes: [],

  // Dialog actions
  setSelectedExtension: (extension) => set({ selectedExtension: extension }),
  setExtensionDialogOpen: (open) => set({ extensionDialogOpen: open }),

  // ========== Extension Actions ==========

  // Fetch all extensions
  // Backend: GET /api/extensions -> ExtensionDto[]
  fetchExtensions: async (params) => {
    set({ extensionsLoading: true })
    try {
      const extensions = await api.listExtensions(params)
      set({ extensions })
    } catch (error) {
      logError(error, { operation: 'Fetch extensions' })
      set({ extensions: [] })
    } finally {
      set({ extensionsLoading: false })
    }
  },

  // Get single extension
  // Backend: GET /api/extensions/:id -> ExtensionDto
  getExtension: async (id) => {
    try {
      const extension = await api.getExtension(id)
      return extension
    } catch (error) {
      logError(error, { operation: 'Fetch extension' })
      return null
    }
  },

  // Register new extension
  // Backend: POST /api/extensions -> { message, extension_id, name, version, extension_type, note }
  registerExtension: async (extension) => {
    try {
      await api.registerExtension(extension)
      // Refresh the list after successful registration
      await get().fetchExtensions()
      return true
    } catch (error) {
      logError(error, { operation: 'Register extension' })
      return false
    }
  },

  // Unregister extension
  // Backend: DELETE /api/extensions/:id -> { message, extension_id }
  unregisterExtension: async (id) => {
    try {
      await api.unregisterExtension(id)
      // Remove from list and clear stats
      set((state) => ({
        extensions: state.extensions.filter((e) => e.id !== id),
        extensionStats: Object.fromEntries(
          Object.entries(state.extensionStats).filter(([key]) => key !== id)
        ) as Record<string, ExtensionStatsDto>,
      }))
      return true
    } catch (error) {
      logError(error, { operation: 'Unregister extension' })
      return false
    }
  },

  // Start extension
  // Backend: POST /api/extensions/:id/start -> { message, extension_id }
  startExtension: async (id) => {
    try {
      await api.startExtension(id)
      set((state) => ({
        extensions: state.extensions.map((e) =>
          e.id === id ? { ...e, state: 'Running' } : e
        ),
      }))
      await get().getExtensionStats(id)
      return true
    } catch (error) {
      logError(error, { operation: 'Start extension' })
      return false
    }
  },

  // Stop extension
  // Backend: POST /api/extensions/:id/stop -> { message, extension_id }
  stopExtension: async (id) => {
    try {
      await api.stopExtension(id)
      set((state) => ({
        extensions: state.extensions.map((e) =>
          e.id === id ? { ...e, state: 'Stopped' } : e
        ),
      }))
      await get().getExtensionStats(id)
      return true
    } catch (error) {
      logError(error, { operation: 'Stop extension' })
      return false
    }
  },

  // Get extension stats
  // Backend: GET /api/extensions/:id/stats -> ExtensionStatsDto
  getExtensionStats: async (id) => {
    try {
      const stats = await api.getExtensionStats(id)
      set((state) => ({
        extensionStats: { ...state.extensionStats, [id]: stats },
      }))
      return stats
    } catch (error) {
      logError(error, { operation: 'Fetch extension stats' })
      return null
    }
  },

  // Get extension health
  // Backend: GET /api/extensions/:id/health -> { extension_id, healthy }
  getExtensionHealth: async (id) => {
    try {
      const response = await api.getExtensionHealth(id)
      return { healthy: response.healthy }
    } catch (error) {
      logError(error, { operation: 'Fetch extension health' })
      return null
    }
  },

  // Discover extensions
  // Backend: POST /api/extensions/discover -> ExtensionDiscoveryResult[]
  discoverExtensions: async () => {
    set({ discovering: true })
    try {
      const results = await api.discoverExtensions()
      // Refresh the extension list after discovery
      await get().fetchExtensions()
      return { discovered: results.length, results }
    } catch (error) {
      logError(error, { operation: 'Discover extensions' })
      return { discovered: 0, results: [] }
    } finally {
      set({ discovering: false })
    }
  },

  // Fetch extension types
  // Backend: GET /api/extensions/types -> ExtensionTypeDto[]
  fetchExtensionTypes: async () => {
    try {
      const types = await api.listExtensionTypes()
      set({ extensionTypes: types })
    } catch (error) {
      logError(error, { operation: 'Fetch extension types' })
      set({ extensionTypes: [] })
    }
  },

  // Execute extension command
  // Backend: POST /api/extensions/:id/command -> result
  executeExtensionCommand: async (id, command, args) => {
    try {
      const result = await api.executeExtensionCommand(id, command, args)
      // Refresh stats after command execution
      await get().getExtensionStats(id)
      return { success: true, result }
    } catch (error) {
      logError(error, { operation: 'Execute extension command' })
      return { success: false, message: 'Command execution failed' }
    }
  },
})
