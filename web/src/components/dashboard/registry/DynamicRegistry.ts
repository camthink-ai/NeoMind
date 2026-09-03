/**
 * Dynamic Component Registry for Extension Dashboard Components
 *
 * This module manages dashboard components provided by extensions,
 * allowing them to be dynamically loaded and registered at runtime.
 */

import * as React from 'react'
import * as ReactDOM from 'react-dom'
import * as jsxRuntime from 'react/jsx-runtime'
import { dynamicIconMap } from '@/lib/dynamicIcons'
import { ComponentMeta } from './types'
import type { DashboardComponentDto, DashboardComponentsResponse } from '@/types'
import { isTauriEnv, getServerOrigin } from '@/lib/api'

// Make React and ReactDOM available globally for extension components
// Extension bundles are built with React as an external dependency
if (typeof window !== 'undefined') {
  (window as any).React = React
  ;(window as any).ReactDOM = ReactDOM
  ;(window as any).jsxRuntime = jsxRuntime
}

/**
 * Dynamic component registry state
 */
interface DynamicRegistryState {
  // All registered dynamic components by type
  components: Record<string, DashboardComponentDto>

  // Extension index (includes globalNames for cleanup)
  extensions: Record<string, { extensionId: string; extensionName: string; componentTypes: string[]; globalNames: Set<string> }>

  // Loaded module cache
  loadedModules: Record<string, unknown>

  // Loading promises (for concurrent load requests)
  loadingPromises: Record<string, Promise<unknown>>
}

/**
 * Dynamic component registry for extension-provided dashboard components
 */
export class DynamicComponentRegistry {
  private state: DynamicRegistryState = {
    components: {},
    extensions: {},
    loadedModules: {},
    loadingPromises: {},
  }

  // Track preload <link> elements for cleanup
  private preloadElements: Map<string, HTMLLinkElement> = new Map()

  // Concurrency control for script loading (max 3 simultaneous loads)
  private static readonly MAX_CONCURRENT_LOADS = 3
  private static loadingQueue: Array<() => void> = []
  private static activeLoads = 0

  /**
   * Check if a component type is a dynamic (extension) component
   */
  isDynamic(type: string): boolean {
    return type in this.state.components
  }

  /**
   * Get component metadata for a dynamic component
   */
  getMeta(type: string): DashboardComponentDto | undefined {
    return this.state.components[type]
  }

  /**
   * Get all dynamic component metadata
   */
  getAllMetas(): DashboardComponentDto[] {
    return Object.values(this.state.components)
  }

  /**
   * Register a component definition
   */
  register(extensionId: string, extensionName: string, def: DashboardComponentDto): void {
    this.state.components[def.type] = def

    // Update extension index
    if (!this.state.extensions[extensionId]) {
      this.state.extensions[extensionId] = {
        extensionId,
        extensionName,
        componentTypes: [],
        globalNames: new Set(),
      }
    }
    if (!this.state.extensions[extensionId].componentTypes.includes(def.type)) {
      this.state.extensions[extensionId].componentTypes.push(def.type)
    }
    // Track global name for cleanup
    if (def.global_name) {
      this.state.extensions[extensionId].globalNames.add(def.global_name)
    }

    // Preload the bundle for faster loading
    this.preloadBundles(def)
  }

  /**
   * Unregister all components from an extension
   */
  unregisterExtension(extensionId: string): void {
    const extInfo = this.state.extensions[extensionId]
    if (!extInfo) return

    // Remove components
    for (const type of extInfo.componentTypes) {
      delete this.state.components[type]
      delete this.state.loadedModules[type]
      delete this.state.loadingPromises[type]
    }

    // Clear global variables for this extension
    // This ensures fresh load when extension is reinstalled
    if (extInfo.globalNames) {
      for (const globalName of extInfo.globalNames) {
        try {
          delete (window as any)[globalName]
        } catch (e) {
          console.warn(`[DynamicRegistry] Failed to clear global ${globalName}:`, e)
        }
      }
    }

    // Remove extension index
    delete this.state.extensions[extensionId]

    // Remove preload links for this extension's components
    for (const type of extInfo.componentTypes) {
      const link = this.preloadElements.get(type)
      if (link) {
        try {
          document.head.removeChild(link)
        } catch { /* link may already be gone */ }
        this.preloadElements.delete(type)
      }
    }
  }

