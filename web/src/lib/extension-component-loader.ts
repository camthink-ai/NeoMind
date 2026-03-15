/**
 * Extension Component Loader
 *
 * Enhanced wrapper around DynamicRegistry with:
 * - Preloading capabilities
 * - React hooks for easier integration
 * - Better error handling
 * - Performance optimizations
 */

import * as React from 'react'
import { dynamicRegistry } from '@/components/dashboard/registry/DynamicRegistry'
import type { DashboardComponentDto } from '@/types'

/**
 * Extension manifest interface
 */
interface ExtensionManifest {
  id: string
  version: string
  frontend: {
    entrypoint: string
    components: ComponentDefinition[]
    styles?: string[]
  }
}

/**
 * Component definition from manifest
 */
interface ComponentDefinition {
  name: string
  type: 'card' | 'widget' | 'dialog'
  displayName: string
  description: string
  defaultSize?: { width: number; height: number }
}

/**
 * Loading state for a component
 */
interface ComponentLoadingState {
  status: 'idle' | 'loading' | 'loaded' | 'error'
  error?: Error
  component?: any
}

/**
 * Enhanced extension component loader with preloading and hooks
 */
class ExtensionComponentLoaderClass {
  private loadingStates = new Map<string, ComponentLoadingState>()
  private preloadingPromises = new Map<string, Promise<any>>()

  /**
   * Load an extension component
   * @param extensionId Extension ID
   * @param componentName Component name (type)
   * @returns Component constructor
   */
  async loadComponent(
    extensionId: string,
    componentName: string
  ): Promise<any> {
    const cacheKey = `${extensionId}:${componentName}`

    // Check loading state
    const state = this.loadingStates.get(cacheKey)
    if (state?.status === 'loaded' && state.component) {
      return state.component
    }

    // Check if currently loading
    if (state?.status === 'loading') {
      // Wait for existing load
      await new Promise(resolve => setTimeout(resolve, 100))
      return this.loadComponent(extensionId, componentName)
    }

    // Start loading
    this.loadingStates.set(cacheKey, { status: 'loading' })

    try {
      const component = await dynamicRegistry.loadComponent(componentName)

      if (!component) {
        throw new Error(`Component ${componentName} not found in extension ${extensionId}`)
      }

      this.loadingStates.set(cacheKey, {
        status: 'loaded',
        component
      })

      return component
    } catch (error) {
      const err = error instanceof Error ? error : new Error(String(error))
      this.loadingStates.set(cacheKey, {
        status: 'error',
        error: err
      })
      throw err
    }
  }

  /**
   * Unload a component from cache
   */
  unloadComponent(extensionId: string, componentName: string): void {
    const cacheKey = `${extensionId}:${componentName}`
    this.loadingStates.delete(cacheKey)
    dynamicRegistry.clearModuleCache(componentName)
  }

  /**
   * Preload all components for an extension
   * @param extensionId Extension ID
   * @returns Promise that resolves when all components are loaded
   */
  async preloadComponents(extensionId: string): Promise<void> {
    // Check if already preloading
    const existing = this.preloadingPromises.get(extensionId)
    if (existing) {
      return existing
    }

    // Start preloading
    const promise = this.doPreloadComponents(extensionId)
    this.preloadingPromises.set(extensionId, promise)

    try {
      await promise
    } finally {
      this.preloadingPromises.delete(extensionId)
    }
  }

  /**
   * Internal preload implementation
   */
  private async doPreloadComponents(extensionId: string): Promise<void> {
    const extensions = dynamicRegistry.getExtensions()
    const extInfo = extensions.find(e => e.extensionId === extensionId)

    if (!extInfo) {
      console.warn(`[ExtensionLoader] Extension ${extensionId} not found`)
      return
    }

    // Load all components in parallel
    await Promise.all(
      extInfo.componentTypes.map(componentType =>
        this.loadComponent(extensionId, componentType)
          .catch(err => {
            console.warn(`[ExtensionLoader] Failed to preload ${componentType}:`, err)
            // Don't fail entire preload if one component fails
          })
      )
    )

    console.log(`[ExtensionLoader] Preloaded ${extInfo.componentTypes.length} components for ${extensionId}`)
  }

