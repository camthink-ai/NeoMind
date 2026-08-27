/**
 * Device Slice
 *
 * Handles device state, device types, and telemetry.
 */

import type { StateCreator } from 'zustand'
import type {
  DeviceState,
  TelemetryState,
} from '../types'
import type {
  Device,
  DeviceType,
  AddDeviceRequest,
} from '@/types'
import { api } from '@/lib/api'
import { logError } from '@/lib/errors'
import { BatchUpdater } from '@/lib/throttle'
import { fetchCache } from '@/lib/utils/async'
import { findDevice } from '@/lib/deviceUtils'

/** Status fragment shared by the device detail/list DTOs and event
 * payloads. The backend always includes these fields on device responses;
 * typed once here so consumers stop re-asserting each field via `as any`. */
interface DeviceStatusFragment {
  online?: boolean
  status?: string
  last_seen?: string
  transport_connected?: boolean
  transport_changed_at?: number
}

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
  updateDevice: (id: string, request: Partial<AddDeviceRequest>) => Promise<boolean>
  deleteDevice: (id: string) => Promise<boolean>
  sendCommand: (deviceId: string, command: string, params?: Record<string, unknown>) => Promise<boolean>

  addDeviceType: (definition: DeviceType) => Promise<boolean>
  deleteDeviceType: (id: string) => Promise<boolean>
  validateDeviceType: (definition: DeviceType) => Promise<{ valid: boolean; errors?: string[]; warnings?: string[]; message: string }>
  generateMDL: (req: { device_name: string; description?: string; uplink_example: string; downlink_example?: string }) => Promise<DeviceType>

  clearDeviceDetails: () => void
  fetchDeviceDetails: (id: string) => Promise<Device | null>
  fetchDeviceTypeDetails: (deviceType: string) => Promise<DeviceType | null>

  fetchTelemetryData: (deviceId: string, metric?: string, start?: number, end?: number, limit?: number, offset?: number) => Promise<void>
  fetchTelemetrySummary: (deviceId: string, hours?: number) => Promise<void>
  fetchDeviceCurrentState: (deviceId: string) => Promise<void>  // New: unified device + metrics
  fetchDevicesCurrentBatch: (deviceIds: string[], signal?: AbortSignal) => Promise<void>  // Batch fetch for dashboard
  fetchCommandHistory: (deviceId: string, limit?: number) => Promise<void>

  // Update device metric from real-time events
  updateDeviceStatus: (deviceId: string, status: 'online' | 'offline') => void
  // Lightweight "device is alive" ping from DeviceMetric events — updates
  // last_seen + online=true. Throttled per-device to at most once / 5s to
  // avoid excessive re-renders on high-frequency telemetry streams.
  touchDeviceActivity: (deviceId: string) => void
  // Update transport connection state from real-time events (MQTT session)
  updateDeviceTransportStatus: (deviceId: string, connected: boolean) => void
  // Update device metric from real-time events
  updateDeviceMetric: (deviceId: string, property: string, value: unknown) => void
  // Apply current_values batch directly (bypasses BatchUpdater RAF for instant store notification)
  _applyCurrentValuesBatch: (results: Record<string, unknown>, deviceIds: string[]) => void
}

// Module-level helper for setting nested properties immutably
const setNestedProperty = (obj: Record<string, unknown>, path: string, value: unknown): Record<string, unknown> => {
  const parts = path.split('.')

  // Create a completely new object tree with new references at every level
  const result = { ...obj }
  let current = result

  for (let i = 0; i < parts.length - 1; i++) {
    const part = parts[i]
    // Get the nested object (or create new one)
    const nestedObj = typeof current[part] === 'object' && current[part] !== null
      ? { ...(current[part] as Record<string, unknown>) }  // Create new reference
      : {}
    current[part] = nestedObj
    current = nestedObj
  }

  current[parts[parts.length - 1]] = value
  return result
}

