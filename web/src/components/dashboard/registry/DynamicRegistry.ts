/**
 * Dynamic Component Registry for Extension Dashboard Components
 *
 * This module manages dashboard components provided by extensions,
 * allowing them to be dynamically loaded and registered at runtime.
 */

import * as React from 'react'
import * as ReactDOM from 'react-dom'
import * as lucideReact from 'lucide-react'
import { ComponentMeta } from './types'
import type { DashboardComponentDto, DashboardComponentsResponse } from '@/types'

// Make React and ReactDOM available globally for extension components
// Extension bundles are built with React as an external dependency
if (typeof window !== 'undefined') {
  (window as any).React = React
  ;(window as any).ReactDOM = ReactDOM
}

/**
 * Dynamic component registry state
 */
interface DynamicRegistryState {
  // All registered dynamic components by type
  components: Record<string, DashboardComponentDto>

  // Extension index
  extensions: Record<string, { extensionId: string; extensionName: string; componentTypes: string[] }>

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
      }
    }
    if (!this.state.extensions[extensionId].componentTypes.includes(def.type)) {
      this.state.extensions[extensionId].componentTypes.push(def.type)
    }
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

    // Remove extension index
    delete this.state.extensions[extensionId]
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
      // Check if we're in dev mode and need to use the full URL
      const isDev = import.meta.env.DEV
      let bundleUrl = def.bundle_url

      if (isDev && bundleUrl.startsWith('/api/')) {
        // Add cache-busting timestamp to avoid cached HTTP response
        const url = new URL(bundleUrl, window.location.origin)
        url.searchParams.set('_t', Date.now().toString())
        bundleUrl = url.toString()

        // For IIFE bundles, use script tag injection instead of dynamic import
        // IIFE assigns to a global variable, which we can access after script loads
        const globalName = this.getGlobalNameForType(type, def.extension_id)

        const Component = await this.loadViaScriptTag(bundleUrl, globalName, def.export_name)

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
   */
  private getGlobalNameForType(type: string, extensionId?: string): string {
    // Map component types to their global variable names (from vite.config.ts name field)
    const globalNames: Record<string, string> = {
      'weather-card': 'WeatherDashboardComponents',
      'image-analyzer': 'ImageAnalyzerDashboardComponents',
      'yolo-video-display': 'YoloVideoDashboardComponents',
    }

    // Direct type lookup first (most reliable)
    if (globalNames[type]) {
      return globalNames[type]
    }

    if (extensionId) {
      // Try to derive from extension ID
      if (extensionId.includes('weather') || extensionId.includes('forecast')) {
        return 'WeatherDashboardComponents'
      }
      if (extensionId.includes('image') || extensionId.includes('analyzer')) {
        return 'ImageAnalyzerDashboardComponents'
      }
      if (extensionId.includes('yolo') || extensionId.includes('video')) {
        return 'YoloVideoDashboardComponents'
      }
    }

    return 'ExtensionComponents'
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
          // Fourth try: find any function export
          else {
            for (const key of Object.keys(global)) {
              if (typeof global[key] === 'function') {
                Component = global[key]
                break
              }
            }
          }
        }

        if (Component) {
          // Check if it's a valid React component
          // - function: regular component
          // - object with $$typeof: forwardRef, memo, etc.
          const isValidComponent = typeof Component === 'function' ||
            (typeof Component === 'object' && Component !== null &&
             ((Component as any).$$typeof || (Component as any).render))

          if (isValidComponent) {
            resolve(Component)
          } else {
            console.error(`[DynamicRegistry] Component is not a valid React component: ${globalName}`)
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
    this.state = {
      components: {},
      extensions: {},
      loadedModules: {},
      loadingPromises: {},
    }
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
  const lucideRecord: any = lucideReact
  const IconComponent = lucideRecord[iconName] || lucideRecord.Box

  return {
    type: dto.type as any, // Extension component types are dynamic
    name: dto.name,
    description: dto.description,
    category: dto.category as any, // Type assertion needed for different ComponentCategory types
    icon: IconComponent,
    sizeConstraints: dto.size_constraints as any, // Convert SizeConstraints to ComponentSizeConstraints
    hasDataSource: dto.has_data_source,
    maxDataSources: dto.max_data_sources,
    hasDisplayConfig: dto.has_display_config,
    hasActions: dto.has_actions,
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
