/**
 * Hook for subscribing to extension lifecycle events
 *
 * Handles automatic updates to DynamicRegistry when extensions are
 * registered or unregistered.
 *
 * Widget *definitions* on dashboards are deliberately decoupled from the
 * extension runtime: when an extension unregisters (reload, crash
 * re-register, uninstall), its component templates leave the registry so
 * existing widgets render the "component unavailable" placeholder, but
 * the widgets themselves are NEVER removed from the dashboard. When the
 * extension comes back ("registered"), the templates return and the
 * widgets resume rendering in place. The backend already persists
 * unknown widget types for the same reason (see update_dashboard_handler).
 */

import { useCallback, useRef, useState } from 'react'
import { useEvents } from './useEvents'
import { dynamicRegistry } from '@/components/dashboard/registry/DynamicRegistry'
import type { ExtensionLifecycleEvent } from '@/lib/events'
import { getApiBase } from '@/lib/api'

export interface UseExtensionLifecycleOptions {
  /** Auto-sync extension components on register (default: true) */
  autoSyncOnRegister?: boolean
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
      const response = await fetch(`${getApiBase()}/extensions/${extensionId}/components`)
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
   * Handle extension unregistered event (reload, crash re-register,
   * uninstall). Only the component TEMPLATES are removed — widgets that
   * reference them stay on the dashboard and render the unavailable
   * placeholder until the extension returns.
   */
  const handleUnregistered = useCallback((extensionId: string) => {
    dynamicRegistry.unregisterExtension(extensionId)

    // Trigger re-render
    setRefreshVersion(v => v + 1)
  }, [])

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
      const response = await fetch(`${getApiBase()}/extensions/dashboard-components`)
      if (response.ok) {
        const result = await response.json()
        const components = result.data || result || []

        dynamicRegistry.clearAllModuleCache()

        for (const comp of components) {
          dynamicRegistry.register(comp.extension_id, comp.extension_id, comp)
        }

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