// Helper to get a nested value by dot-separated path
const getNestedValue = (obj: Record<string, unknown>, path: string): unknown => {
  const parts = path.split('.')
  let current: unknown = obj
  for (const part of parts) {
    if (current === null || current === undefined || typeof current !== 'object') return undefined
    current = (current as Record<string, unknown>)[part]
  }
  return current
}

// Module-level helper to build nested object from flat dot-separated key paths.
// Skips entries with null/undefined values.
const buildNestedValues = (metrics: Record<string, unknown>): Record<string, unknown> => {
  const result: Record<string, unknown> = {}
  for (const [key, raw] of Object.entries(metrics)) {
    // For { value, timestamp } envelope objects, extract the actual value
    const value = raw !== null && typeof raw === 'object' && 'value' in (raw as Record<string, unknown>)
      ? (raw as { value: unknown }).value
      : raw
    if (value === null || value === undefined) continue
    const parts = key.split('.')
    let current = result
    for (let i = 0; i < parts.length - 1; i++) {
      if (!(parts[i] in current) || typeof current[parts[i]] !== 'object' || current[parts[i]] === null) {
        current[parts[i]] = {}
      }
      current = current[parts[i]] as Record<string, unknown>
    }
    current[parts[parts.length - 1]] = value
  }
  return result
}

// Module-level batch updater for device metric updates
let metricBatchUpdater: BatchUpdater<{ deviceId: string; property: string; value: unknown }> | null = null

export const createDeviceSlice: StateCreator<
  DeviceSlice,
  [],
  [],
  DeviceSlice
