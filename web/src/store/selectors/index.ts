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

import type { NeoTalkStore } from '..'

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
// Decision Selectors (Legacy - for backward compatibility)
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
// Stats Selectors (Legacy - for backward compatibility)
// ============================================================================

/**
 * Get connection status summary
 */
export const selectConnectionStatus = (state: NeoTalkStore) => ({
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
