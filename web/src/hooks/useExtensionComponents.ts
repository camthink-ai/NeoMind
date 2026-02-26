/**
 * Hook for synchronizing extension dashboard components
 *
 * Loads component definitions from extensions and registers them
 * in the dynamic registry for use in dashboards.
 */

import { useEffect, useState, useCallback, useRef } from 'react'
import type { DashboardComponentsResponse, DashboardComponentDto } from '@/types'
import { dynamicRegistry } from '@/components/dashboard/registry/DynamicRegistry'

/**
 * Manually sync extension components from the API
 * This can be called from anywhere to refresh the component registry
 */
export async function syncExtensionComponents(): Promise<number> {
  try {
    const response = await fetch('/api/extensions/dashboard-components')
    if (!response.ok) {
      throw new Error(`Failed to fetch extension components: ${response.statusText}`)
    }

    const result = await response.json()
    const components: DashboardComponentDto[] = result.data || result || []

    // Clear all registered components and module caches before re-registering
    dynamicRegistry.clear()

    // Register components in dynamic registry
    for (const comp of components) {
      dynamicRegistry.register(comp.extension_id, comp.extension_id, comp)
    }

    console.log(`[syncExtensionComponents] Loaded ${components.length} components`)
    return components.length
  } catch (e) {
    console.error('Failed to sync extension components:', e)
    return 0
  }
}

/**
 * Result of extension components sync
 */
export interface ExtensionComponentsSyncResult {
  /** Loading state */
  loading: boolean
  /** Error if any */
  error: string | null
  /** Number of components loaded */
  componentCount: number
  /** Number of extensions providing components */
  extensionCount: number
  /** All loaded components */
  components: DashboardComponentDto[]
  /** Whether sync has been performed at least once */
  initialized: boolean
  /** Manually trigger a sync */
  sync: () => Promise<void>
}

/**
 * State for extension components
 */
interface ExtensionComponentsState {
  components: Record<string, DashboardComponentDto>
  extensions: Record<string, { extensionId: string; extensionName: string }>
}

/**
 * Hook to sync extension components from the API
 *
 * This hook fetches all dashboard components from extensions
 * and registers them in the dynamic registry.
 *
 * @param options - Sync options
 * @returns Sync result
 */
export function useExtensionComponents(options?: {
  /** Auto-sync on mount (default: true) */
  autoSync?: boolean
  /** Sync interval in ms (default: 30000 = 30 seconds) */
  syncInterval?: number
}): ExtensionComponentsSyncResult {
  const { autoSync = true, syncInterval = 30000 } = options || {}

  const [state, setState] = useState<ExtensionComponentsState>({
    components: {},
    extensions: {},
  })
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [initialized, setInitialized] = useState(false)
  const syncingRef = useRef(false)

  /**
   * Sync components from API
   */
  const sync = useCallback(async () => {
    // Prevent concurrent syncs
    if (syncingRef.current) return
    syncingRef.current = true

    setLoading(true)
    setError(null)

    try {
      // Get all dashboard components from all extensions
      // This endpoint reads from manifest.json files, so components are available
      // even if the extension itself is not running
      const response = await fetch('/api/extensions/dashboard-components')
      if (!response.ok) {
        throw new Error(`Failed to fetch extension components: ${response.statusText}`)
      }

      const result = await response.json()
      // Handle wrapped API response format { success: true, data: [...] }
      const components: DashboardComponentDto[] = result.data || result || []

      // Clear all registered components and module caches before re-registering
      // This ensures unregistered extensions' components are removed
      dynamicRegistry.clear()

      // Register components in dynamic registry
      const newComponents: Record<string, DashboardComponentDto> = {}
      const newExtensions: Record<string, { extensionId: string; extensionName: string }> = {}

      for (const comp of components) {
        newComponents[comp.type] = comp
        newExtensions[comp.extension_id] = {
          extensionId: comp.extension_id,
          extensionName: comp.extension_id,
        }

        // Register in dynamic registry
        dynamicRegistry.register(comp.extension_id, comp.extension_id, comp)
      }

      setState({ components: newComponents, extensions: newExtensions })
      setInitialized(true)

      console.log(`[useExtensionComponents] Loaded ${components.length} components from ${Object.keys(newExtensions).length} extensions`)
    } catch (e) {
      const errorMessage = e instanceof Error ? e.message : String(e)
      setError(errorMessage)
      console.error('Failed to sync extension components:', e)
    } finally {
      setLoading(false)
      syncingRef.current = false
    }
  }, [])

  // Auto-sync on mount
  useEffect(() => {
    if (autoSync) {
      sync()
    }
  }, [autoSync, sync])

  // Interval sync
  useEffect(() => {
    if (!syncInterval || syncInterval <= 0) return

    const interval = setInterval(() => {
      sync()
    }, syncInterval)

    return () => clearInterval(interval)
  }, [syncInterval, sync])

  return {
    loading,
    error,
    componentCount: Object.keys(state.components).length,
    extensionCount: Object.keys(state.extensions).length,
    components: Object.values(state.components),
    initialized,
    sync,
  }
}

/**
 * Hook to get components from a specific extension
 *
 * @param extensionId - The extension ID
 * @returns Components from the extension
 */
export function useExtensionComponentsByExtension(extensionId: string): {
  components: DashboardComponentDto[]
  loading: boolean
  error: string | null
  sync: () => Promise<void>
} {
  const [components, setComponents] = useState<DashboardComponentDto[]>([])
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState<string | null>(null)

  const sync = async () => {
    setLoading(true)
    setError(null)

    try {
      const response = await fetch(`/api/extensions/${extensionId}/components`)
      if (!response.ok) {
        if (response.status === 404) {
          // Extension not found or no components - return empty
          setComponents([])
          return
        }
        throw new Error(`Failed to fetch components: ${response.statusText}`)
      }

      const result = await response.json()
      // Handle wrapped API response format
      const data: DashboardComponentsResponse = result.data || result

      // Register components
      for (const comp of data.components) {
        dynamicRegistry.register(extensionId, data.extension_name, comp)
      }

      setComponents(data.components)
    } catch (e) {
      const errorMessage = e instanceof Error ? e.message : String(e)
      setError(errorMessage)
      console.error(`Failed to sync components for extension ${extensionId}:`, e)
    } finally {
      setLoading(false)
    }
  }

  useEffect(() => {
    sync()
  }, [extensionId])

  return {
    components,
    loading,
    error,
    sync,
  }
}
