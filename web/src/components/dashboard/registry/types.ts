/**
 * Component Registry - Types
 *
 * Centralized metadata for all dashboard components.
 */

import type { ComponentType, ComponentSizeConstraints } from '@/types/dashboard'

// ============================================================================
// Component Categories
// ============================================================================

export type ComponentCategory =
  | 'indicators'    // Value displays, metrics
  | 'charts'        // Visual data representations
  | 'controls'      // Interactive inputs
  | 'lists'         // Tables, lists, feeds
  | 'display'       // Content display (images, web, markdown)
  | 'layout'        // Structural components
  | 'business'      // NeoTalk-specific components

// ============================================================================
// Component Metadata
// ============================================================================

export interface ComponentMeta {
  // Basic info
  type: ComponentType
  name: string
  description: string
  category: ComponentCategory

  // Display
  icon: React.ComponentType<{ className?: string }>

  // Sizing
  sizeConstraints: ComponentSizeConstraints

  // Configuration
  hasDataSource: boolean      // Does this component use data binding?
  maxDataSources?: number     // Max number of data sources (1 = single, undefined = 1, >1 = multiple)
  hasDisplayConfig: boolean   // Can this component be styled?
  hasActions: boolean         // Does this component support actions?

  // Props acceptance (for config builder)
  acceptsProp: (prop: string) => boolean

  // Default config
  defaultProps?: Record<string, unknown>

  // Variants (if multiple styles)
  variants?: string[]
}

// ============================================================================
// Registry Type
// ============================================================================

export type ComponentRegistry = Record<ComponentType, ComponentMeta>

// ============================================================================
// Filter Options
// ============================================================================

export interface RegistryFilterOptions {
  category?: ComponentCategory
  hasDataSource?: boolean
  searchQuery?: string
}

// ============================================================================
// Grouped Registry
// ============================================================================

export interface GroupedRegistry {
  category: ComponentCategory
  components: ComponentMeta[]
}

export type GroupedComponentRegistry = GroupedRegistry[]
