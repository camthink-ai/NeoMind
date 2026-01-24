/**
 * Store Selectors
 *
 * Computed selectors for deriving state from the store.
 * Selectors are memoized functions that compute derived state.
 */

import type { NeoTalkStore } from '..'

// ============================================================================
// Device Selectors
// ============================================================================

/**
 * Get all online devices
 */
export const selectOnlineDevices = (state: NeoTalkStore) =>
  state.devices.filter((d) => d.status === 'online')

/**
 * Get all offline devices
 */
export const selectOfflineDevices = (state: NeoTalkStore) =>
  state.devices.filter((d) => d.status === 'offline')

/**
 * Get count of online devices
 */
export const selectOnlineDeviceCount = (state: NeoTalkStore) =>
  state.devices.filter((d) => d.status === 'online').length

/**
 * Get count of offline devices
 */
export const selectOfflineDeviceCount = (state: NeoTalkStore) =>
  state.devices.filter((d) => d.status === 'offline').length

/**
 * Get devices by type
 */
export const selectDevicesByType = (state: NeoTalkStore, deviceType: string) =>
  state.devices.filter((d) => d.device_type === deviceType)

/**
 * Get device by ID
 */
export const selectDeviceById = (state: NeoTalkStore, deviceId: string) =>
  state.devices.find((d) => d.id === deviceId)

// ============================================================================
// Alert Selectors
// ============================================================================

/**
 * Get unacknowledged alerts
 */
export const selectUnacknowledgedAlerts = (state: NeoTalkStore) =>
  state.alerts.filter((a) => !a.acknowledged)

/**
 * Get critical alerts
 */
export const selectCriticalAlerts = (state: NeoTalkStore) =>
  state.alerts.filter((a) => a.severity === 'critical')

/**
 * Get alerts by severity
 */
export const selectAlertsBySeverity = (state: NeoTalkStore, severity: string) =>
  state.alerts.filter((a) => a.severity === severity)

/**
 * Get count of unacknowledged alerts
 */
export const selectUnacknowledgedAlertCount = (state: NeoTalkStore) =>
  state.alerts.filter((a) => !a.acknowledged).length

// ============================================================================
// Decision Selectors
// ============================================================================

/**
 * Get pending decisions (not approved or rejected)
 */
export const selectPendingDecisions = (state: NeoTalkStore) =>
  state.decisions.filter((d) => d.status === 'pending')

/**
 * Get approved decisions
 */
export const selectApprovedDecisions = (state: NeoTalkStore) =>
  state.decisions.filter((d) => d.status === 'approved')

/**
 * Get rejected decisions
 */
export const selectRejectedDecisions = (state: NeoTalkStore) =>
  state.decisions.filter((d) => d.status === 'rejected')

/**
 * Get executed decisions
 */
export const selectExecutedDecisions = (state: NeoTalkStore) =>
  state.decisions.filter((d) => d.status === 'executed')

/**
 * Get count of pending decisions
 */
export const selectPendingDecisionCount = (state: NeoTalkStore) =>
  state.decisions.filter((d) => d.status === 'pending').length

// ============================================================================
// Stats Selectors
// ============================================================================

/**
 * Get total device count
 */
export const selectTotalDeviceCount = (state: NeoTalkStore) => state.devices.length

/**
 * Get total alert count
 */
export const selectTotalAlertCount = (state: NeoTalkStore) => state.alerts.length

/**
 * Get connection status summary
 */
export const selectConnectionStatus = (state: NeoTalkStore) => ({
  ai: state.wsConnected ? 'online' : 'offline',
  authenticated: state.isAuthenticated,
})
