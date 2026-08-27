/**
 * Community Component Registry
 *
 * Manages community marketplace dashboard components.
 * Components are installed from the marketplace and loaded dynamically via IIFE bundles.
 *
 * This registry follows the same pattern as DynamicRegistry for extension components,
 * but is specifically for community marketplace components.
 */

import { dynamicIconMap } from '@/lib/dynamicIcons'
import type { ComponentMeta } from './types'
import type { FrontendComponentMeta } from '@/types/frontend-component'
import { isTauriEnv, getServerOrigin } from '@/lib/api'

/**
 * Community component registry state
 */
interface CommunityRegistryState {
  // All registered community components by type
  components: Record<string, FrontendComponentMeta>

  // Loaded module cache
  loadedModules: Record<string, unknown>

  // Loading promises (for concurrent load requests)
  loadingPromises: Record<string, Promise<unknown>>
}

/**
 * Community component registry for marketplace components
 */
export class CommunityComponentRegistry {
  private state: CommunityRegistryState = {
    components: {},
    loadedModules: {},
    loadingPromises: {},
  }

  /**
   * Check if a component type is a community component
   */
  isCommunity(type: string): boolean {
    return type in this.state.components
  }

  /**
   * Get component metadata for a community component
   */
  getMeta(type: string): FrontendComponentMeta | undefined {
    return this.state.components[type]
  }

  /**
   * Check if a community component is registered
   */
  isRegistered(type: string): boolean {
    return type in this.state.components
  }

  /**
   * Get all community component metadata
   */
  getAllMetas(): FrontendComponentMeta[] {
    return Object.values(this.state.components)
  }

  /**
   * Sync from API response - incremental update
   * Compares existing components with new list and adds/removes as needed
   *
   * @param metas - Array of component metadata from API
   * @returns Object with counts of added and removed components
   */
  syncFromApi(metas: FrontendComponentMeta[]): { added: number; removed: number } {
    const newIds = new Set(metas.map(m => m.id))
    const currentIds = new Set(Object.keys(this.state.components))

    let added = 0
    let removed = 0

    // Find and remove components that no longer exist
    for (const id of currentIds) {
      if (!newIds.has(id)) {
        this.unregister(id)
        removed++
      }
    }

    // Add or update components
    for (const meta of metas) {
      const exists = currentIds.has(meta.id)

      // If component already exists and key fields changed, clear caches for fresh reload
      if (exists) {
        const oldMeta = this.state.components[meta.id]
        const changed = oldMeta?.global_name !== meta.global_name ||
          oldMeta?.export_name !== meta.export_name

        if (changed) {
          if (oldMeta?.global_name) {
            try {
              delete (window as any)[oldMeta.global_name]
            } catch (e) {
              console.warn(`[CommunityRegistry] Failed to clear global ${oldMeta.global_name}:`, e)
            }
          }
          delete this.state.loadedModules[meta.id]
          delete this.state.loadingPromises[meta.id]
        }
      }

      // Register component definition
      this.state.components[meta.id] = meta

      if (!exists) {
        added++
      }
    }

    return { added, removed }
  }

  /**
   * Load a component module dynamically via IIFE script tag
   *
   * @param type - Component type (ID)
   * @returns The loaded component module
   */
  async loadComponent(type: string): Promise<unknown> {
    // Check cache
    if (type in this.state.loadedModules) {
      return this.state.loadedModules[type]
    }

    // Check existing loading promise
    if (type in this.state.loadingPromises) {
      return this.state.loadingPromises[type]
    }

    const meta = this.state.components[type]
    if (!meta) {
      throw new Error(`Unknown community component: ${type}`)
    }

    // Start loading
    const promise = this.doLoadComponent(meta, type)
    this.state.loadingPromises[type] = promise

    try {
      const module = await promise
      // A null result means the load failed (doLoadComponent resolves null on
      // error). Never cache it — otherwise every automatic retry and the UI's
      // Retry button hit this cache and the component can only recover by
      // reloading the page.
      if (module == null) {
        return null
      }
      this.state.loadedModules[type] = module
      return module
    } finally {
      delete this.state.loadingPromises[type]
    }
  }

