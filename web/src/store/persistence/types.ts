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
      minW?: number
      minH?: number
      maxW?: number
      maxH?: number
    }
    title?: string
    dataSource?: Record<string, unknown>
    display?: Record<string, unknown>
    config?: Record<string, unknown>
    actions?: Array<Record<string, unknown>>
  }>
  created_at: number
  updated_at: number
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
 */
export function toDashboardDTO(dashboard: Dashboard): DashboardDTO {
  return {
    id: dashboard.id,
    name: dashboard.name,
    layout: dashboard.layout,
    components: dashboard.components.map(c => ({
      id: c.id,
      type: c.type,
      position: c.position,
      title: (c as any).title,
      dataSource: (c as any).dataSource as Record<string, unknown> | undefined,
      display: (c as any).display as Record<string, unknown> | undefined,
      config: (c as any).config,
      actions: (c as any).actions?.map((a: unknown) => a as Record<string, unknown>),
    })),
    created_at: dashboard.createdAt,
    updated_at: dashboard.updatedAt,
  }
}

/**
 * Convert API DTO to internal Dashboard format
 */
export function fromDashboardDTO(dto: DashboardDTO): Dashboard {
  return {
    id: dto.id,
    name: dto.name,
    layout: dto.layout,
    components: dto.components.map(c => ({
      id: c.id,
      type: c.type as any,
      position: c.position,
      title: c.title,
      dataSource: c.dataSource as any,
      display: c.display as any,
      config: c.config,
      actions: c.actions as any,
    })),
    createdAt: dto.created_at,
    updatedAt: dto.updated_at,
  }
}

/**
 * Convert to create DTO (without id and timestamps)
 */
export function toCreateDashboardDTO(dashboard: Omit<Dashboard, 'id' | 'createdAt' | 'updatedAt'>): CreateDashboardDTO {
  return {
    name: dashboard.name,
    layout: dashboard.layout,
    components: dashboard.components.map(c => ({
      id: c.id,
      type: c.type,
      position: c.position,
      title: (c as any).title,
      dataSource: (c as any).dataSource as Record<string, unknown> | undefined,
      display: (c as any).display as Record<string, unknown> | undefined,
      config: (c as any).config,
      actions: (c as any).actions?.map((a: unknown) => a as Record<string, unknown>),
    })),
  }
}

/**
 * Convert to update DTO (partial)
 */
export function toUpdateDashboardDTO(updates: Partial<Dashboard>): UpdateDashboardDTO {
  const dto: UpdateDashboardDTO = {}

  if (updates.name !== undefined) dto.name = updates.name
  if (updates.layout !== undefined) dto.layout = updates.layout

  if (updates.components !== undefined) {
    dto.components = updates.components.map(c => ({
      id: c.id,
      type: c.type,
      position: c.position,
      title: (c as any).title,
      dataSource: (c as any).dataSource as Record<string, unknown> | undefined,
      display: (c as any).display as Record<string, unknown> | undefined,
      config: (c as any).config,
      actions: (c as any).actions?.map((a: unknown) => a as Record<string, unknown>),
    }))
  }

  return dto
}
