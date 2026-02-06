/**
 * Store Selectors
 *
 * Memoized selectors for deriving state from the store.
 * These selectors use Zustand's optimized selector pattern to prevent unnecessary re-renders.
 *
 * Usage:
 *   import { useStore } from '@/store'
 *   import { selectOnlineDevices, selectActiveSession } from '@/store/selectors'
 *
 *   // For single value: direct selector
 *   const onlineCount = useStore(selectOnlineDeviceCount)
 *
 *   // For arrays/objects: use shallow comparison
 *   import { shallow } from 'zustand/shallow'
 *   const devices = useStore(selectDevicesRaw, shallow)
 */

import type { NeoMindStore } from '..'

// ============================================================================
// Device Selectors
// ============================================================================

export {
  selectDevicesRaw,
  selectOnlineDevices,
  selectOfflineDevices,
  selectDevicesByType,
  selectDeviceById,
  selectDeviceMap,
  selectOnlineDeviceCount,
  selectOfflineDeviceCount,
  selectTotalDeviceCount,
  selectDevicesByStatus,
  selectDevicesGroupedByType,
  selectDevicesShallow,
  selectDevicesSummary,
} from './deviceSelectors'

// ============================================================================
// Session Selectors
// ============================================================================

export {
  selectSessionsRaw,
  selectActiveSession,
  selectActiveSessionId,
  selectSessionById,
  selectSessionMap,
  selectSessionsByRecent,
  selectSessionsByNewest,
  selectSessionsByName,
  selectSessionCount,
  selectSessionsGroupedByDate,
  selectSessionsSummary,
  selectSessionsBySearchTerm,
  selectSessionsWithMessages,
} from './sessionSelectors'

// ============================================================================
// Alert Selectors
// ============================================================================

export {
  selectAlertsRaw,
  selectUnacknowledgedAlerts,
  selectAcknowledgedAlerts,
  selectActiveAlerts,
  selectResolvedAlerts,
  selectCriticalAlerts,
  selectWarningAlerts,
  selectInfoAlerts,
  selectAlertsBySeverity,
  selectUnacknowledgedCriticalAlerts,
  selectUnacknowledgedBySeverity,
  selectActiveBySeverity,
  selectUnacknowledgedAlertCount,
  selectCriticalAlertCount,
  selectActiveAlertCount,
  selectTotalAlertCount,
  selectAlertSummaryBySeverity,
  selectAlertFullSummary,
  selectAlertsBySeverityPriority,
  selectAlertsByNewest,
  selectAlertsByOldest,
  selectAlertMap,
  selectAlertById,
} from './alertSelectors'

// ============================================================================
// Stats Selectors (Legacy - for backward compatibility)
// ============================================================================

/**
 * Get connection status summary
 */
export const selectConnectionStatus = (state: NeoMindStore) => ({
  ai: state.wsConnected ? 'online' : 'offline',
  authenticated: state.isAuthenticated,
})

// ============================================================================
// Chat Selectors
// ============================================================================

export {
  selectSessionId,
  selectMessages,
  selectUser,
  selectLlmBackends,
  selectActiveBackendId,
  selectActiveBackend,
  selectLlmBackendState,
  selectChatActions,
  selectChatState,
} from './chatSelectors'
