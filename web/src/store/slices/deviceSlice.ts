/**
 * Device Slice
 *
 * Handles device state, device types, discovery, and telemetry.
 */

import type { StateCreator } from 'zustand'
import type {
  DeviceState,
  TelemetryState,
} from '../types'
import type {
  Device,
  DeviceType,
  DiscoveredDevice,
  AddDeviceRequest,
} from '@/types'
import { api } from '@/lib/api'
import { logError } from '@/lib/errors'

export interface DeviceSlice extends DeviceState, TelemetryState {
  // Actions
  setSelectedDevice: (device: Device | null) => void
  setSelectedDeviceId: (id: string | null) => void
  setAddDeviceDialogOpen: (open: boolean) => void
  setAddDeviceTypeDialogOpen: (open: boolean) => void
  setDeviceDetailsDialogOpen: (open: boolean) => void

  fetchDevices: () => Promise<void>
  fetchDeviceTypes: () => Promise<void>
  addDevice: (request: AddDeviceRequest) => Promise<boolean>
  deleteDevice: (id: string) => Promise<boolean>
  sendCommand: (deviceId: string, command: string, params?: Record<string, unknown>) => Promise<boolean>

  addDeviceType: (definition: DeviceType) => Promise<boolean>
  deleteDeviceType: (id: string) => Promise<boolean>
  validateDeviceType: (definition: DeviceType) => Promise<{ valid: boolean; errors?: string[]; warnings?: string[]; message: string }>
  generateMDL: (req: { device_name: string; description?: string; uplink_example: string; downlink_example?: string }) => Promise<DeviceType>

  fetchDeviceDetails: (id: string) => Promise<Device | null>
  fetchDeviceTypeDetails: (deviceType: string) => Promise<DeviceType | null>

  discoverDevices: (host: string, ports?: number[], timeoutMs?: number) => Promise<void>
  setDiscoveredDevices: (devices: DiscoveredDevice[]) => void

  fetchTelemetryData: (deviceId: string, metric?: string, start?: number, end?: number, limit?: number) => Promise<void>
  fetchTelemetrySummary: (deviceId: string, hours?: number) => Promise<void>
  fetchDeviceCurrentState: (deviceId: string) => Promise<void>  // New: unified device + metrics
  fetchDevicesCurrentBatch: (deviceIds: string[]) => Promise<void>  // Batch fetch for dashboard
  fetchCommandHistory: (deviceId: string, limit?: number) => Promise<void>

  // Device status update from events
  updateDeviceStatus: (deviceId: string, status: 'online' | 'offline') => void
  // Update device metric from real-time events
  updateDeviceMetric: (deviceId: string, property: string, value: unknown) => void
}

export const createDeviceSlice: StateCreator<
  DeviceSlice,
  [],
  [],
  DeviceSlice
