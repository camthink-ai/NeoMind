/**
 * Hook for subscribing to community component lifecycle events
 *
 * Handles automatic updates to CommunityRegistry and store when
 * components are installed or uninstalled.
 */

import { useCallback, useRef, useState } from 'react'
import { useEvents } from './useEvents'
import { communityRegistry } from '@/components/dashboard/registry/CommunityRegistry'
import type { FrontendComponentLifecycleEvent } from '@/lib/events'
import { useStore } from '@/store'
import type { FrontendComponentMeta } from '@/types/frontend-component'

export interface UseCommunityComponentLifecycleOptions {
  /** Auto-refresh installed components on install (default: true) */
  autoRefreshOnInstall?: boolean
  /** Auto-remove from registry on uninstall (default: true) */
  autoRemoveOnUninstall?: boolean
}

export interface CommunityComponentLifecycleResult {
  /** Refresh installed components - increment when components change, use to trigger re-renders */
  refreshVersion: number
}

/**
 * Hook for handling community component lifecycle events
 *
 * @param options - Configuration options
 * @returns Result object with refreshVersion
 */
export function useCommunityComponentLifecycle(
  options: UseCommunityComponentLifecycleOptions = {}
): CommunityComponentLifecycleResult {
  const {
    autoRefreshOnInstall = true,
    autoRemoveOnUninstall = true,
  } = options

  const [refreshVersion, setRefreshVersion] = useState(0)
  const fetchingRef = useRef(false)

  const fetchInstalled = useStore(s => s.fetchInstalled)
  const setInstalled = useStore(_s => (components: FrontendComponentMeta[]) => {
    // Direct state update to avoid full fetchInstalled call
    // This is a lightweight update for immediate UI feedback
    const currentState = useStore.getState()
    useStore.setState({
      installed: components,
      fetchCache: {
        ...currentState.fetchCache,
        installed: { timestamp: Date.now() },
      },
    })
  })

  /**
   * Handle component installed event
   */
  const handleInstalled = useCallback(async (componentId: string) => {
    if (!autoRefreshOnInstall || fetchingRef.current) return

    fetchingRef.current = true
    try {
      // Refresh installed components from API
      await fetchInstalled()

      // Trigger re-render
      setRefreshVersion(v => v + 1)
    } catch (e) {
      console.error(`[CommunityComponentLifecycle] Failed to refresh components after install of ${componentId}:`, e)
    } finally {
      fetchingRef.current = false
    }
  }, [autoRefreshOnInstall, fetchInstalled])

  /**
   * Handle component uninstalled event
   */
  const handleUninstalled = useCallback((componentId: string) => {
    if (!autoRemoveOnUninstall) return

    // 1. Unregister from CommunityRegistry (cleans up global variables and caches)
    communityRegistry.unregister(componentId)

    // 2. Remove from store state (immediate UI update)
    const currentState = useStore.getState()
    const updatedInstalled = currentState.installed.filter(c => c.id !== componentId)
    setInstalled(updatedInstalled)

    // Trigger re-render
    setRefreshVersion(v => v + 1)
  }, [autoRemoveOnUninstall, setInstalled])

  // Subscribe to frontend component lifecycle events
  useEvents({
    category: 'all',
    onEvent: (event) => {
      // Check if this is a Custom event with FrontendComponentLifecycle type
      if (event.type === 'Custom') {
        const customEvent = event as any
        if (customEvent.data?.event_type === 'FrontendComponentLifecycle') {
          const lifecycleEvent = customEvent as FrontendComponentLifecycleEvent
          const { component_id, state } = lifecycleEvent.data

          switch (state) {
            case 'installed':
              handleInstalled(component_id)
              break
            case 'uninstalled':
              handleUninstalled(component_id)
              break
          }
        }
      }
    },
  })

  return {
    refreshVersion,
  }
}
