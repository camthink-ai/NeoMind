// ============================================================================
// Dashboard Types
// ============================================================================

/**
 * Dashboard response from API
 */
export interface DashboardResponse {
  id: string
  name: string
  description?: string
  layout: {
    columns: number
    rows: number | 'auto'
    breakpoints: {
      lg: number
      md: number
      sm: number
      xs: number
    }
  }
  components: DashboardComponentResponse[]
  created_at: number
  updated_at: number
  is_default?: boolean
}

/**
 * Dashboard component response from API
 */
export interface DashboardComponentResponse {
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
  config?: Record<string, unknown>
  data_source?: {
    type: string
    endpoint?: string
    transform?: string
    refresh?: number
    params?: Record<string, unknown>
    static_value?: unknown
  }
  display?: Record<string, unknown>
  actions?: Array<{
    type: string
    method?: string
    endpoint?: string
    path?: string
    dialog?: string
    confirm?: boolean
  }>
}

/**
 * Request to create a dashboard
 */
export interface CreateDashboardRequest {
  name: string
  layout: DashboardResponse['layout']
  components: Omit<DashboardComponentResponse, 'id'>[]
}

/**
 * Request to update a dashboard
 */
export interface UpdateDashboardRequest {
  name?: string
  layout?: DashboardResponse['layout']
  components?: DashboardComponentResponse[]
}

/**
 * Dashboard template response
 */
export interface DashboardTemplateResponse {
  id: string
  name: string
  description: string
  category: string
  icon?: string
  layout: DashboardResponse['layout']
  components: Omit<DashboardComponentResponse, 'id'>[]
  required_resources?: {
    devices?: number
    agents?: number
    rules?: number
  }
}
