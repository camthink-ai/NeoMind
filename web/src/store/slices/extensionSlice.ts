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
  ExtensionStatsDto,
  ExtensionTypeDto,
  ExtensionDiscoveryResult,
  ExtensionCommandDescriptor,
  ExtensionDataSourceInfo,
  ExtensionExecuteRequest,
  ExtensionExecuteResponse,
  ExtensionQueryParams,
  ExtensionQueryResult,
  TransformDataSourceInfo,
} from '@/types'
import { api } from '@/lib/api'
import { logError } from '@/lib/errors'

export interface ExtensionState {
  // Unified Extension State
  extensions: Extension[]
  selectedExtension: Extension | null
  extensionsLoading: boolean
  extensionDialogOpen: boolean
  discovering: boolean
  extensionStats: Record<string, ExtensionStatsDto>
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
  registerExtension: (extension: { file_path: string; auto_start?: boolean }) => Promise<void>
  unregisterExtension: (id: string) => Promise<boolean>
  startExtension: (id: string) => Promise<boolean>
  stopExtension: (id: string) => Promise<boolean>
  reloadExtension: (id: string) => Promise<boolean>
  getExtensionStats: (id: string) => Promise<ExtensionStatsDto | null>
  getExtensionHealth: (id: string) => Promise<{ healthy: boolean } | null>
  discoverExtensions: () => Promise<{ discovered: number; results: ExtensionDiscoveryResult[] }>
  fetchExtensionTypes: () => Promise<void>
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
    set({ extensionsLoading: true })
    try {
      const extensions = await api.listExtensions(params)
      set({ extensions })
      // Cache commands for each extension
      const commandsMap: Record<string, ExtensionCommandDescriptor[]> = {}
      extensions.forEach((ext) => {
        commandsMap[ext.id] = ext.commands
      })
      set({ commands: commandsMap })
    } catch (error) {
      logError(error, { operation: 'Fetch extensions' })
      set({ extensions: [], commands: {} })
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

  // Register new extension
  // Backend: POST /api/extensions -> { message, extension_id, name, version, auto_start, note }
  // Throws on error with message from API
  registerExtension: async (extension) => {
    await api.registerExtension(extension)
    // Refresh the list after successful registration
    await get().fetchExtensions()
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

  // Reload extension
  // Backend: POST /api/extensions/:id/reload -> { message, extension_id, config_applied }
  reloadExtension: async (id) => {
    try {
      await api.reloadExtension(id)
      // Refresh extension data after reload
      await get().fetchExtensions()
      return true
    } catch (error) {
      logError(error, { operation: 'Reload extension' })
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
      return {
        success: false,
        output: {},
        outputs: [],
        duration_ms: 0,
        error: 'Command execution failed',
      }
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
})
