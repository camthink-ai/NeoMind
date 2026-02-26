/**
 * Hook for subscribing to extension lifecycle events
 *
 * Handles automatic updates to DynamicRegistry and Dashboard when
 * extensions are registered or unregistered.
 */

import { useCallback, useRef, useState } from 'react'
import { useEvents } from './useEvents'
import { dynamicRegistry } from '@/components/dashboard/registry/DynamicRegistry'
import type { ExtensionLifecycleEvent } from '@/lib/events'
import { useStore } from '@/store'
import type { DashboardComponent, Dashboard } from '@/types/dashboard'

export interface UseExtensionLifecycleOptions {
  /** Auto-sync extension components on register (default: true) */
  autoSyncOnRegister?: boolean
  /** Auto-remove dashboard components on unregister (default: true) */
  autoRemoveOnUnregister?: boolean
}

export interface ExtensionLifecycleResult {
  /** Sync extension components from API */
  syncComponents: () => Promise<void>
  /** Refresh version - increment when components change, use to trigger re-renders */
  refreshVersion: number
}

/**
 * Hook for handling extension lifecycle events
 *
 * @param options - Configuration options
 * @returns Result object with syncComponents method and refreshVersion
 */
export function useExtensionLifecycle(
  options: UseExtensionLifecycleOptions = {}
): ExtensionLifecycleResult {
  const {
    autoSyncOnRegister = true,
    autoRemoveOnUnregister = true,
  } = options

  const syncingRef = useRef(false)
  const [refreshVersion, setRefreshVersion] = useState(0)

  /**
   * Handle extension registered event
   */
  const handleRegistered = useCallback(async (extensionId: string) => {
    if (!autoSyncOnRegister || syncingRef.current) return

    syncingRef.current = true
    try {
      // Fetch new components from API
      const response = await fetch(`/api/extensions/${extensionId}/components`)
      if (response.ok) {
        const result = await response.json()
        const components = result.data?.components || result.components || []

        // Register in dynamic registry
        for (const comp of components) {
          dynamicRegistry.register(
            comp.extension_id || extensionId,
            result.extension_name || extensionId,
            comp
          )
        }

        console.log(`[ExtensionLifecycle] Registered ${components.length} components from ${extensionId}`)

        // Trigger re-render
        setRefreshVersion(v => v + 1)
      }
    } catch (e) {
      console.error(`[ExtensionLifecycle] Failed to sync components for ${extensionId}:`, e)
    } finally {
        syncingRef.current = false
    }
  }, [autoSyncOnRegister])

  /**
   * Handle extension unregistered event
   */
  const handleUnregistered = useCallback((extensionId: string) => {
    if (!autoRemoveOnUnregister) return

    // 1. Get component types from DynamicRegistry BEFORE unregistering
    //    We need these types to remove components from Dashboard
    const extInfo = dynamicRegistry.getExtensions().find(ext => ext.extensionId === extensionId)
    const componentTypes = extInfo?.componentTypes || []

    // 2. Remove components from Dashboard by matching component types
    if (componentTypes.length > 0) {
      const { currentDashboard, dashboards, persistDashboard } = useStore.getState()
      if (currentDashboard) {
        const typeSet = new Set(componentTypes)
        const componentsToRemove = currentDashboard.components.filter(
          (comp: DashboardComponent) => typeSet.has(comp.type)
        )

        if (componentsToRemove.length > 0) {
          const idsToRemove = new Set(componentsToRemove.map((c: DashboardComponent) => c.id))
          const updatedDashboard: Dashboard = {
            ...currentDashboard,
            components: currentDashboard.components.filter((c: DashboardComponent) => !idsToRemove.has(c.id)),
            updatedAt: Date.now(),
          }

          const updatedDashboards = dashboards.map((d: Dashboard) =>
            d.id === currentDashboard.id ? updatedDashboard : d
          )

          useStore.setState({
            dashboards: updatedDashboards,
            currentDashboard: updatedDashboard,
          })

          // Persist changes to storage
          persistDashboard(updatedDashboard.id).catch((err: unknown) => {
            console.warn('[ExtensionLifecycle] Failed to persist dashboard after removing components:', err)
          })

          console.log(`[ExtensionLifecycle] Removed ${componentsToRemove.length} components from dashboard`)
        }
      }
    }

    // 3. Unregister from DynamicRegistry (removes component templates)
    dynamicRegistry.unregisterExtension(extensionId)

    console.log(`[ExtensionLifecycle] Extension ${extensionId} unregistered, components removed from registry and dashboard`)

    // Trigger re-render
    setRefreshVersion(v => v + 1)
  }, [autoRemoveOnUnregister])

  // Subscribe to extension lifecycle events
  useEvents({
    category: 'extension',
    onEvent: (event) => {
      if (event.type === 'ExtensionLifecycle') {
        const lifecycleEvent = event as ExtensionLifecycleEvent
        const { extension_id, state } = lifecycleEvent.data

        switch (state) {
          case 'registered':
          case 'loaded':
            handleRegistered(extension_id)
            break
          case 'unregistered':
            handleUnregistered(extension_id)
            break
        }
      }
    },
  })

  /**
   * Manually sync all extension components
   */
  const syncComponents = useCallback(async () => {
    if (syncingRef.current) return
    syncingRef.current = true

    try {
      const response = await fetch('/api/extensions/dashboard-components')
      if (response.ok) {
        const result = await response.json()
        const components = result.data || result || []

        dynamicRegistry.clearAllModuleCache()

        for (const comp of components) {
          dynamicRegistry.register(comp.extension_id, comp.extension_id, comp)
        }

        console.log(`[ExtensionLifecycle] Synced ${components.length} components`)

        // Trigger re-render
        setRefreshVersion(v => v + 1)
      }
    } catch (e) {
      console.error('[ExtensionLifecycle] Failed to sync components:', e)
    } finally {
      syncingRef.current = false
    }
  }, [])

  return {
    syncComponents,
    refreshVersion,
  }
}
