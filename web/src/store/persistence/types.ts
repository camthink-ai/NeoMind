/**
 * Persistence Layer - Types
 *
 * Abstract interface for dashboard storage operations.
 * Supports multiple storage backends (localStorage, API, hybrid).
 */

import type { Dashboard } from '@/types/dashboard'

// ============================================================================
// Storage Operation Result
// ============================================================================

export interface StorageResult<T> {
  data: T | null
  error: Error | null
  source: 'local' | 'api' | 'cache'
}

// ============================================================================
// Storage Backend Interface
// ============================================================================

export interface DashboardStorage {
  /**
   * Load all dashboards from storage
   */
  load(): Promise<StorageResult<Dashboard[]>>

  /**
   * Save all dashboards to storage
   */
  save(dashboards: Dashboard[]): Promise<StorageResult<void>>

  /**
   * Sync a single dashboard (create or update)
   */
  sync(dashboard: Dashboard): Promise<StorageResult<Dashboard>>

  /**
   * Delete a dashboard
   */
  delete(id: string): Promise<StorageResult<void>>

  /**
   * Clear all dashboard data from storage
   */
  clear(): void

  /**
   * Check if storage is available
   */
  isAvailable(): boolean

  /**
   * Get storage type identifier
   */
  getType(): string
}

// ============================================================================
// Storage Configuration
// ============================================================================

export interface StorageConfig {
  // Primary storage type
  primary: 'api' | 'local'

  // Fallback to localStorage if API fails
  fallback?: boolean

  // Cache API responses in localStorage
  cache?: boolean

  // Debounce sync operations (ms)
  debounceMs?: number

  // Auto-save changes
  autoSave?: boolean
}

// ============================================================================
// DTO Conversion (between API and internal format)
// ============================================================================

export interface DashboardDTO {
  id: string
  name: string
  layout: {
    columns: number
    rows: 'auto' | number
    breakpoints: {
      lg: number
      md: number
      sm: number
      xs: number
    }
  }
  components: Array<{
    id: string
    type: string
    position: {
      x: number
      y: number
      w: number
      h: number
      min_w?: number
      min_h?: number
      max_w?: number
      max_h?: number
    }
    title?: string
    dataSource?: Record<string, unknown>
    display?: Record<string, unknown>
    config?: Record<string, unknown>
    actions?: Array<Record<string, unknown>>
  }>
  created_at: number
  updated_at: number
  is_default?: boolean
}

export interface CreateDashboardDTO {
  name: string
  layout: DashboardDTO['layout']
  components: DashboardDTO['components']
}

export interface UpdateDashboardDTO {
  name?: string
  layout?: DashboardDTO['layout']
  components?: DashboardDTO['components']
}

// ============================================================================
// DTO Conversion Helpers
// ============================================================================

/**
 * Convert internal Dashboard to API DTO format
 * Returns API format with snake_case fields (data_source)
 */
export function toDashboardDTO(dashboard: Dashboard): any {
  return {
    id: dashboard.id,
    name: dashboard.name,
    layout: dashboard.layout,
    components: dashboard.components.map(c => ({
      id: c.id,
      type: c.type,
      position: {
        x: c.position.x,
        y: c.position.y,
        w: c.position.w,
        h: c.position.h,
        min_w: c.position.minW,
        min_h: c.position.minH,
        max_w: c.position.maxW,
        max_h: c.position.maxH,
      },
      title: (c as any).title,
      // API uses snake_case for data_source
      data_source: (c as any).dataSource as Record<string, unknown> | undefined,
      display: (c as any).display as Record<string, unknown> | undefined,
      config: (c as any).config,
      actions: (c as any).actions?.map((a: unknown) => a as Record<string, unknown>),
    })),
    created_at: dashboard.createdAt,
    updated_at: dashboard.updatedAt,
    is_default: dashboard.isDefault,
  }
}

/**
 * Convert API DTO to internal Dashboard format
 * Handles both DashboardDTO (camelCase) and API DashboardResponse (snake_case)
 */
export function fromDashboardDTO(dto: DashboardDTO | any): Dashboard {
  // Handle API response format with snake_case fields
  const components = (dto.components || []).map((c: any) => {
    // API uses data_source (snake_case), internal uses dataSource (camelCase)
    const dataSource = c.data_source ?? c.dataSource

    return {
      id: c.id,
      type: c.type as any,
      position: {
        x: c.position.x,
        y: c.position.y,
        w: c.position.w,
        h: c.position.h,
        minW: c.position.min_w,
        minH: c.position.min_h,
        maxW: c.position.max_w,
        maxH: c.position.max_h,
      },
      title: c.title,
      dataSource: dataSource as any,
      display: c.display as any,
      config: c.config,
      actions: c.actions as any,
    }
  })

  return {
    id: dto.id,
    name: dto.name,
    layout: dto.layout,
    components,
    createdAt: dto.created_at,
    updatedAt: dto.updated_at,
    isDefault: dto.is_default,
  }
}

/**
 * Convert to create DTO (without id and timestamps)
 * Returns API format with snake_case fields (data_source)
 */
export function toCreateDashboardDTO(dashboard: Omit<Dashboard, 'id' | 'createdAt' | 'updatedAt'>): any {
  return {
    name: dashboard.name,
    layout: dashboard.layout,
    components: dashboard.components.map(c => ({
      id: c.id,
      type: c.type,
      position: {
        x: c.position.x,
        y: c.position.y,
        w: c.position.w,
        h: c.position.h,
        min_w: c.position.minW,
        min_h: c.position.minH,
        max_w: c.position.maxW,
        max_h: c.position.maxH,
      },
      title: (c as any).title,
      // API uses snake_case for data_source
      data_source: (c as any).dataSource as Record<string, unknown> | undefined,
      display: (c as any).display as Record<string, unknown> | undefined,
      config: (c as any).config,
      actions: (c as any).actions?.map((a: unknown) => a as Record<string, unknown>),
    })),
  }
}

/**
 * Convert to update DTO (partial)
 * Returns API format with snake_case fields (data_source)
 */
export function toUpdateDashboardDTO(updates: Partial<Dashboard>): any {
  const dto: any = {}

  if (updates.name !== undefined) dto.name = updates.name
  if (updates.layout !== undefined) dto.layout = updates.layout

  if (updates.components !== undefined) {
    // API expects snake_case field names (data_source)
    dto.components = updates.components.map(c => ({
      id: c.id,
      type: c.type,
      position: {
        x: c.position.x,
        y: c.position.y,
        w: c.position.w,
        h: c.position.h,
        min_w: c.position.minW,
        min_h: c.position.minH,
        max_w: c.position.maxW,
        max_h: c.position.maxH,
      },
      title: (c as any).title,
      // API uses snake_case for data_source
      data_source: (c as any).dataSource as Record<string, unknown> | undefined,
      display: (c as any).display as Record<string, unknown> | undefined,
      config: (c as any).config,
      actions: (c as any).actions?.map((a: unknown) => a as Record<string, unknown>),
    }))
  }

  return dto
}