  /**
   * Internal method to load a component module
   * Uses script tag injection for IIFE bundles
   */
  private async doLoadComponent(meta: FrontendComponentMeta, type: string): Promise<unknown> {
    try {
      // Build bundle URL
      let bundleUrl = `/api/frontend-components/${meta.id}/bundle`
      const isTauri = isTauriEnv()

      // Handle API URLs - use script tag injection for IIFE bundles
      if (bundleUrl.startsWith('/api/')) {
        // For absolute URLs (Tauri or remote instance), use the dynamic server origin
        if (isTauri || getServerOrigin() !== window.location.origin) {
          const url = new URL(bundleUrl, getServerOrigin())
          url.searchParams.set('_t', Date.now().toString())
          bundleUrl = url.toString()
        } else {
          // Add cache-busting query parameter for web browser
          const separator = bundleUrl.includes('?') ? '&' : '?'
          bundleUrl = `${bundleUrl}${separator}_t=${Date.now()}`
        }

        // Use global_name from metadata
        const globalName = meta.global_name

        if (!globalName) {
          console.error(`[CommunityRegistry] No global_name defined for component: ${type}`)
          return null
        }

        const Component = await this.loadViaScriptTag(bundleUrl, globalName, meta.export_name, type)

        // Check if Component is valid
        if (!Component) {
          console.warn(`[CommunityRegistry] No export found for component: ${type}`)
          return null
        }

        // Check if Component is a valid React component type
        const isValidComponent = typeof Component === 'function' ||
          (typeof Component === 'object' && Component !== null &&
           ((Component as any).$$typeof || typeof (Component as any).render === 'function'))

        if (isValidComponent) {
          return Component
        }

        // If Component is an object, try to find a valid component inside it
        if (typeof Component === 'object' && Component !== null) {
          for (const key of Object.keys(Component)) {
            const prop = (Component as Record<string, unknown>)[key]
            if (typeof prop === 'function' ||
                (typeof prop === 'object' && prop !== null &&
                 ((prop as any).$$typeof || typeof (prop as any).render === 'function'))) {
              return prop
            }
          }
        }

        console.warn(`[CommunityRegistry] Component is not a valid React component: ${type}`)
        return null
      } else {
        // Standard dynamic import for local paths
        const module = await import(/* @vite-ignore */ bundleUrl)
        const exportName = meta.export_name || 'default'
        const Component = module[exportName] || module.default || module

        if (!Component || typeof Component !== 'function') {
          console.warn(`No export found for component: ${type} (tried: ${exportName}, default)`)
          return null
        }

        return Component
      }
    } catch (e) {
      console.error(`Failed to load community component: ${type}`, e)
      return null
    }
  }

  /**
   * Load an IIFE bundle via script tag injection
   * Returns the component export from the global variable
   */
  private async loadViaScriptTag(bundleUrl: string, globalName: string, exportName?: string, componentId?: string): Promise<unknown> {
    return new Promise((resolve, reject) => {
      // Check if the global variable already exists (bundle already loaded)
      const existingGlobal = (window as any)[globalName]
      if (existingGlobal) {
        // Get the export from the global
        const exportKey = exportName || 'default'
        let Component = existingGlobal[exportKey]

        // If exportName is specified, look for it as a named export
        if (!Component && exportName) {
          if (typeof existingGlobal === 'function') {
            Component = existingGlobal
          } else if (existingGlobal.default && typeof existingGlobal.default === 'function') {
            Component = existingGlobal.default
          }
        }

        if (Component) {
          resolve(Component)
          return
        }
      }

      // Create a script element to load the bundle
      const script = document.createElement('script')
      script.src = bundleUrl
      script.async = true
      if (componentId) {
        script.setAttribute('data-component-id', componentId)
      }

      // Set up load handler
      script.onload = () => {
        // Access the global variable
        const global = (window as any)[globalName]

        // Clean up
        document.head.removeChild(script)

        // Get the component from the global
        let Component: unknown = null

        if (global) {
          // First try: named export
          if (exportName && global[exportName]) {
            Component = global[exportName]
          }
          // Second try: default export
          else if (global.default && typeof global.default === 'function') {
            Component = global.default
          }
          // Third try: global itself is the component
          else if (typeof global === 'function') {
            Component = global
          }
          // Fourth try: find any function or React component export
          else {
            for (const key of Object.keys(global)) {
              const val = global[key]
              if (typeof val === 'function' ||
                  (typeof val === 'object' && val !== null && val.$$typeof)) {
                Component = val
                break
              }
            }
          }
        }

        if (Component) {
          // Check if it's a valid React component
          const typeofComponent = typeof Component
          const hasTypeof = (Component as any)?.$$typeof
          const hasRender = typeof (Component as any)?.render === 'function'
          const isValidComponent = typeofComponent === 'function' ||
            (typeofComponent === 'object' && Component !== null &&
             (hasTypeof || hasRender))

          if (isValidComponent) {
            resolve(Component)
          } else {
            console.error(`[CommunityRegistry] Component is not a valid React component: ${globalName}`, Component)
            reject(new Error(`Component is not a valid React component: ${globalName}`))
          }
        } else {
          console.error(`[CommunityRegistry] No component export found in global ${globalName}`)
          reject(new Error(`No component export found in global ${globalName}`))
        }
      }

      // Set up error handler
      script.onerror = (error) => {
        console.error(`[CommunityRegistry] Failed to load script for ${globalName}:`, error)
        try {
          document.head.removeChild(script)
        } catch {
          // Script already removed
        }
        reject(new Error(`Failed to load script: ${error}`))
      }

      // Inject the script
      document.head.appendChild(script)
    })
  }

