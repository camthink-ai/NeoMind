/**
 * Persistence Layer - Types
 *
 * Abstract interface for dashboard storage operations.
 * Supports multiple storage backends (localStorage, API, hybrid).
 */

import type { Dashboard, DashboardComponent, ComponentPosition, GenericComponent, BusinessComponent, DataSource, DataSourceOrList, ActionConfig, DisplayConfig } from '@/types/dashboard'
import { isGenericComponent, normalizeDataSource } from '@/types/dashboard'

// ============================================================================
// Storage Operation Result
// ============================================================================

export interface StorageResult<T> {
  data: T | null
  error: Error | null
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
   * Persist a full manual ordering of dashboard IDs (index 0 = top).
   * Backend assigns each its index as `sort_order`. Returns ok:false on error.
   */
  reorder?(dashboardIds: string[]): Promise<StorageResult<void>>

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

/** Component in API snake_case format */
export interface ComponentDTO {
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
  data_source?: Record<string, unknown> | Record<string, unknown>[]
  display?: Record<string, unknown>
  config?: Record<string, unknown>
  actions?: Array<Record<string, unknown>>
}

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
  components: ComponentDTO[]
  created_at: number
  updated_at: number
  is_default?: boolean
  sort_order?: number
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

/** Convert internal ComponentPosition to API snake_case position */
function positionToDTO(p: ComponentPosition): ComponentDTO['position'] {
  return {
    x: p.x,
    y: p.y,
    w: p.w,
    h: p.h,
    min_w: p.minW,
    min_h: p.minH,
    max_w: p.maxW,
    max_h: p.maxH,
  }
}

/**
 * Strip runtime-only fields from a data source before it reaches the API DTO.
 * `_saveTs` is an in-memory re-render stamp written after config saves; any
 * layout drag that follows would otherwise serialize the stamp into the
 * backend payload.
 */
function stripRuntimeFields(ds: unknown): unknown {
  if (Array.isArray(ds)) return ds.map(stripRuntimeFields)
  if (ds && typeof ds === 'object') {
    const { _saveTs, ...rest } = ds as Record<string, unknown>
    return rest
  }
  return ds
}

/** Convert a single DashboardComponent to API snake_case component DTO */
function componentToDTO(c: DashboardComponent): ComponentDTO {
  const isGeneric = isGenericComponent(c)
  const dataSource = (isGeneric ? (c as GenericComponent).dataSource : (c as BusinessComponent).dataSource) as (Record<string, unknown> | Record<string, unknown>[]) | undefined
  return {
    id: c.id,
    type: c.type,
    position: positionToDTO(c.position),
    title: c.title,
    data_source: dataSource === undefined ? undefined : (stripRuntimeFields(dataSource) as (Record<string, unknown> | Record<string, unknown>[])),
    display: isGeneric ? ((c as GenericComponent).display as Record<string, unknown> | undefined) : undefined,
    config: 'config' in c ? ((c as GenericComponent).config as Record<string, unknown> | undefined) : undefined,
    actions: isGeneric && (c as GenericComponent).actions
      ? ((c as GenericComponent).actions as unknown as Array<Record<string, unknown>>)
      : undefined,
  }
}

/** Convert API snake_case position to internal ComponentPosition.
 *  Returns a safe default if the position object is missing or malformed.
 *  Clamps x/y >= 0 and w/h >= 1 to prevent invalid grid positions. */
function positionFromDTO(p: ComponentDTO['position'] | undefined | null): ComponentPosition {
  if (!p || typeof p !== 'object') {
    return { x: 0, y: 0, w: 4, h: 3 }
  }
  const x = typeof p.x === 'number' ? Math.max(0, p.x) : 0
  const y = typeof p.y === 'number' ? Math.max(0, p.y) : 0
  const w = typeof p.w === 'number' ? Math.max(1, p.w) : 4
  const h = typeof p.h === 'number' ? Math.max(1, p.h) : 3
  return {
    x,
    y,
    w,
    h,
    minW: p.min_w,
    minH: p.min_h,
    maxW: p.max_w,
    maxH: p.max_h,
  }
}

/**
 * Convert internal Dashboard to API DTO format
 * Returns API format with snake_case fields (data_source)
 */
export function toDashboardDTO(dashboard: Dashboard): CreateDashboardDTO & { id: string; created_at: number; updated_at: number; is_default?: boolean; sort_order?: number } {
  return {
    id: dashboard.id,
    name: dashboard.name,
    layout: dashboard.layout,
    components: dashboard.components.map(componentToDTO),
    created_at: dashboard.createdAt,
    updated_at: dashboard.updatedAt,
    is_default: dashboard.isDefault,
    sort_order: dashboard.sortOrder,
  }
}

/**
 * Convert API DTO to internal Dashboard format
 * Handles both DashboardDTO (camelCase) and API DashboardResponse (snake_case)
 */
export function fromDashboardDTO(dto: DashboardDTO): Dashboard {
  const components: DashboardComponent[] = (dto.components || [])
    .filter((c): c is ComponentDTO & { id: string; type: string } =>
      c != null && typeof c === 'object' && typeof c.id === 'string' && typeof c.type === 'string'
    )
    .map((c) => {
    const dataSource = c.data_source

    // Normalize data sources to resolve legacy fields (aggregate→aggregateExt, timeRange→timeWindow)
    const normalizedDS = dataSource
      ? normalizeDataSource(dataSource as unknown as DataSourceOrList)
      : undefined

    // Preserve single vs array format: 1 source → DataSource, 2+ sources → DataSource[]
    const dsValue = normalizedDS && normalizedDS.length > 0
      ? (normalizedDS.length === 1 ? normalizedDS[0] : normalizedDS)
      : undefined

    const base = {
      id: c.id,
      type: c.type as DashboardComponent['type'],
      position: positionFromDTO(c.position),
      title: c.title,
    }

    if (isGenericComponent({ type: c.type } as DashboardComponent)) {
      const comp: GenericComponent = {
        ...base,
        type: c.type as GenericComponent['type'],
        ...(dsValue ? { dataSource: dsValue } : {}),
        ...(c.display ? { display: c.display as unknown as DisplayConfig } : {}),
        ...(c.config && typeof c.config === 'object' && !Array.isArray(c.config) ? { config: c.config } : {}),
        ...(c.actions ? { actions: c.actions as unknown as ActionConfig[] } : {}),
      }
      return comp
    }

    // BusinessComponent: config + dataSource (for agent binding, etc.)
    const comp: BusinessComponent = {
      ...base,
      type: c.type as BusinessComponent['type'],
      ...(dsValue ? { dataSource: dsValue } : {}),
      ...(c.config && typeof c.config === 'object' && !Array.isArray(c.config) ? { config: c.config } : {}),
    }
    return comp
  })

  return {
    id: dto.id,
    name: dto.name,
    layout: dto.layout,
    components,
    createdAt: dto.created_at,
    updatedAt: dto.updated_at,
    isDefault: dto.is_default,
    sortOrder: dto.sort_order,
  }
}

/**
 * Convert to create DTO (without id and timestamps)
 * Returns API format with snake_case fields (data_source)
 */
export function toCreateDashboardDTO(dashboard: Omit<Dashboard, 'id' | 'createdAt' | 'updatedAt'>): CreateDashboardDTO {
  return {
    name: dashboard.name,
    layout: dashboard.layout,
    components: dashboard.components.map(componentToDTO),
  }
}

/**
 * Convert to update DTO (partial)
 * Returns API format with snake_case fields (data_source)
 */
export function toUpdateDashboardDTO(updates: Partial<Dashboard>): UpdateDashboardDTO {
  const dto: UpdateDashboardDTO = {}

  if (updates.name !== undefined) dto.name = updates.name
  if (updates.layout !== undefined) dto.layout = updates.layout

  if (updates.components !== undefined) {
    dto.components = updates.components.map(componentToDTO)
  }

  return dto
}
