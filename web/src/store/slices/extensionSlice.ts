/**
 * Extension Slice
 *
 * Handles extension state, registration, and management.
 *
 * Matches backend API: crates/api/src/handlers/extensions.rs
 *
 * Unified extension system - command-based API where each extension
 * exposes commands with JSON Schema input/output definitions.
 */

import type { StateCreator } from 'zustand'
import type {
  Extension,
  ExtensionTypeDto,
  ExtensionLogEntry,
  ExtensionCommandDescriptor,
  ExtensionDataSourceInfo,
  ExtensionExecuteRequest,
  ExtensionExecuteResponse,
  ExtensionQueryParams,
  ExtensionQueryResult,
  
} from '@/types'
import { api } from '@/lib/api'
import { logError } from '@/lib/errors'
import { dynamicRegistry } from '@/components/dashboard/registry/DynamicRegistry'
import { fetchCache } from '@/lib/utils/async'

export interface ExtensionState {
  // Unified Extension State
  extensions: Extension[]
  selectedExtension: Extension | null
  extensionsLoading: boolean
  extensionDialogOpen: boolean
  extensionTypes: ExtensionTypeDto[]

  // Commands and data sources (cached by extension_id)
  commands: Record<string, ExtensionCommandDescriptor[]>
  dataSources: Record<string, ExtensionDataSourceInfo[]>
}

export interface ExtensionSlice extends ExtensionState {
  // Dialog actions
  setSelectedExtension: (extension: Extension | null) => void
  setExtensionDialogOpen: (open: boolean) => void

  // Extension actions
  fetchExtensions: (params?: { state?: string }) => Promise<void>
  getExtension: (id: string) => Promise<Extension | null>
  unregisterExtension: (id: string) => Promise<boolean>
  startExtension: (id: string) => Promise<boolean>
  stopExtension: (id: string) => Promise<boolean>
  reloadExtension: (id: string) => Promise<boolean>
  getExtensionHealth: (id: string) => Promise<{ healthy: boolean } | null>
  fetchExtensionTypes: () => Promise<void>
  getExtensionLogs: (id: string) => Promise<ExtensionLogEntry[]>
  clearExtensionLogs: (id: string) => Promise<void>
  executeExtensionCommand: (id: string, command: string, args?: Record<string, unknown>) => Promise<{ success: boolean; result?: unknown; message?: string }>

  // Command and data source actions
  fetchCommands: (id: string) => Promise<ExtensionCommandDescriptor[]>
  executeCommand: (id: string, request: ExtensionExecuteRequest) => Promise<ExtensionExecuteResponse>
  fetchDataSources: (id: string) => Promise<ExtensionDataSourceInfo[]>
  fetchAllDataSources: () => Promise<ExtensionDataSourceInfo[]>
  queryData: (params: ExtensionQueryParams) => Promise<ExtensionQueryResult>

  // Convenience aliases for backward compatibility
  extensionDataSources: ExtensionDataSourceInfo[]
  fetchExtensionDataSources: () => Promise<ExtensionDataSourceInfo[]>

  // Direct setter for extension data sources (used by UnifiedDataSourceConfig to avoid duplicate API calls)
  setExtensionDataSources: (sources: ExtensionDataSourceInfo[]) => void
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
  extensionTypes: [],
  commands: {},
  dataSources: {},
  extensionDataSources: [],

  // Dialog actions
  setSelectedExtension: (extension) => set({ selectedExtension: extension }),
  setExtensionDialogOpen: (open) => set({ extensionDialogOpen: open }),

  // ========== Extension Actions ==========

  // Fetch all extensions with their commands
  // Backend: GET /api/extensions -> Extension[]
  fetchExtensions: async (params) => {
    if (!fetchCache.shouldFetch('extensions')) return
    fetchCache.markFetching('extensions')
    set({ extensionsLoading: true })
    try {
      const extensions = await api.listExtensions(params)
      set({ extensions })
      // Cache commands for each extension
      const commandsMap: Record<string, ExtensionCommandDescriptor[]> = {}
      ;(extensions || []).forEach((ext) => {
        commandsMap[ext.id] = ext.commands
      })
      set({ commands: commandsMap })
      fetchCache.markFetched('extensions')
    } catch (error) {
      logError(error, { operation: 'Fetch extensions' })
      set({ extensions: [], commands: {} })
      fetchCache.invalidate('extensions')
    } finally {
      set({ extensionsLoading: false })
    }
  },

  // Get single extension
  // Backend: GET /api/extensions/:id -> Extension
  getExtension: async (id) => {
    try {
      const extension = await api.getExtension(id)
      return extension
    } catch (error) {
      logError(error, { operation: 'Fetch extension' })
      return null
    }
  },