  /**
   * Preload components for multiple extensions
   */
  async preloadExtensions(extensionIds: string[]): Promise<void> {
    await Promise.all(
      extensionIds.map(id => this.preloadComponents(id))
    )
  }

  /**
   * Get loading state for a component
   */
  getLoadingState(extensionId: string, componentName: string): ComponentLoadingState | undefined {
    const cacheKey = `${extensionId}:${componentName}`
    return this.loadingStates.get(cacheKey)
  }

  /**
   * Check if a component is loaded
   */
  isLoaded(extensionId: string, componentName: string): boolean {
    const state = this.getLoadingState(extensionId, componentName)
    return state?.status === 'loaded'
  }
}

// Singleton instance
export const extensionComponentLoader = new ExtensionComponentLoaderClass()

/**
 * React Hook: Load an extension component
 *
 * @param extensionId Extension ID
 * @param componentName Component name (type)
 * @returns Object with component, loading state, and error
 *
 * @example
 * ```tsx
 * const { component, loading, error } = useExtensionComponent('weather-v2', 'weather-card')
 *
 * if (loading) return <div>Loading...</div>
 * if (error) return <div>Error: {error.message}</div>
 * if (!component) return null
 *
 * return <Component title="Weather" />
 * ```
 */
export function useExtensionComponent(
  extensionId: string,
  componentName: string
) {
  const [component, setComponent] = React.useState<any | null>(null)
  const [loading, setLoading] = React.useState(true)
  const [error, setError] = React.useState<Error | null>(null)

  React.useEffect(() => {
    let cancelled = false

    extensionComponentLoader
      .loadComponent(extensionId, componentName)
      .then(comp => {
        if (!cancelled) {
          setComponent(comp)
          setLoading(false)
        }
      })
      .catch(err => {
        if (!cancelled) {
          setError(err instanceof Error ? err : new Error(String(err)))
          setLoading(false)
        }
      })

    return () => {
      cancelled = true
    }
  }, [extensionId, componentName])

  return { component, loading, error }
}

/**
 * React Hook: Preload components for an extension
 *
 * @param extensionIds Extension IDs to preload
 * @param options Options for preloading
 *
 * @example
 * ```tsx
 * usePreloadExtensions(['weather-v2', 'image-analyzer'])
 * ```
 */
export function usePreloadExtensions(
  extensionIds: string[],
  options?: {
    delay?: number  // Delay before preloading (ms)
    enabled?: boolean  // Enable/disable preloading
  }
) {
  const { delay = 0, enabled = true } = options || {}

  React.useEffect(() => {
    if (!enabled) return

    const timer = setTimeout(() => {
      extensionComponentLoader.preloadExtensions(extensionIds)
    }, delay)

    return () => clearTimeout(timer)
  }, [extensionIds, delay, enabled])
}

/**
 * React Hook: Get component loading state
 *
 * @param extensionId Extension ID
 * @param componentName Component name
 * @returns Loading state
 */
export function useComponentLoadingState(
  extensionId: string,
  componentName: string
): ComponentLoadingState | undefined {
  const [state, setState] = React.useState<ComponentLoadingState | undefined>(
    extensionComponentLoader.getLoadingState(extensionId, componentName)
  )

  React.useEffect(() => {
    // Check state periodically
    const interval = setInterval(() => {
      const newState = extensionComponentLoader.getLoadingState(extensionId, componentName)
      setState(newState)
    }, 100)

    return () => clearInterval(interval)
  }, [extensionId, componentName])

  return state
}

/**
 * React Hook: Extension components registry
 *
 * @returns All registered extension components
 */
export function useExtensionComponents() {
  const [components, setComponents] = React.useState<DashboardComponentDto[]>([])

  React.useEffect(() => {
    // Initial load
    setComponents(dynamicRegistry.getAllMetas())

    // Poll for changes (in a real app, you'd use an event emitter)
    const interval = setInterval(() => {
      setComponents(dynamicRegistry.getAllMetas())
    }, 1000)

    return () => clearInterval(interval)
  }, [])

  return components
}