> = (set, get) => ({
  // Initial state
  devices: [],
  deviceTelemetry: {},
  deviceTypes: [],
  selectedDevice: null,
  selectedDeviceId: null,
  deviceDetails: null,
  deviceTypeDetails: null,
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
    if (!fetchCache.shouldFetch('devices')) return
    fetchCache.markFetching('devices')
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

      // Migrate current_values from devices into deviceTelemetry map
      // Use buildNestedValues to normalize flat dot-notation keys (e.g. "values.imageUrl")
      // and unwrap { value, timestamp } envelopes into plain values.
      // This ensures the same format as fetchDevicesCurrentBatch / _applyCurrentValuesBatch.
      const telemetryInit: Record<string, Record<string, unknown>> = {}
      for (const device of sortedDevices) {
        if (device.current_values && typeof device.current_values === 'object' && Object.keys(device.current_values).length > 0) {
          const nested = buildNestedValues(device.current_values as Record<string, unknown>)
          if (nested && Object.keys(nested).length > 0) {
            telemetryInit[device.id || device.device_id] = nested
          }
        }
      }

      set({ devices: sortedDevices, deviceTelemetry: telemetryInit })
      if (sortedDevices.length === 0) {
        // Don't cache empty results — backend may still be loading devices from DB
        fetchCache.invalidate('devices')
      } else {
        fetchCache.markFetched('devices')
      }
    } catch (error) {
      if ((error as Error).message === 'UNAUTHORIZED') {
        // Will be handled by auth slice
      }
      logError(error, { operation: 'Fetch devices' })
      set({ devices: [] })
      fetchCache.invalidate('devices')
    } finally {
      set({ devicesLoading: false })
    }
  },

  fetchDeviceTypes: async () => {
    if (!fetchCache.shouldFetch('deviceTypes')) return
    fetchCache.markFetching('deviceTypes')
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
      fetchCache.markFetched('deviceTypes')
    } catch (error) {
      if ((error as Error).message === 'UNAUTHORIZED') {
        // Will be handled by auth slice
      }
      logError(error, { operation: 'Fetch device types' })
      fetchCache.invalidate('deviceTypes')
    } finally {
      set({ deviceTypesLoading: false })
    }
  },

  addDevice: async (request: AddDeviceRequest) => {
    try {
      const result = await api.addDevice(request)
      // Backend returns { device_id, added: true } after unwrap
      if (result.added || result.device_id) {
        fetchCache.invalidate('devices')
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

  updateDevice: async (id: string, request: Partial<AddDeviceRequest>) => {
    try {
      const result = await api.updateDevice(id, request)
      // Backend returns { device_id, updated: true } after unwrap
      if (result.updated) {
        fetchCache.invalidate('devices')
        await get().fetchDevices()
        return true
      }
      return false
    } catch (error) {
      if ((error as Error).message === 'UNAUTHORIZED') {
        // Will be handled by auth slice
      }
      logError(error, { operation: 'Update device' })
      return false
    }
  },

  deleteDevice: async (id) => {
    const result = await api.deleteDevice(id)
    // Backend returns { device_id, deleted: true } after unwrap
    if (result.deleted) {
      fetchCache.invalidate('devices')
      await get().fetchDevices()
      // Clean up dashboard components referencing this device
      const store = (await import('@/store')).useStore
      store.getState().removeComponentsByDevice(id)
      return true
    }
    return false
  },

  sendCommand: async (deviceId, command, params) => {
    try {
      const result = await api.sendCommand(deviceId, command, params)
      // Backend returns { device_id, command, sent: true } after unwrap
      if (result.sent) {
        fetchCache.invalidate('devices')
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
    fetchCache.invalidate('deviceTypes')
    await get().fetchDeviceTypes()
    return true
  },

  deleteDeviceType: async (id) => {
    await api.deleteDeviceType(id)
    fetchCache.invalidate('deviceTypes')
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

  // Clear device detail data (before loading new device to avoid stale data flash)
  clearDeviceDetails: () => {
    set({
      deviceDetails: null,
      deviceTypeDetails: null,
      deviceCurrentState: null,
      telemetryData: null,
    })
  },

  // Device Details
  fetchDeviceDetails: async (id) => {
    try {
      const details = await api.getDevice(id)
      set({ deviceDetails: details })
      // Propagate fresh status fields back to the `devices` list cache so the
      // list page shows consistent state immediately after the user returns
      // from the detail page. Without this, fetchCache TTL (10s) skips the
      // list refetch and the user sees stale online/last_seen for up to 10s.
      // Events fired while the user was on the detail page (which doesn't
      // subscribe to useDeviceEvents) are also lost, so this is the only
      // reliable propagation path.
      if (details) {
        const frag = details as Partial<DeviceStatusFragment>
        const online = !!frag.online
        const status = frag.status
        const lastSeen = frag.last_seen
        const transportConnected = frag.transport_connected
        const transportChangedAt = frag.transport_changed_at
        set((state) => ({
          devices: state.devices.map((d) =>
            d.id === id || d.device_id === id
              ? {
                  ...d,
                  // Only overwrite status fields when the detail fetch provides them
                  // (older backends may omit transport_*)
                  ...(online !== undefined ? { online } : {}),
                  ...(status !== undefined ? { status } : {}),
                  ...(lastSeen !== undefined ? { last_seen: lastSeen } : {}),
                  ...(transportConnected !== undefined ? { transport_connected: transportConnected } : {}),
                  ...(transportChangedAt !== undefined ? { transport_changed_at: transportChangedAt } : {}),
                }
              : d
          ),
        }))
      }
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

  // Telemetry
  fetchTelemetryData: async (deviceId, metric, start, end, limit, offset) => {
    set({ telemetryLoading: true })
    try {
      const data = await api.getDeviceTelemetry(deviceId, metric, start, end, limit, offset)
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

      // Also update device telemetry with current values
      const newValues = buildNestedValues(data.metrics || {})
      // Sync status fields (online/status/last_seen/transport_*) back to the
      // devices list cache — same rationale as fetchDeviceDetails above.
      const frag = data as Partial<DeviceStatusFragment>
      const online = frag.online
      const status = frag.status
      const lastSeen = frag.last_seen
      const transportConnected = frag.transport_connected
      const transportChangedAt = frag.transport_changed_at
      set((state) => ({
        deviceTelemetry: newValues && Object.keys(newValues).length > 0
          ? { ...state.deviceTelemetry, [deviceId]: newValues }
          : state.deviceTelemetry,
        devices: state.devices.map((d) =>
          d.id === deviceId || d.device_id === deviceId
            ? {
                ...d,
                current_values: newValues,
                ...(online !== undefined ? { online } : {}),
                ...(status !== undefined ? { status } : {}),
                ...(lastSeen !== undefined ? { last_seen: lastSeen } : {}),
                ...(transportConnected !== undefined ? { transport_connected: transportConnected } : {}),
                ...(transportChangedAt !== undefined ? { transport_changed_at: transportChangedAt } : {}),
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
  fetchDevicesCurrentBatch: async (deviceIds, signal) => {
    if (!deviceIds || deviceIds.length === 0) {
      return
    }

    try {
      const data = await api.getDevicesCurrentBatch(deviceIds, signal)

      // Check if request was aborted before updating state
      if (signal?.aborted) return

      // Update devices array with fetched current_values
      const deviceDataMap = data.devices || {}

      // Shallow equality check for current_values — avoids expensive JSON.stringify
      // while still catching the vast majority of no-change cases
      const shallowEqualValues = (a: Record<string, unknown>, b: Record<string, unknown>): boolean => {
        const keysA = Object.keys(a)
        const keysB = Object.keys(b)
        if (keysA.length !== keysB.length) return false
        for (const key of keysA) {
          if (a[key] !== b[key]) return false
        }
        return true
      }

      const _t0 = performance.now()
      set((state) => {
        let changed = false
        const telemetryPatch: Record<string, Record<string, unknown>> = {}

        // Track which device IDs from the batch already exist in store
        const existingIds = new Set<string>()

        for (const device of state.devices) {
          const id = device.id || device.device_id
          existingIds.add(id)
          const deviceData = deviceDataMap[id]
          if (!deviceData) continue

          const newValues = buildNestedValues(deviceData.current_values)

          // Check if telemetry actually changed (compare against deviceTelemetry first, then current_values)
          const existing = state.deviceTelemetry[id] || device.current_values || {}
          if (existing && shallowEqualValues(existing, newValues)) continue

          changed = true
          // Preserve virtual metrics (from transforms) when replacing telemetry
          const virtualData = (state.deviceTelemetry[id] as Record<string, unknown> | undefined)?.virtual
          telemetryPatch[id] = virtualData ? { ...newValues, virtual: virtualData } : newValues
        }

        // For devices not yet in store, add placeholder entries with current_values
        // so that readDataFromStore can find them synchronously on first mount
        const newDevices: Device[] = []
        for (const [id, deviceData] of Object.entries(deviceDataMap)) {
          if (existingIds.has(id)) continue
          if (!deviceData?.current_values) continue
          const newValues = buildNestedValues(deviceData.current_values)
          if (Object.keys(newValues).length === 0) continue
          changed = true
          telemetryPatch[id] = newValues
          newDevices.push({
            id,
            device_id: id,
            name: id,
            status: 'unknown',
            online: false,
            current_values: newValues,
          } as Device)
        }

        if (!changed) return state as any

        return {
          deviceTelemetry: { ...state.deviceTelemetry, ...telemetryPatch },
          devices: newDevices.length > 0 ? [...state.devices, ...newDevices] : state.devices,
        }
      })
      const _dt = performance.now() - _t0
      if (_dt > 100) console.warn(`[perf] fetchDevicesCurrentBatch set(): ${Math.round(_dt)}ms`)

      // Invalidate fetch cache so callers can retry if needed
      fetchCache.invalidate('devicesCurrentBatch')
    } catch (error) {
      const errorMessage = (error as Error).message
      // Silently ignore expected errors
      if (errorMessage.includes('405') || errorMessage.includes('Method Not Allowed')) return
      if (errorMessage.includes('aborted') || (error as Error).name === 'AbortError') return
      logError(error, { operation: 'Fetch devices current batch' })
      fetchCache.invalidate('devicesCurrentBatch')
    }
  },

  // Throttle map: deviceId → epoch-ms of last touchDeviceActivity write.
  // Prevents store churn on high-frequency telemetry (one device can emit
  // dozens of metrics per second). 5s granularity is well within the
  // "x seconds ago" display resolution.
  // Module-scoped so it survives re-renders without entering React state.
  touchDeviceActivity: (() => {
    const lastTouch = new Map<string, number>()
    const THROTTLE_MS = 5000

    const fn = (deviceId: string) => {
      const now = Date.now()
      const last = lastTouch.get(deviceId) ?? 0
      if (now - last < THROTTLE_MS) return
      lastTouch.set(deviceId, now)

      const isoNow = new Date(now).toISOString()
      set((state) => ({
        devices: state.devices.map((device) =>
          device.id === deviceId || device.device_id === deviceId
            ? {
                ...device,
                last_seen: isoNow,
                // Mark as online — we just received data from it
                online: true,
                status: device.status === 'offline' || device.status === 'disconnected'
                  ? 'online'
                  : device.status,
              }
            : device
        ),
      }))
      set((state) => ({
        selectedDevice:
          state.selectedDevice?.id === deviceId ||
          state.selectedDevice?.device_id === deviceId
            ? {
                ...state.selectedDevice,
                last_seen: isoNow,
                online: true,
                status: state.selectedDevice.status === 'offline' || state.selectedDevice.status === 'disconnected'
                  ? 'online'
                  : state.selectedDevice.status,
              }
            : state.selectedDevice,
      }))
    }

    // Expose the throttle map for testing / reset
    ;(fn as any)._reset = () => lastTouch.clear()
    return fn
  })(),

  // Update device status from real-time events
  updateDeviceStatus: (deviceId: string, status: 'online' | 'offline') => {
    const now = new Date().toISOString()
    set((state) => ({
      devices: state.devices.map((device) =>
        device.id === deviceId || device.device_id === deviceId
          ? { ...device, status, online: status === 'online', last_seen: now }
          : device
      ),
    }))
    // Also update selectedDevice if it matches
    set((state) => ({
      selectedDevice:
        state.selectedDevice?.id === deviceId ||
        state.selectedDevice?.device_id === deviceId
          ? { ...state.selectedDevice, status, online: status === 'online', last_seen: now }
          : state.selectedDevice,
    }))
  },

  // Update transport connection state from DeviceTransportOnline/Offline events
  updateDeviceTransportStatus: (deviceId: string, connected: boolean) => {
    const now = Math.floor(Date.now() / 1000)
    set((state) => ({
      devices: state.devices.map((device) =>
        device.id === deviceId || device.device_id === deviceId
          ? { ...device, transport_connected: connected, transport_changed_at: now }
          : device
      ),
    }))
    set((state) => ({
      selectedDevice:
        state.selectedDevice?.id === deviceId ||
        state.selectedDevice?.device_id === deviceId
          ? { ...state.selectedDevice, transport_connected: connected, transport_changed_at: now }
          : state.selectedDevice,
    }))
  },

  // Update device metric from real-time events
  // Supports nested property paths like "values.battery" or "temperature"
  // If device doesn't exist in store, silently skip (will be added by fetchDevices)
  // Batches multiple metric updates within a single RAF tick for performance
  updateDeviceMetric: (deviceId: string, property: string, value: unknown) => {
    if (!metricBatchUpdater) {
      metricBatchUpdater = new BatchUpdater((updates) => {
        set((state: any) => {
          // Group updates by device
          const deviceUpdates = new Map<string, Map<string, unknown>>()
          for (const [, update] of updates) {
            if (!deviceUpdates.has(update.deviceId)) {
              deviceUpdates.set(update.deviceId, new Map())
            }
            deviceUpdates.get(update.deviceId)!.set(update.property, update.value)
          }

          // Write telemetry to deviceTelemetry map (does NOT touch devices array)
          const telemetryPatch: Record<string, Record<string, unknown>> = {}
          const statusChangeIds = new Set<string>()

          const now = new Date().toISOString()
          for (const [devId, props] of deviceUpdates) {
            // Skip devices that no longer exist (deleted between push and flush)
            const device = findDevice(state.devices, devId)
            if (!device && !state.deviceTelemetry[devId]) continue

            const existing = state.deviceTelemetry[devId] || {}
            let merged = { ...existing }
            let telemetryChanged = false
            for (const [prop, val] of props) {
              const oldVal = prop.includes('.') ? getNestedValue(merged, prop) : merged[prop]
              if (oldVal === val) continue
              merged = setNestedProperty(merged, prop, val)
              telemetryChanged = true
            }
            if (telemetryChanged) {
              telemetryPatch[devId] = merged
              // Detect status transition (offline→online) — requires devices array update
              if (device && device.status !== 'online') {
                statusChangeIds.add(devId)
              }
            }
          }

          if (Object.keys(telemetryPatch).length === 0) return state as any

          // Only update devices array for status changes (offline→online)
          let updatedDevices = state.devices
          if (statusChangeIds.size > 0) {
            updatedDevices = state.devices.map((device: any) => {
              const id = device.id || device.device_id
              if (!statusChangeIds.has(id)) return device
              return { ...device, last_seen: now, status: 'online', online: true }
            })
          }

          // Update selectedDevice if affected
          let updatedSelectedDevice = state.selectedDevice
          const selKey = state.selectedDevice?.id || state.selectedDevice?.device_id
          if (selKey) {
            const selTelemetry = telemetryPatch[selKey]
            if (selTelemetry && state.selectedDevice) {
              updatedSelectedDevice = {
                ...state.selectedDevice,
                current_values: selTelemetry,
                last_seen: now,
                ...(state.selectedDevice.status !== 'online' ? { status: 'online', online: true } : {}),
              }
            }
          }

          return {
            deviceTelemetry: { ...state.deviceTelemetry, ...telemetryPatch },
            devices: updatedDevices,
            selectedDevice: updatedSelectedDevice,
          }
        })
      })
    }
    metricBatchUpdater.push(`${deviceId}:${property}`, { deviceId, property, value })
  },

  // Apply current_values batch from fetchDeviceTelemetry directly via set(),
  // bypassing BatchUpdater RAF so store subscribers are notified immediately.
  _applyCurrentValuesBatch: (results, deviceIds) => {
    set((state) => {
      let changed = false
      const telemetryPatch: Record<string, Record<string, unknown>> = {}

      // Build telemetry patch for devices that exist in store
      const existingIds = new Set<string>()
      for (const device of state.devices) {
        const id = device.id || device.device_id
        existingIds.add(id)
        const entry = results[id] as { current_values?: Record<string, unknown> } | undefined
        if (!entry?.current_values) continue
        const newValues = buildNestedValues(entry.current_values)
        if (!newValues || Object.keys(newValues).length === 0) continue
        changed = true
        // Preserve virtual metrics (from transforms) when replacing telemetry
        const existingTelemetry = state.deviceTelemetry[id] as Record<string, unknown> | undefined
        const virtualData = existingTelemetry?.virtual
        telemetryPatch[id] = virtualData ? { ...newValues, virtual: virtualData } : newValues
      }

      // Add placeholder entries for devices not yet in store
      const newDevices: Device[] = []
      for (const [id, entry] of Object.entries(results)) {
        if (existingIds.has(id)) continue
        const cv = (entry as { current_values?: Record<string, unknown> })?.current_values
        if (!cv) continue
        const newValues = buildNestedValues(cv)
        if (Object.keys(newValues).length === 0) continue
        changed = true
        telemetryPatch[id] = newValues
        newDevices.push({ id, device_id: id, name: id, status: 'unknown', online: false, current_values: newValues } as Device)
      }

      if (!changed) return state as any

      // Update selectedDevice if its telemetry is in the patch
      let updatedSelectedDevice = state.selectedDevice
      const selKey = state.selectedDevice?.id || state.selectedDevice?.device_id
      if (selKey && telemetryPatch[selKey]) {
        updatedSelectedDevice = { ...state.selectedDevice!, current_values: telemetryPatch[selKey] }
      }

      return {
        deviceTelemetry: { ...state.deviceTelemetry, ...telemetryPatch },
        devices: newDevices.length > 0 ? [...state.devices, ...newDevices] : state.devices,
        selectedDevice: updatedSelectedDevice,
      }
    })
  },
})