  // Unregister extension
  // Backend: DELETE /api/extensions/:id -> { message, extension_id }
  unregisterExtension: async (id) => {
    try {
      await api.unregisterExtension(id)
      // Clear dynamic registry caches and global variables for this extension
      dynamicRegistry.unregisterExtension(id)
      // Close and clean up extension stream client to prevent memory leak
      const { closeExtensionStreamClient } = await import('@/lib/extension-stream')
      closeExtensionStreamClient(id)
      // Remove from list and clear caches
      set((state) => ({
        extensions: state.extensions.filter((e) => e.id !== id),
        commands: Object.fromEntries(
          Object.entries(state.commands).filter(([key]) => key !== id)
        ),
        dataSources: Object.fromEntries(
          Object.entries(state.dataSources).filter(([key]) => key !== id)
        ),
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
      return true
    } catch (error) {
      logError(error, { operation: 'Stop extension' })
      return false
    }
  },

  // Reload extension
  // Backend: POST /api/extensions/:id/reload -> { message, extension_id, config_applied }
  reloadExtension: async (id) => {
    try {
      // Clean up stale frontend state before reload
      dynamicRegistry.unregisterExtension(id)
      try {
        const { closeExtensionStreamClient } = await import('@/lib/extension-stream')
        closeExtensionStreamClient(id)
      } catch {
        // Stream may not exist for this extension, safe to ignore
      }

      await api.reloadExtension(id)

      // Brief pause to let the new process fully initialize before we re-fetch
      await new Promise(resolve => setTimeout(resolve, 300))

      // Refresh extension data after reload
      fetchCache.invalidate('extensions')
      await get().fetchExtensions()
      return true
    } catch (error) {
      logError(error, { operation: 'Reload extension' })
      return false
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

  // Fetch extension types
  // Backend: GET /api/extensions/types -> ExtensionTypeDto[]
  fetchExtensionTypes: async () => {
    if (!fetchCache.shouldFetch('extensionTypes')) return
    fetchCache.markFetching('extensionTypes')
    try {
      const types = await api.listExtensionTypes()
      set({ extensionTypes: types || [] })
      fetchCache.markFetched('extensionTypes')
    } catch (error) {
      logError(error, { operation: 'Fetch extension types' })
      set({ extensionTypes: [] })
      fetchCache.invalidate('extensionTypes')
    }
  },

  // Get extension logs
  // Backend: GET /api/extensions/:id/logs -> ExtensionLogEntry[]
  getExtensionLogs: async (id) => {
    try {
      const logs = await api.getExtensionLogs(id)
      return logs || []
    } catch (error) {
      logError(error, { operation: 'Fetch extension logs' })
      return []
    }
  },

  // Clear extension logs
  // Backend: DELETE /api/extensions/:id/logs
  clearExtensionLogs: async (id) => {
    try {
      await api.clearExtensionLogs(id)
    } catch (error) {
      logError(error, { operation: 'Clear extension logs' })
    }
  },

  // Execute extension command
  // Backend: POST /api/extensions/:id/command -> result
  executeExtensionCommand: async (id, command, args) => {
    try {
      const result = await api.executeExtensionCommand(id, command, args)
      return { success: true, result }
    } catch (error) {
      logError(error, { operation: 'Execute extension command' })
      return { success: false, message: 'Command execution failed' }
    }
  },

  // ========== Command and Data Source Actions ==========

  // Fetch commands for an extension
  // Backend: GET /api/extensions/:id/commands -> CommandDescriptor[]
  fetchCommands: async (id) => {
    try {
      const commands = await api.listCommands(id)
      // Cache the commands
      set((state) => ({
        commands: { ...state.commands, [id]: commands },
      }))
      return commands
    } catch (error) {
      logError(error, { operation: 'Fetch commands' })
      return []
    }
  },

  // Execute an extension command
  // Backend: POST /api/extensions/:id/command -> ExtensionExecuteResponse
  executeCommand: async (id, request) => {
    try {
      const response = await api.executeCommand(id, request)
      return response
    } catch (error) {
      logError(error, { operation: 'Execute command' })
      return { error: 'Command execution failed' }
    }
  },

  // Fetch data sources for an extension
  // Backend: GET /api/extensions/:id/datasources -> ExtensionDataSourceInfo[]
  fetchDataSources: async (id) => {
    try {
      const dataSources = await api.listDataSources(id)
      // Cache the data sources
      set((state) => ({
        dataSources: { ...state.dataSources, [id]: dataSources },
      }))
      return dataSources
    } catch (error) {
      logError(error, { operation: 'Fetch data sources' })
      return []
    }
  },

  // Fetch all data sources (for dashboard, rules, etc.)
  // Backend: GET /api/extensions/datasources -> (ExtensionDataSourceInfo | TransformDataSourceInfo)[]
  fetchAllDataSources: async () => {
    try {
      const allSources = await api.listAllDataSources()
      // Filter only extension data sources (exclude transform data sources)
      const dataSources = allSources.filter(
        (source): source is ExtensionDataSourceInfo => 'extension_id' in source
      )
      // Group by extension_id for caching
      const grouped: Record<string, ExtensionDataSourceInfo[]> = {}
      dataSources.forEach((ds) => {
        if (!grouped[ds.extension_id]) {
          grouped[ds.extension_id] = []
        }
        grouped[ds.extension_id].push(ds)
      })
      set((state) => ({
        dataSources: { ...state.dataSources, ...grouped },
        // Also update the flat array for convenience
        extensionDataSources: dataSources,
      }))
      return dataSources
    } catch (error) {
      logError(error, { operation: 'Fetch all data sources' })
      return []
    }
  },

  // Query data from an extension
  // Backend: POST /api/extensions/query -> ExtensionQueryResult
  queryData: async (params) => {
    try {
      const result = await api.queryData(params)
      return result
    } catch (error) {
      logError(error, { operation: 'Query data' })
      return {
        source_id: '',
        data_points: [],
      }
    }
  },

  // ========== Convenience Properties ==========

  // Alias for fetchAllDataSources
  fetchExtensionDataSources: async () => {
    const result = await get().fetchAllDataSources()
    // Update the flat array
    set({ extensionDataSources: result })
    return result
  },

  // Direct setter to populate extension data sources without extra API call
  setExtensionDataSources: (sources) => {
    // Group by extension_id for caching
    const grouped: Record<string, ExtensionDataSourceInfo[]> = {}
    sources.forEach((ds) => {
      if (!grouped[ds.extension_id]) {
        grouped[ds.extension_id] = []
      }
      grouped[ds.extension_id].push(ds)
    })
    set((state) => ({
      dataSources: { ...state.dataSources, ...grouped },
      extensionDataSources: sources,
    }))
  },
})
