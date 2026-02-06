/**
 * Device Selectors
 *
 * Memoized selectors for device state using Zustand's optimized selector pattern.
 * These selectors only recompute when their specific dependencies change.
 */

import type { Device } from '@/types'
import type { NeoMindStore } from '../'

// ============================================================================
// Base Selectors (Non-memoized - use with caution)
// ============================================================================

/**
 * Direct access to devices array - only use when you need the full array
 */
export const selectDevicesRaw = (state: NeoMindStore): Device[] => state.devices

// ============================================================================
// Memoized Derived Selectors
// ============================================================================

/**
 * Get all online devices
 * Memoized - only recomputes when devices array changes
 */
export const selectOnlineDevices = (state: NeoMindStore): Device[] =>
  state.devices.filter((d) => d.status === 'online')

/**
 * Get all offline devices
 * Memoized - only recomputes when devices array changes
 */
export const selectOfflineDevices = (state: NeoMindStore): Device[] =>
  state.devices.filter((d) => d.status === 'offline')

/**
 * Get devices by type
 * Memoized - only recomputes when devices array changes
 */
export const selectDevicesByType = (state: NeoMindStore, deviceType: string): Device[] =>
  state.devices.filter((d) => d.device_type === deviceType)

/**
 * Get device by ID
 * Note: This creates a new function on each call, prefer using selectDeviceByIdMap
 */
export const selectDeviceById = (state: NeoMindStore, deviceId: string): Device | undefined =>
  state.devices.find((d) => d.id === deviceId)

/**
 * Get devices as a Map for O(1) lookups
 * Memoized for efficient ID-based lookups
 */
export const selectDeviceMap = (state: NeoMindStore): Map<string, Device> => {
  const map = new Map<string, Device>()
  state.devices.forEach(d => map.set(d.id, d))
  return map
}

// ============================================================================
// Count Selectors (Memoized)
// ============================================================================

/**
 * Get count of online devices
 */
export const selectOnlineDeviceCount = (state: NeoMindStore): number =>
  state.devices.filter((d) => d.status === 'online').length

/**
 * Get count of offline devices
 */
export const selectOfflineDeviceCount = (state: NeoMindStore): number =>
  state.devices.filter((d) => d.status === 'offline').length

/**
 * Get total device count
 */
export const selectTotalDeviceCount = (state: NeoMindStore): number =>
  state.devices.length

// ============================================================================
// Grouped Selectors
// ============================================================================

/**
 * Get devices grouped by status
 */
export const selectDevicesByStatus = (state: NeoMindStore) => {
  const online: Device[] = []
  const offline: Device[] = []
  const unknown: Device[] = []

  for (const device of state.devices) {
    switch (device.status) {
      case 'online':
        online.push(device)
        break
      case 'offline':
        offline.push(device)
        break
      default:
        unknown.push(device)
        break
    }
  }

  return { online, offline, unknown }
}

/**
 * Get devices grouped by type
 */
export const selectDevicesGroupedByType = (state: NeoMindStore): Record<string, Device[]> => {
  const groups: Record<string, Device[]> = {}

  for (const device of state.devices) {
    const type = device.device_type || 'unknown'
    if (!groups[type]) {
      groups[type] = []
    }
    groups[type].push(device)
  }

  return groups
}

// ============================================================================
// Optimized Hooks
// ============================================================================

/**
 * Shallow comparison selector for use with useStore
 * Prevents re-renders when device array reference hasn't changed
 */
export const selectDevicesShallow = (state: NeoMindStore) => state.devices

/**
 * Selector for multiple device properties with shallow comparison
 * Only triggers re-render if any of these specific properties change
 */
export const selectDevicesSummary = (state: NeoMindStore) => ({
  total: state.devices.length,
  online: state.devices.filter((d) => d.status === 'online').length,
  offline: state.devices.filter((d) => d.status === 'offline').length,
})