  /**
   * Load a component module dynamically
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

    const def = this.state.components[type]
    if (!def) {
      throw new Error(`Unknown dynamic component: ${type}`)
    }

    // Start loading
    const promise = this.doLoadComponent(def, type)
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
   * For IIFE bundles, we use script tag injection which properly sets up global variables
   */
  private async doLoadComponent(def: DashboardComponentDto, type: string): Promise<unknown> {
    try {
      let bundleUrl = def.bundle_url
      const isTauri = isTauriEnv()

      // Handle API URLs - use script tag injection for IIFE bundles
      // This works for both Tauri and web browser environments
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

        // For IIFE bundles, use script tag injection instead of dynamic import
        // IIFE assigns to a global variable, which we can access after script loads
        // Use global_name from API if available, otherwise fall back to type-based mapping
        const globalName = def.global_name || this.getGlobalNameForType(type, def.extension_id)

        if (!globalName) {
          console.error(`[DynamicRegistry] No global_name defined for component: ${type}`)
          return null
        }

        // Acquire a concurrency slot before loading the script
        await this.acquireSlot()
        let Component: unknown
        try {
          Component = await this.loadViaScriptTag(bundleUrl, globalName, def.export_name)
        } finally {
          this.releaseSlot()
        }

        // Check if Component is valid - can be a function, forwardRef, memo, or object containing a function
        if (!Component) {
          console.warn(`[DynamicRegistry] No export found for component: ${type}`)
          return null
        }

        // Check if Component is a valid React component type
        // - function: regular component
        // - object with $$typeof: forwardRef, memo, etc.
        // - object with render function: some wrapped components
        const isValidComponent = typeof Component === 'function' ||
          (typeof Component === 'object' && Component !== null &&
           ((Component as any).$$typeof || typeof (Component as any).render === 'function'))

        if (isValidComponent) {
          return Component
        }

        // If Component is an object, try to find a valid component inside it
        if (typeof Component === 'object' && Component !== null) {
          // Check if it has a function property we can use
          for (const key of Object.keys(Component)) {
            const prop = (Component as Record<string, unknown>)[key]
            if (typeof prop === 'function' ||
                (typeof prop === 'object' && prop !== null &&
                 ((prop as any)?.$$typeof || typeof (prop as any).render === 'function'))) {
              return prop
            }
          }
        }

        console.warn(`[DynamicRegistry] Component is not a valid React component: ${type}`)
        return null
      } else {
        // Standard dynamic import for production or local paths
        const module = await import(/* @vite-ignore */ bundleUrl)
        const exportName = def.export_name || 'default'
        const Component = module[exportName] || module.default || module

        if (!Component || typeof Component !== 'function') {
          console.warn(`No export found for component: ${type} (tried: ${exportName}, default)`)
          return null
        }

        return Component
      }
    } catch (e) {
      console.error(`Failed to load extension component: ${type}`, e)
      // Return null instead of throwing to allow graceful fallback
      return null
    }
  }

  /**
   * Get the global variable name for an extension's IIFE bundle
   * This is a fallback for extensions that don't define global_name in their manifest
   * @deprecated Extensions should define global_name in their manifest.json
   */
  private getGlobalNameForType(type: string, extensionId?: string): string | undefined {
    // Map component types to their global variable names (from vite.config.ts name field)
    // NOTE: This is a fallback for legacy extensions. New extensions should define global_name in manifest.json
    const globalNames: Record<string, string> = {
      'weather-card': 'WeatherForecastV2Components',
      'image-card': 'ImageAnalyzerV2Components',
      'yolo-card': 'YoloVideoV2Components',
    }

    // Direct type lookup first (most reliable)
    if (globalNames[type]) {
      console.warn(`[DynamicRegistry] Using hardcoded global_name for ${type}. Please update manifest.json to include global_name field.`)
      return globalNames[type]
    }

    if (extensionId) {
      // Try to derive from extension ID
      if (extensionId.includes('weather') || extensionId.includes('forecast')) {
        console.warn(`[DynamicRegistry] Using derived global_name for ${type}. Please update manifest.json to include global_name field.`)
        return 'WeatherForecastV2Components'
      }
      if (extensionId.includes('image') || extensionId.includes('analyzer')) {
        console.warn(`[DynamicRegistry] Using derived global_name for ${type}. Please update manifest.json to include global_name field.`)
        return 'ImageAnalyzerV2Components'
      }
      if (extensionId.includes('yolo') || extensionId.includes('video')) {
        console.warn(`[DynamicRegistry] Using derived global_name for ${type}. Please update manifest.json to include global_name field.`)
        return 'YoloVideoV2Components'
      }
    }

    // Return undefined instead of a generic name to force proper configuration
    return undefined
  }

  /**
   * Load an IIFE bundle via script tag injection
   * Returns the component export from the global variable
   */
  private async loadViaScriptTag(bundleUrl: string, globalName: string, exportName?: string): Promise<unknown> {
    return new Promise((resolve, reject) => {
      // Check if the global variable already exists (bundle already loaded)
      const existingGlobal = (window as any)[globalName]
      if (existingGlobal) {
        // Get the export from the global
        const exportKey = exportName || 'default'
        let Component = existingGlobal[exportKey]

        // If exportName is specified, look for it as a named export
        if (!Component && exportName) {
          // IIFE with exports: 'named' puts named exports directly on global
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

      // Set up load handler
      script.onload = () => {
        // Access the global variable
        const global = (window as any)[globalName]

        // Clean up
        document.head.removeChild(script)

        // Get the component from the global
        let Component: unknown = null

        if (global) {
          // First try: named export (IIFE with exports: 'named')
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
          // - function: regular component
          // - object with $$typeof: forwardRef, memo, etc.
          const typeofComponent = typeof Component
          const hasTypeof = (Component as any)?.$$typeof
          const hasRender = typeof (Component as any)?.render === 'function'
          const isValidComponent = typeofComponent === 'function' ||
            (typeofComponent === 'object' && Component !== null &&
             (hasTypeof || hasRender))

          if (isValidComponent) {
            resolve(Component)
          } else {
            console.error(`[DynamicRegistry] Component is not a valid React component: ${globalName}`, Component)
            reject(new Error(`Component is not a valid React component: ${globalName}`))
          }
        } else {
          console.error(`[DynamicRegistry] No component export found in global ${globalName}`)
          reject(new Error(`No component export found in global ${globalName}`))
        }
      }

      // Set up error handler
      script.onerror = (error) => {
        console.error(`[DynamicRegistry] Failed to load script for ${globalName}:`, error)
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
   * Get all registered extensions
   */
  getExtensions(): Array<{ extensionId: string; extensionName: string; componentTypes: string[] }> {
    return Object.values(this.state.extensions)
  }

  /**
   * Clear all registered components (for testing)
   */
  clear(): void {
    // Remove all preload links
    for (const [, link] of this.preloadElements) {
      try {
        document.head.removeChild(link)
      } catch { /* link may already be gone */ }
    }
    this.preloadElements.clear()

    this.state = {
      components: {},
      extensions: {},
      loadedModules: {},
      loadingPromises: {},
    }
  }

  /**
   * Incremental sync: Compare and update only changes
   * Preserves already-loaded modules for unchanged components
   *
   * @returns Object with counts of added and removed components
   */
  syncComponents(newComponents: DashboardComponentDto[]): { added: number; removed: number; unchanged: number } {
    const newTypes = new Set(newComponents.map(c => c.type))
    const currentTypes = new Set(Object.keys(this.state.components))

    let added = 0
    let removed = 0
    let unchanged = 0

    // Find and remove components that no longer exist
    for (const type of currentTypes) {
      if (!newTypes.has(type)) {
        // Remove from components map
        delete this.state.components[type]
        // Remove from loaded modules cache
        delete this.state.loadedModules[type]
        delete this.state.loadingPromises[type]
        removed++
      }
    }

    // Add or update components
    for (const comp of newComponents) {
      const exists = currentTypes.has(comp.type)

      // If component already exists and key fields changed, clear caches for fresh reload
      if (exists) {
        const oldDef = this.state.components[comp.type]
        const changed = oldDef?.bundle_url !== comp.bundle_url
          || oldDef?.global_name !== comp.global_name
          || oldDef?.export_name !== comp.export_name
        if (changed) {
          if (oldDef?.global_name) {
            try { delete (window as any)[oldDef.global_name] } catch { /* property may be non-configurable */ }
          }
          delete this.state.loadedModules[comp.type]
          delete this.state.loadingPromises[comp.type]
        }
      }

      // Register component definition
      this.state.components[comp.type] = comp

      // Update extension index
      if (!this.state.extensions[comp.extension_id]) {
        this.state.extensions[comp.extension_id] = {
          extensionId: comp.extension_id,
          extensionName: comp.extension_id,
          componentTypes: [],
          globalNames: new Set(),
        }
      }
      const extInfo = this.state.extensions[comp.extension_id]
      if (!extInfo.componentTypes.includes(comp.type)) {
        extInfo.componentTypes.push(comp.type)
      }
      // Track global name for cleanup
      if (comp.global_name) {
        extInfo.globalNames.add(comp.global_name)
      }

      if (!exists) {
        // Preload bundle for new component
        this.preloadBundles(comp)
        added++
      } else {
        unchanged++
      }
    }

    // Clean up extension index for removed extensions
    for (const [extId, extInfo] of Object.entries(this.state.extensions)) {
      // Check if any of its components still exist
      const hasComponents = extInfo.componentTypes.some(type => newTypes.has(type))
      if (!hasComponents) {
        // Clear global variables before removing extension
        if (extInfo.globalNames) {
          for (const globalName of extInfo.globalNames) {
            try {
              delete (window as any)[globalName]
            } catch (e) {
              console.warn(`[DynamicRegistry] Failed to clear global ${globalName}:`, e)
            }
          }
        }
        delete this.state.extensions[extId]
      }
    }

    return { added, removed, unchanged }
  }

  /**
   * Clear module cache for a specific component type
   * Use this to force reload of a component after updates
   */
  clearModuleCache(type: string): void {
    delete this.state.loadedModules[type]
    delete this.state.loadingPromises[type]
  }

  /**
   * Clear all module caches
   * Use this to force reload all extension components
   */
  clearAllModuleCache(): void {
    this.state.loadedModules = {}
    this.state.loadingPromises = {}
  }

  /**
   * Get the current state (for debugging)
   */
  /**
   * Preload an extension bundle via <link rel="preload">
   * The browser downloads the bundle preemptively so that when loadViaScriptTag()
   * later injects the <script> tag, it hits the browser cache.
   */
  private preloadBundles(def: DashboardComponentDto): void {
    // Only preload IIFE bundles served from the API
    if (!def.bundle_url?.startsWith('/api/') || !def.global_name) return
    // Don't duplicate preloads for the same component type
    if (this.preloadElements.has(def.type)) return
    const link = document.createElement('link')
    link.rel = 'preload'
    link.href = def.bundle_url
    link.as = 'script'
    document.head.appendChild(link)
    this.preloadElements.set(def.type, link)
  }

  /**
   * Acquire a slot in the concurrent loading queue.
   * Waits if all slots are occupied.
   */
  private acquireSlot(): Promise<void> {
    return new Promise((resolve) => {
      if (DynamicComponentRegistry.activeLoads < DynamicComponentRegistry.MAX_CONCURRENT_LOADS) {
        DynamicComponentRegistry.activeLoads++
        resolve()
      } else {
        DynamicComponentRegistry.loadingQueue.push(resolve)
      }
    })
  }

  /**
   * Release a slot in the concurrent loading queue.
   * Dequeues the next waiting load if any.
   */
  private releaseSlot(): void {
    if (DynamicComponentRegistry.loadingQueue.length > 0) {
      const next = DynamicComponentRegistry.loadingQueue.shift()
      if (next) next()
    } else {
      DynamicComponentRegistry.activeLoads--
    }
  }

  getState(): Readonly<DynamicRegistryState> {
    return this.state
  }
}

// Singleton instance
export const dynamicRegistry = new DynamicComponentRegistry()

/**
 * Convert DashboardComponentDto to ComponentMeta
 */
export function dtoToComponentMeta(dto: DashboardComponentDto): ComponentMeta {
  // Get icon component from lucide-react
  const iconName = dto.icon || 'Box'
  const IconComponent = dynamicIconMap[iconName] || dynamicIconMap.Box

  return {
    type: dto.type as any, // Extension component types are dynamic
    name: dto.name,
    description: dto.description,
    // Extensions may omit or null the category — default them into the
    // canonical "custom" (Extension Components) group instead of letting
    // a null key fall through the grouping logic.
    category: (dto.category || 'custom') as any,
    icon: IconComponent,
    sizeConstraints: dto.size_constraints as any, // Convert SizeConstraints to ComponentSizeConstraints
    hasDataSource: dto.has_data_source,
    maxDataSources: dto.max_data_sources,
    hasDisplayConfig: dto.has_display_config,
    hasActions: dto.has_actions,
    hasDeviceBinding: dto.has_device_binding,
    acceptsProp: createPropChecker(dto.config_schema),
    defaultProps: dto.default_config,
    variants: dto.variants,
  }
}

/**
 * Create a prop checker function from JSON Schema
 */
function createPropChecker(schema?: { properties?: Record<string, unknown> }): (prop: string) => boolean {
  if (!schema || !schema.properties) return () => false
  const allowedProps = Object.keys(schema.properties)
  return (prop: string) => allowedProps.includes(prop)
}
