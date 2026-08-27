/**
 * useDashboardRealtime
 *
 * Handles real-time dashboard synchronization:
 * - SSE listener for DashboardUpdated events
 * - Device fetching retry when devices are empty
 * - Batch fetch current values for devices used in dashboard components
 * - Polling interval cleanup on dashboard switch
 * - Recovery when dashboards array becomes empty
 * - devicesRef synchronization
 *
 * This hook contains ONLY side effects. It returns nothing.
 */

import { useEffect, useMemo, useRef, type MutableRefObject } from 'react'
import { useEvents } from '@/hooks/useEvents'
import { isSelfSyncEcho } from '@/store/slices/dashboardCrudSlice'
import { useStore } from '@/store'
import { getSourceId } from '@/types/dashboard'
import type { Dashboard, DataSource, GenericComponent } from '@/types/dashboard'
import type { MapBinding } from '@/components/dashboard/generic/MapEditorDialog'
import type { Device } from '@/types'

export interface UseDashboardRealtimeParams {
  currentDashboard: Dashboard | null | undefined
  currentDashboardId: string | null | undefined
  devicesLength: number
  dashboardsLoading: boolean
  dashboardsCount: number
  devicesRef: MutableRefObject<Device[]>
  fetchDashboards: () => Promise<void>
  fetchDevices: () => Promise<void>
  fetchDevicesCurrentBatch: (deviceIds: string[], signal?: AbortSignal) => Promise<void>
}

export function useDashboardRealtime(params: UseDashboardRealtimeParams): void {
  const {
    currentDashboard,
    currentDashboardId,
    devicesLength,
    dashboardsLoading,
    dashboardsCount,
    devicesRef,
    fetchDashboards,
    fetchDevices,
    fetchDevicesCurrentBatch,
  } = params

  // Real-time dashboard sync: refetch when another client or AI modifies dashboards.
  // Skip events that are echoes of our own saves (drag, config edit, etc.)
  // to avoid overwriting in-progress edits with stale server data.
  useEvents({
    eventTypes: ['DashboardUpdated'],
    onEvent: (event) => {
      const data = event.data as { dashboard_id?: string; action?: string } | undefined
      const dashboardId = data?.dashboard_id
      if (dashboardId && isSelfSyncEcho(dashboardId)) return
      fetchDashboards()
    },
  })

  // Retry device fetching when devices are empty (backend DB may still be loading)
  // Max 10 retries (30s) to avoid polling forever when no devices exist
  useEffect(() => {
    // Only retry if we have dashboard components that need device data
    if (!currentDashboard || currentDashboard.components.length === 0) return
    if (devicesLength > 0) return
    if (dashboardsLoading) return

    let attempts = 0
    const MAX_ATTEMPTS = 10
    const interval = setInterval(() => {
      if (attempts >= MAX_ATTEMPTS) {
        clearInterval(interval)
        return
      }
      attempts++
      fetchDevices()
    }, 5000)

    return () => clearInterval(interval)
  }, [devicesLength, currentDashboard, dashboardsLoading, fetchDevices])

  // Batch fetch current values for devices used in dashboard components
  // Only considers the CURRENT dashboard (not all dashboards) to avoid
  // re-fetching when switching between dashboards with different devices.
  const dashboardDeviceIdsKey = useMemo(() => {
    if (!currentDashboard) return ''
    const deviceIds = new Set<string>()
    for (const component of currentDashboard.components) {
      const genericComponent = component as GenericComponent
      const dataSource = genericComponent.dataSource
      if (dataSource) {
        const sources = Array.isArray(dataSource) ? dataSource : [dataSource]
        for (const ds of sources) {
          const sid = getSourceId(ds)
          if (sid) deviceIds.add(sid)
        }
      }
      // Device-bound community/extension components (e.g. NE101 camera)
      const deviceBindingId = (genericComponent.config as any)?.deviceBinding?.deviceId as string | undefined
      if (deviceBindingId) deviceIds.add(deviceBindingId)
      if (genericComponent.type === 'map-display') {
        const bindings = (genericComponent.config as any)?.bindings as MapBinding[] || []
        for (const binding of bindings) {
          const ds = binding.dataSource as DataSource | undefined
          if (ds && getSourceId(ds)) {
            deviceIds.add(getSourceId(ds)!)
          }
        }
      }
    }
    return Array.from(deviceIds).sort().join(',')
  }, [currentDashboard])

  // Fetch batch current values when device set changes.
  // v0.7.0 approach: single initial fetch + slow background refresh (120s).
  // NO 2-second fast retry polling — it blocks the main thread during scroll
  // in WKWebView (Tauri), causing white screen frames.
  const batchFetchControllerRef = useRef<{ deviceIds: string[]; interval: ReturnType<typeof setInterval> | null }>({ deviceIds: [], interval: null })

  useEffect(() => {
    if (!dashboardDeviceIdsKey) return

    const deviceIds = dashboardDeviceIdsKey.split(',').filter(Boolean)
    if (deviceIds.length === 0) return

    // Clear previous polling if device set changed
    const ctrl = batchFetchControllerRef.current
    if (ctrl.interval) {
      clearInterval(ctrl.interval)
      ctrl.interval = null
    }
    ctrl.deviceIds = deviceIds

    // Initial fetch — do NOT pass an AbortSignal.
    // React 18 StrictMode unmounts→remounts components synchronously,
    // which aborts the signal before the fetch completes. Without a signal,
    // the initial fetch always succeeds regardless of effect re-runs.
    fetchDevicesCurrentBatch(deviceIds)

    // Periodic refresh uses a separate abortable controller
    const refreshController = new AbortController()
    const SLOW_REFRESH_MS = 120_000
    ctrl.interval = setInterval(() => {
      if (!refreshController.signal.aborted) {
        fetchDevicesCurrentBatch(deviceIds, refreshController.signal)
      }
    }, SLOW_REFRESH_MS)

    return () => {
      refreshController.abort()
      if (ctrl.interval) {
        clearInterval(ctrl.interval)
        ctrl.interval = null
      }
    }
  }, [dashboardDeviceIdsKey, fetchDevicesCurrentBatch])

  // Fix 3: Update devicesRef only when devices actually change (not on every render)
  useEffect(() => {
    devicesRef.current = useStore.getState().devices
  }, [devicesLength])

  // NOTE: there is deliberately NO "clear polling on dashboard switch" effect
  // here. The batch-refresh effect above owns its full lifecycle: its cleanup
  // runs whenever the device set changes, and its setup creates a fresh
  // interval. A separate switch-keyed effect ran AFTER that setup in the same
  // commit and unconditionally cleared the just-created interval, permanently
  // stopping background refresh after any dashboard switch.

  // Re-load dashboards if array becomes empty but we have a current ID
  useEffect(() => {
    if (dashboardsCount === 0 && currentDashboardId) {
      // Try to recover by fetching again
      fetchDashboards()
    }
  }, [dashboardsCount, currentDashboardId, fetchDashboards])
}
