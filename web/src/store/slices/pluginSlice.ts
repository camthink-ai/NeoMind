/**
 * Plugin Slice
 *
 * Handles plugin state, registration, and management.
 *
 * Matches backend API: crates/api/src/handlers/plugins.rs
 */

import type { StateCreator } from 'zustand'
import type { Plugin, PluginStatsDto, AdapterPluginDto, AdapterDeviceDto } from '@/types'
import { api } from '@/lib/api'

export interface PluginState {
  plugins: Plugin[]
  selectedPlugin: Plugin | null
  pluginsLoading: boolean
  pluginDialogOpen: boolean
  configDialogOpen: boolean
  commandDialogOpen: boolean
  discovering: boolean
  pluginStats: Record<string, PluginStatsDto>
  // Device adapter plugin state
  deviceAdapters: AdapterPluginDto[]
  deviceAdaptersLoading: boolean
  adapterDialogOpen: boolean
  selectedAdapterDevices: AdapterDeviceDto[]
  selectedAdapterDevicesLoading: boolean
}

export interface PluginSlice extends PluginState {
  // Actions
  setSelectedPlugin: (plugin: Plugin | null) => void
  setPluginDialogOpen: (open: boolean) => void
  setConfigDialogOpen: (open: boolean) => void
  setCommandDialogOpen: (open: boolean) => void
  setAdapterDialogOpen: (open: boolean) => void

  fetchPlugins: (params?: { type?: string; state?: string; enabled?: boolean }) => Promise<void>
  getPlugin: (id: string) => Promise<Plugin | null>
  registerPlugin: (plugin: {
    id: string
    name: string
    plugin_type: string
    description?: string
    path?: string
    config?: Record<string, unknown>
    auto_start?: boolean
    enabled?: boolean
  }) => Promise<boolean>
  unregisterPlugin: (id: string) => Promise<boolean>
  enablePlugin: (id: string) => Promise<boolean>
  disablePlugin: (id: string) => Promise<boolean>
  startPlugin: (id: string) => Promise<boolean>
  stopPlugin: (id: string) => Promise<boolean>
  getPluginConfig: (id: string) => Promise<Record<string, unknown> | null>
  updatePluginConfig: (id: string, config: Record<string, unknown>, reload?: boolean) => Promise<boolean>
  executePluginCommand: (id: string, command: string, parameters?: Record<string, unknown>) => Promise<{ success: boolean; result?: unknown; message?: string }>
  getPluginStats: (id: string) => Promise<PluginStatsDto | null>
  getPluginHealth: (id: string) => Promise<{ status: string; state: string } | null>
  discoverPlugins: () => Promise<{ discovered: number; message: string }>
  listPluginsByType: (type: string) => Promise<Plugin[]>
  getPluginTypesSummary: () => Promise<Record<string, number> | null>

  // Device adapter plugin actions
  fetchDeviceAdapters: () => Promise<void>
  registerDeviceAdapter: (adapter: {
    id: string
    name: string
    adapter_type: string
    config?: Record<string, unknown>
    auto_start?: boolean
    enabled?: boolean
  }) => Promise<boolean>
  getAdapterDevices: (pluginId: string) => Promise<AdapterDeviceDto[]>
  getDeviceAdapterStats: () => Promise<{ total_adapters: number; running_adapters: number; total_devices: number } | null>
}

export const createPluginSlice: StateCreator<
  PluginSlice,
  [],
  [],
  PluginSlice