> = (set, get) => ({
  // Initial state
  devices: [],
  deviceTypes: [],
  selectedDevice: null,
  selectedDeviceId: null,
  deviceDetails: null,
  deviceTypeDetails: null,
  discovering: false,
  discoveredDevices: [],
  devicesLoading: false,
  deviceTypesLoading: false,
  addDeviceDialogOpen: false,
  addDeviceTypeDialogOpen: false,
  deviceDetailsDialogOpen: false,

  // Telemetry state
  telemetryData: null,
  telemetrySummary: null,
  deviceCurrentState: null,
  commandHistory: null,
  telemetryLoading: false,

  // Dialog actions
  setSelectedDevice: (device) => set({ selectedDevice: device }),
  setSelectedDeviceId: (id) => set({ selectedDeviceId: id }),
  setAddDeviceDialogOpen: (open) => set({ addDeviceDialogOpen: open }),
  setAddDeviceTypeDialogOpen: (open) => set({ addDeviceTypeDialogOpen: open }),
  setDeviceDetailsDialogOpen: (open) => set({ deviceDetailsDialogOpen: open }),

  // Device CRUD
  fetchDevices: async () => {
    set({ devicesLoading: true })
    try {
      const data = await api.getDevices()

      // Sort by last_seen descending (newest first), online devices first
      const sortedDevices = (data.devices || []).sort((a, b) => {
        // Online devices first
        if (a.status === 'online' && b.status !== 'online') return -1
        if (a.status !== 'online' && b.status === 'online') return 1
        // Then by last_seen descending
        return new Date(b.last_seen).getTime() - new Date(a.last_seen).getTime()
      })
      set({ devices: sortedDevices })
    } catch (error) {
      if ((error as Error).message === 'UNAUTHORIZED') {
        // Will be handled by auth slice
      }
      logError(error, { operation: 'Fetch devices' })
      set({ devices: [] })
    } finally {
      set({ devicesLoading: false })
    }
  },

  fetchDeviceTypes: async () => {
    set({ deviceTypesLoading: true })
    try {
      const data = await api.getDeviceTypes()
      // Sort by created_at descending (newest first)
      const sortedTypes = (data.device_types || []).sort((a: any, b: any) => {
        const aTime = a.created_at ? new Date(a.created_at).getTime() : 0
        const bTime = b.created_at ? new Date(b.created_at).getTime() : 0
        return bTime - aTime
      })
      set({ deviceTypes: sortedTypes })
    } catch (error) {
      if ((error as Error).message === 'UNAUTHORIZED') {
        // Will be handled by auth slice
      }
      logError(error, { operation: 'Fetch device types' })
    } finally {
      set({ deviceTypesLoading: false })
    }
  },

  addDevice: async (request: AddDeviceRequest) => {
    try {
      const result = await api.addDevice(request)
      // Backend returns { device_id, added: true } after unwrap
      if (result.added || result.device_id) {
        await get().fetchDevices()
        return true
      }
      return false
    } catch (error) {
      if ((error as Error).message === 'UNAUTHORIZED') {
        // Will be handled by auth slice
      }
      logError(error, { operation: 'Add device' })
      return false
    }
  },

  deleteDevice: async (id) => {
    const result = await api.deleteDevice(id)
    // Backend returns { device_id, deleted: true } after unwrap
    if (result.deleted) {
      await get().fetchDevices()
      return true
    }
    return false
  },

  sendCommand: async (deviceId, command, params) => {
    try {
      const result = await api.sendCommand(deviceId, command, params)
      // Backend returns { device_id, command, sent: true } after unwrap
      if (result.sent) {
        await get().fetchDevices()
        return true
      }
      return false
    } catch (error) {
      logError(error, { operation: 'Send command' })
      return false
    }
  },

  // Device Type CRUD
  addDeviceType: async (definition) => {
    await api.addDeviceType(definition)
    await get().fetchDeviceTypes()
    return true
  },

  deleteDeviceType: async (id) => {
    await api.deleteDeviceType(id)
    await get().fetchDeviceTypes()
    return true
  },

  validateDeviceType: async (definition) => {
    try {
      return await api.validateDeviceType(definition)
    } catch (error) {
      logError(error, { operation: 'Validate device type' })
      return {
        valid: false,
        errors: [`验证失败: ${(error as Error).message}`],
        message: '验证请求失败'
      }
    }
  },

  generateMDL: async (req) => {
    try {
      return await api.generateMDL(req)
    } catch (error) {
      logError(error, { operation: 'Generate MDL' })
      throw error
    }
  },

  // Device Details
  fetchDeviceDetails: async (id) => {
    try {
      const details = await api.getDevice(id)
      set({ deviceDetails: details })
      return details
    } catch (error) {
      logError(error, { operation: 'Fetch device details' })
      return null
    }
  },

  fetchDeviceTypeDetails: async (deviceType) => {
    try {
      const details = await api.getDeviceType(deviceType)
      set({ deviceTypeDetails: details })
      return details
    } catch (error) {
      logError(error, { operation: 'Fetch device type details' })
      return null
    }
  },

  // Device Discovery
  discoverDevices: async (host, ports, timeoutMs) => {
    set({ discovering: true })
    try {
      const result = await api.discoverDevices(host, ports, timeoutMs)
      set({ discoveredDevices: result.devices || [] })
    } catch (error) {
      logError(error, { operation: 'Discover devices' })
      set({ discoveredDevices: [] })
    } finally {
      set({ discovering: false })
    }
  },

  setDiscoveredDevices: (devices) => set({ discoveredDevices: devices }),

  // Telemetry
  fetchTelemetryData: async (deviceId, metric, start, end, limit) => {
    set({ telemetryLoading: true })
    try {
      const data = await api.getDeviceTelemetry(deviceId, metric, start, end, limit)
      set({ telemetryData: data })
    } catch (error) {
      logError(error, { operation: 'Fetch telemetry data' })
      set({ telemetryData: null })
    } finally {
      set({ telemetryLoading: false })
    }
  },

  fetchTelemetrySummary: async (deviceId, hours) => {
    set({ telemetryLoading: true })
    try {
      const data = await api.getDeviceTelemetrySummary(deviceId, hours)
      set({ telemetrySummary: data })
    } catch (error) {
      logError(error, { operation: 'Fetch telemetry summary' })
      set({ telemetrySummary: null })
    } finally {
      set({ telemetryLoading: false })
    }
  },

  fetchDeviceCurrentState: async (deviceId) => {
    set({ telemetryLoading: true })
    try {
      const data = await api.getDeviceCurrent(deviceId)
      set({ deviceCurrentState: data })

      // Helper function to build nested object from flat key paths
      // Only includes metrics with non-null values
      const buildNestedValues = (metrics: Record<string, any>) => {
        const result: Record<string, unknown> = {}
        for (const [key, metricData] of Object.entries(metrics)) {
          // Skip null values - only store actual data
          if (metricData.value === null || metricData.value === undefined) {
            continue
          }
          const parts = key.split('.')
          let current = result
          for (let i = 0; i < parts.length - 1; i++) {
            const part = parts[i]
            if (!(part in current)) {
              current[part] = {}
            }
            current = current[part] as Record<string, unknown>
          }
          current[parts[parts.length - 1]] = metricData.value
        }
        return result
      }

      // Also update device in the devices list with current values
      // This keeps the devices list in sync with the latest data
      set((state) => ({
        devices: state.devices.map((d) =>
          d.id === deviceId || d.device_id === deviceId
            ? {
                ...d,
                current_values: buildNestedValues(data.metrics),
              }
            : d
        ),
      }))
    } catch (error) {
      logError(error, { operation: 'Fetch device current state' })
      set({ deviceCurrentState: null })
    } finally {
      set({ telemetryLoading: false })
    }
  },

  fetchCommandHistory: async (deviceId, limit) => {
    set({ telemetryLoading: true })
    try {
      const data = await api.getDeviceCommandHistory(deviceId, limit)
      set({ commandHistory: data })
    } catch (error) {
      logError(error, { operation: 'Fetch command history' })
      set({ commandHistory: null })
    } finally {
      set({ telemetryLoading: false })
    }
  },

  // Batch fetch current values for multiple devices
  // Optimized for dashboard - fetches all device current_values in one API call
  // Note: Silently skip if backend doesn't support this endpoint (405 error)
  fetchDevicesCurrentBatch: async (deviceIds) => {
    if (!deviceIds || deviceIds.length === 0) {
      return
    }

    try {
      const data = await api.getDevicesCurrentBatch(deviceIds)

      // Helper function to build nested object from flat key paths
      const buildNestedValues = (metrics: Record<string, unknown>) => {
        const result: Record<string, unknown> = {}
        for (const [key, value] of Object.entries(metrics)) {
          const parts = key.split('.')
          let current = result
          for (let i = 0; i < parts.length - 1; i++) {
            const part = parts[i]
            if (!(part in current)) {
              current[part] = {}
            }
            current = current[part] as Record<string, unknown>
          }
          current[parts[parts.length - 1]] = value
        }
        return result
      }

      // Update devices array with fetched current_values
      set((state) => ({
        devices: state.devices.map((device) => {
          const deviceData = data.devices[device.id || device.device_id]
          if (!deviceData) {
            return device
          }

          return {
            ...device,
            current_values: buildNestedValues(deviceData.current_values),
          }
        }),
      }))
    } catch (error) {
      const errorMessage = (error as Error).message
      // Silently ignore 405 errors - backend doesn't support this endpoint yet
      if (errorMessage.includes('405') || errorMessage.includes('Method Not Allowed')) {
        // Endpoint not implemented in backend - skip silently
        return
      }
      logError(error, { operation: 'Fetch devices current batch' })
    }
  },

  // Update device status from real-time events
  updateDeviceStatus: (deviceId: string, status: 'online' | 'offline') => {
    set((state) => ({
      devices: state.devices.map((device) =>
        device.id === deviceId || device.device_id === deviceId
          ? { ...device, status }
          : device
      ),
    }))
    // Also update selectedDevice if it matches
    set((state) => ({
      selectedDevice:
        state.selectedDevice?.id === deviceId ||
        state.selectedDevice?.device_id === deviceId
          ? { ...state.selectedDevice, status }
          : state.selectedDevice,
    }))
  },

  // Update device metric from real-time events
  // Supports nested property paths like "values.battery" or "temperature"
  // If device doesn't exist in store, silently skip (will be added by fetchDevices)
  updateDeviceMetric: (deviceId: string, property: string, value: unknown) => {
    // Helper function to set nested property
    const setNestedProperty = (obj: Record<string, unknown>, path: string, value: unknown) => {
      const parts = path.split('.')
      let current: Record<string, unknown> = obj
      for (let i = 0; i < parts.length - 1; i++) {
        const part = parts[i]
        if (!(part in current) || typeof current[part] !== 'object' || current[part] === null) {
          current[part] = {}
        }
        current = current[part] as Record<string, unknown>
      }
      current[parts[parts.length - 1]] = value
      return obj
    }

    // Single atomic update for both devices array and selectedDevice
    set((state) => {
      // Check if device exists
      const existingDevice = state.devices.find((d) => d.id === deviceId || d.device_id === deviceId)

      // If device doesn't exist, skip update (don't create placeholder)
      // This prevents accumulating invalid devices in the store
      if (!existingDevice) {
        return state
      }

      // Update existing device in devices array
      const updatedDevices = state.devices.map((device) => {
        if (device.id === deviceId || device.device_id === deviceId) {
          const currentValues = device.current_values || {}
          const updatedValues = setNestedProperty({ ...currentValues }, property, value)

          return {
            ...device,
            current_values: updatedValues,
            last_seen: new Date().toISOString(),
            status: 'online',  // Update status to online when receiving metrics
            online: true,
          }
        }
        return device
      })

      // Also update selectedDevice if it matches
      let updatedSelectedDevice = state.selectedDevice
      if (state.selectedDevice?.id === deviceId || state.selectedDevice?.device_id === deviceId) {
        const currentValues = state.selectedDevice.current_values || {}
        const updatedValues = setNestedProperty({ ...currentValues }, property, value)

        updatedSelectedDevice = {
          ...state.selectedDevice,
          current_values: updatedValues,
          last_seen: new Date().toISOString(),
        }
      }

      return {
        devices: updatedDevices,
        selectedDevice: updatedSelectedDevice,
      }
    })
  },
})