  /**
   * Refresh a component: clear its cached module, loading promise, and old script tag.
   * Does NOT re-load — the caller should re-fetch from API and then loadComponent().
   *
   * @param type - Component type (ID) to refresh
   */
  refreshComponent(type: string): void {
    const meta = this.state.components[type]

    // Remove old <script> tag if present
    const oldScript = document.querySelector(`script[data-component-id="${type}"]`)
    if (oldScript) {
      oldScript.remove()
    }

    // Clear caches
    delete this.state.loadedModules[type]
    delete this.state.loadingPromises[type]

    // Clear global variable
    if (meta?.global_name) {
      try {
        delete (window as any)[meta.global_name]
      } catch (e) {
        console.warn(`[CommunityRegistry] Failed to clear global ${meta.global_name}:`, e)
      }
    }
  }

  /**
   * Unregister a component and cleanup
   *
   * @param type - Component type (ID) to unregister
   */
  unregister(type: string): void {
    const meta = this.state.components[type]
    if (!meta) return

    // Remove from components map
    delete this.state.components[type]

    // Remove from loaded modules cache
    delete this.state.loadedModules[type]
    delete this.state.loadingPromises[type]

    // Clear global variable
    if (meta.global_name) {
      try {
        delete (window as any)[meta.global_name]
      } catch (e) {
        console.warn(`[CommunityRegistry] Failed to clear global ${meta.global_name}:`, e)
      }
    }
  }

  /**
   * Convert FrontendComponentMeta to ComponentMeta for registry integration
   *
   * @param meta - Frontend component metadata from API
   * @returns ComponentMeta compatible with the component registry
   */
  communityMetaToComponentMeta(meta: FrontendComponentMeta): ComponentMeta {
    // Get icon component from lucide-react
    const iconName = meta.icon || 'Box'
    const IconComponent = dynamicIconMap[iconName] || dynamicIconMap.Box

    // Get localized name (current locale or fallback to English)
    const getName = (): string => {
      if (typeof meta.name === 'string') {
        return meta.name
      }
      // Try to get locale from i18n, fallback to 'en', then first available
      const locale = (window as any).__locale__ || 'en'
      return meta.name[locale] || meta.name.en || Object.values(meta.name)[0] || meta.id
    }

    // Get localized description
    const getDescription = (): string => {
      if (typeof meta.description === 'string') {
        return meta.description
      }
      const locale = (window as any).__locale__ || 'en'
      return meta.description[locale] || meta.description.en || Object.values(meta.description)[0] || ''
    }

    // Convert size_constraints from snake_case to camelCase
    const sizeConstraints = {
      minW: meta.size_constraints.min_w,
      minH: meta.size_constraints.min_h,
      defaultW: meta.size_constraints.default_w,
      defaultH: meta.size_constraints.default_h,
      maxW: meta.size_constraints.max_w,
      maxH: meta.size_constraints.max_h,
    }

    // Create prop checker from config schema
    const acceptsProp = (prop: string) => {
      if (!meta.config_schema || !meta.config_schema.properties) {
        return false
      }
      const allowedProps = Object.keys(meta.config_schema.properties)
      return allowedProps.includes(prop)
    }

    return {
      type: meta.id as any, // Community component types are dynamic
      name: getName(),
      description: getDescription(),
      category: (meta.source === 'marketplace' ? 'marketplace' : 'local') as any,
      icon: IconComponent,
      sizeConstraints: sizeConstraints as any,
      hasDataSource: meta.has_data_source,
      maxDataSources: meta.max_data_sources,
      hasDisplayConfig: meta.has_display_config,
      hasActions: meta.has_actions,
      hasDeviceBinding: meta.has_device_binding,
      acceptsProp,
      defaultProps: meta.default_config,
      variants: meta.variants,
    }
  }

  /**
   * Clear all registered components (for testing)
   */
  clear(): void {
    // Clear all global variables first
    for (const meta of Object.values(this.state.components)) {
      if (meta.global_name) {
        try {
          delete (window as any)[meta.global_name]
        } catch (e) {
          console.warn(`[CommunityRegistry] Failed to clear global ${meta.global_name}:`, e)
        }
      }
    }

    this.state = {
      components: {},
      loadedModules: {},
      loadingPromises: {},
    }
  }

  /**
   * Get the current state (for debugging)
   */
  getState(): Readonly<CommunityRegistryState> {
    return this.state
  }
}

// Singleton instance
export const communityRegistry = new CommunityComponentRegistry()