> = (set, get) => ({
  // Initial state
  plugins: [],
  selectedPlugin: null,
  pluginsLoading: false,
  pluginDialogOpen: false,
  configDialogOpen: false,
  commandDialogOpen: false,
  discovering: false,
  pluginStats: {},
  // Device adapter plugin state
  deviceAdapters: [],
  deviceAdaptersLoading: false,
  adapterDialogOpen: false,
  selectedAdapterDevices: [],
  selectedAdapterDevicesLoading: false,

  // Dialog actions
  setSelectedPlugin: (plugin) => set({ selectedPlugin: plugin }),
  setPluginDialogOpen: (open) => set({ pluginDialogOpen: open }),
  setConfigDialogOpen: (open) => set({ configDialogOpen: open }),
  setCommandDialogOpen: (open) => set({ commandDialogOpen: open }),
  setAdapterDialogOpen: (open) => set({ adapterDialogOpen: open }),

  // Fetch all plugins
  // Backend: GET /api/plugins -> { plugins: PluginDto[], count: number }
  fetchPlugins: async (params) => {
    set({ pluginsLoading: true })
    try {
      const response = await api.listPlugins(params)
      // Backend response: { plugins: [...], count: N }
      set({ plugins: response.plugins || [] })
    } catch (error) {
      console.error('Failed to fetch plugins:', error)
      set({ plugins: [] })
    } finally {
      set({ pluginsLoading: false })
    }
  },

  // Get single plugin
  // Backend: GET /api/plugins/:id -> { plugin: PluginDto }
  getPlugin: async (id) => {
    try {
      const response = await api.getPlugin(id)
      return response.plugin
    } catch (error) {
      console.error('Failed to fetch plugin:', error)
      return null
    }
  },

  // Register new plugin
  // Backend: POST /api/plugins -> { message: string, plugin_id: string }
  registerPlugin: async (plugin) => {
    try {
      await api.registerPlugin(plugin)
      // Refresh the list after successful registration
      await get().fetchPlugins()
      return true
    } catch (error) {
      console.error('Failed to register plugin:', error)
      return false
    }
  },

  // Unregister plugin
  // Backend: DELETE /api/plugins/:id -> { message: string }
  unregisterPlugin: async (id) => {
    try {
      await api.unregisterPlugin(id)
      // Remove from list and clear stats
      set((state) => ({
        plugins: state.plugins.filter((p) => p.id !== id),
        pluginStats: Object.fromEntries(
          Object.entries(state.pluginStats).filter(([key]) => key !== id)
        ) as Record<string, PluginStatsDto>,
      }))
      return true
    } catch (error) {
      console.error('Failed to unregister plugin:', error)
      return false
    }
  },

  // Enable plugin
  // Backend: POST /api/plugins/:id/enable -> { message: string }
  enablePlugin: async (id) => {
    try {
      await api.enablePlugin(id)
      set((state) => ({
        plugins: state.plugins.map((p) =>
          p.id === id ? { ...p, enabled: true } : p
        ),
      }))
      return true
    } catch (error) {
      console.error('Failed to enable plugin:', error)
      return false
    }
  },

  // Disable plugin
  // Backend: POST /api/plugins/:id/disable -> { message: string }
  disablePlugin: async (id) => {
    try {
      await api.disablePlugin(id)
      set((state) => ({
        plugins: state.plugins.map((p) =>
          p.id === id ? { ...p, enabled: false } : p
        ),
      }))
      return true
    } catch (error) {
      console.error('Failed to disable plugin:', error)
      return false
    }
  },

  // Start plugin
  // Backend: POST /api/plugins/:id/start -> { message: string }
  startPlugin: async (id) => {
    try {
      await api.startPlugin(id)
      set((state) => ({
        plugins: state.plugins.map((p) =>
          p.id === id ? { ...p, state: 'Running', running: true } : p
        ),
      }))
      await get().getPluginStats(id)
      return true
    } catch (error) {
      console.error('Failed to start plugin:', error)
      return false
    }
  },

  // Stop plugin
  // Backend: POST /api/plugins/:id/stop -> { message: string }
  stopPlugin: async (id) => {
    try {
      await api.stopPlugin(id)
      set((state) => ({
        plugins: state.plugins.map((p) =>
          p.id === id ? { ...p, state: 'Stopped', running: false } : p
        ),
      }))
      await get().getPluginStats(id)
      return true
    } catch (error) {
      console.error('Failed to stop plugin:', error)
      return false
    }
  },

  // Get plugin config
  // Backend: GET /api/plugins/:id/config -> { plugin_id: string, config: json }
  getPluginConfig: async (id) => {
    try {
      const response = await api.getPluginConfig(id)
      return response.config
    } catch (error) {
      console.error('Failed to fetch plugin config:', error)
      return null
    }
  },

  // Update plugin config
  // Backend: PUT /api/plugins/:id/config -> { message: string }
  updatePluginConfig: async (id, config, reload) => {
    try {
      await api.updatePluginConfig(id, config, reload)
      return true
    } catch (error) {
      console.error('Failed to update plugin config:', error)
      return false
    }
  },

  // Execute plugin command
  // Backend: POST /api/plugins/:id/command -> { result: unknown }
  executePluginCommand: async (id, command, parameters) => {
    try {
      const response = await api.executePluginCommand(id, command, parameters)
      // Refresh stats after command execution
      await get().getPluginStats(id)
      return { success: true, result: response.result }
    } catch (error) {
      console.error('Failed to execute plugin command:', error)
      return { success: false, message: 'Command execution failed' }
    }
  },

  // Get plugin stats
  // Backend: GET /api/plugins/:id/stats -> { plugin_id: string, stats: PluginStats }
  getPluginStats: async (id) => {
    try {
      const response = await api.getPluginStats(id)
      set((state) => ({
        pluginStats: { ...state.pluginStats, [id]: response.stats },
      }))
      return response.stats
    } catch (error) {
      console.error('Failed to fetch plugin stats:', error)
      return null
    }
  },

  // Get plugin health
  // Backend: GET /api/plugins/:id/health -> { status: string, plugin_id: string, state: string }
  getPluginHealth: async (id) => {
    try {
      const response = await api.getPluginHealth(id)
      return { status: response.status, state: response.state }
    } catch (error) {
      console.error('Failed to fetch plugin health:', error)
      return null
    }
  },

  // Discover plugins
  // Backend: POST /api/plugins/discover -> { message: string, count: number }
  discoverPlugins: async () => {
    set({ discovering: true })
    try {
      const response = await api.discoverPlugins()
      // Refresh the plugin list after discovery
      await get().fetchPlugins()
      return { discovered: response.count || 0, message: response.message }
    } catch (error) {
      console.error('Failed to discover plugins:', error)
      return { discovered: 0, message: 'Discovery failed' }
    } finally {
      set({ discovering: false })
    }
  },

  // List plugins by type
  // Backend: GET /api/plugins/type/:type -> { plugin_type: string, plugins: PluginDto[], count: number }
  listPluginsByType: async (type) => {
    try {
      const response = await api.listPluginsByType(type)
      return response.plugins || []
    } catch (error) {
      console.error('Failed to list plugins by type:', error)
      return []
    }
  },

  // Get plugin types summary
  // Backend: GET /api/plugins/types -> { types: Record<string, number>, total: number }
  getPluginTypesSummary: async () => {
    try {
      const response = await api.getPluginTypesSummary()
      return response.types
    } catch (error) {
      console.error('Failed to get plugin types summary:', error)
      return null
    }
  },

  // ========== Device Adapter Plugin Actions ==========

  // Fetch all device adapter plugins
  // Backend: GET /api/plugins/device-adapters -> { total_adapters, running_adapters, total_devices, adapters: AdapterPluginDto[] }
  fetchDeviceAdapters: async () => {
    set({ deviceAdaptersLoading: true })
    try {
      const response = await api.listDeviceAdapters()
      set({ deviceAdapters: response.adapters || [] })
    } catch (error) {
      console.error('Failed to fetch device adapters:', error)
      set({ deviceAdapters: [] })
    } finally {
      set({ deviceAdaptersLoading: false })
    }
  },

  // Register a new device adapter plugin
  // Backend: POST /api/plugins/device-adapters -> { message: string, plugin_id: string }
  registerDeviceAdapter: async (adapter) => {
    try {
      await api.registerDeviceAdapter(adapter)
      // Refresh the list
      await get().fetchDeviceAdapters()
      return true
    } catch (error) {
      console.error('Failed to register device adapter:', error)
      return false
    }
  },

  // Get devices managed by an adapter plugin
  // Backend: GET /api/plugins/:id/devices -> { plugin_id: string, devices: AdapterDeviceDto[], count: number }
  getAdapterDevices: async (pluginId) => {
    set({ selectedAdapterDevicesLoading: true })
    try {
      const response = await api.getAdapterDevices(pluginId)
      set({ selectedAdapterDevices: response.devices || [] })
      return response.devices || []
    } catch (error) {
      console.error('Failed to fetch adapter devices:', error)
      set({ selectedAdapterDevices: [] })
      return []
    } finally {
      set({ selectedAdapterDevicesLoading: false })
    }
  },

  // Get device adapter statistics
  // Backend: GET /api/plugins/device-adapters/stats -> { total_adapters, running_adapters, total_devices, adapters: AdapterPluginDto[] }
  getDeviceAdapterStats: async () => {
    try {
      const response = await api.getDeviceAdapterStats()
      return {
        total_adapters: response.total_adapters,
        running_adapters: response.running_adapters,
        total_devices: response.total_devices,
      }
    } catch (error) {
      console.error('Failed to fetch device adapter stats:', error)
      return null
    }
  },
})
