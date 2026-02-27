/**
 * NeoMind Extension Frontend SDK Types
 *
 * Type definitions for building extension frontend components.
 */

import { ReactNode, ComponentType, Ref, RefObject } from 'react'

// ============================================================================
// Core Types
// ============================================================================

/**
 * Extension component props base
 */
export interface ExtensionComponentProps {
  /** Component title */
  title?: string
  /** Custom class name */
  className?: string
  /** Custom styles */
  style?: React.CSSProperties
  /** Data source configuration */
  dataSource?: DataSource
  /** Refresh interval in milliseconds */
  refreshInterval?: number
  /** Dark mode support */
  darkMode?: boolean
}

/**
 * Data source configuration
 */
export interface DataSource {
  /** Data source type */
  type: string
  /** Extension ID providing the data */
  extensionId?: string
  /** Metric name to fetch */
  metric?: string
  /** Command to execute */
  command?: string
  /** Command arguments */
  args?: Record<string, unknown>
  /** Additional configuration */
  [key: string]: unknown
}

/**
 * API response wrapper
 */
export interface ApiResponse<T = unknown> {
  /** Whether the request succeeded */
  success: boolean
  /** Response data */
  data?: T
  /** Error message */
  error?: string
  /** Timestamp */
  timestamp?: string
}

/**
 * Extension metadata for frontend
 */
export interface ExtensionInfo {
  /** Extension ID */
  id: string
  /** Display name */
  name: string
  /** Version */
  version: string
  /** Description */
  description?: string
  /** Author */
  author?: string
  /** Available commands */
  commands: CommandInfo[]
  /** Available metrics */
  metrics: MetricInfo[]
  /** Frontend component metadata */
  components?: ComponentMetadata[]
}

/**
 * Command information
 */
export interface CommandInfo {
  /** Command name */
  name: string
  /** Display name */
  displayName: string
  /** Description */
  description?: string
  /** Parameter schema */
  parameters?: ParameterSchema[]
  /** Template for command arguments */
  payloadTemplate?: string
}

/**
 * Metric information
 */
export interface MetricInfo {
  /** Metric name */
  name: string
  /** Display name */
  displayName: string
  /** Data type */
  dataType: 'integer' | 'float' | 'boolean' | 'string'
  /** Unit */
  unit?: string
  /** Description */
  description?: string
}

/**
 * Parameter schema
 */
export interface ParameterSchema {
  /** Parameter name */
  name: string
  /** Parameter type */
  type: 'string' | 'number' | 'boolean' | 'object' | 'array'
  /** Required flag */
  required?: boolean
  /** Default value */
  default?: unknown
  /** Description */
  description?: string
}

/**
 * Component metadata
 */
export interface ComponentMetadata {
  /** Component name */
  name: string
  /** Component type */
  type: 'card' | 'widget' | 'panel' | 'dialog' | 'settings'
  /** Display name */
  displayName: string
  /** Description */
  description?: string
  /** Default size */
  defaultSize?: {
    width: number
    height: number
  }
  /** Min size */
  minSize?: {
    width: number
    height: number
  }
  /** Max size */
  maxSize?: {
    width: number
    height: number
  }
  /** Configuration schema */
  configSchema?: Record<string, ParameterSchema>
}

// ============================================================================
// API Helpers
// ============================================================================

/**
 * Get authentication token from storage
 */
export function getAuthToken(): string | null {
  if (typeof window === 'undefined') return null

  return (
    localStorage.getItem('neomind_token') ||
    sessionStorage.getItem('neomind_token_session') ||
    localStorage.getItem('token') ||
    null
  )
}

/**
 * Get API base URL
 */
export function getApiBase(): string {
  if (typeof window !== 'undefined' && (window as Window & { __TAURI__?: unknown }).__TAURI__) {
    return 'http://localhost:9375/api'
  }
  return '/api'
}

/**
 * API request options
 */
export interface ApiRequestOptions {
  /** HTTP method */
  method?: 'GET' | 'POST' | 'PUT' | 'DELETE'
  /** Request headers */
  headers?: Record<string, string>
  /** Request body */
  body?: unknown
  /** Timeout in milliseconds */
  timeout?: number
}

/**
 * Make an authenticated API request
 */
export async function apiRequest<T = unknown>(
  path: string,
  options: ApiRequestOptions = {}
): Promise<ApiResponse<T>> {
  const { method = 'GET', headers = {}, body, timeout = 30000 } = options

  const token = getAuthToken()
  const apiBase = getApiBase()

  const requestHeaders: Record<string, string> = {
    'Content-Type': 'application/json',
    ...headers,
  }

  if (token) {
    requestHeaders['Authorization'] = `Bearer ${token}`
  }

  const controller = new AbortController()
  const timeoutId = setTimeout(() => controller.abort(), timeout)

  try {
    const response = await fetch(`${apiBase}${path}`, {
      method,
      headers: requestHeaders,
      body: body ? JSON.stringify(body) : undefined,
      signal: controller.signal,
    })

    clearTimeout(timeoutId)

    if (!response.ok) {
      return {
        success: false,
        error: response.status === 401
          ? 'Authentication required'
          : `HTTP ${response.status}: ${response.statusText}`,
      }
    }

    const data = await response.json()
    return {
      success: true,
      data,
    }
  } catch (error) {
    clearTimeout(timeoutId)

    if (error instanceof Error && error.name === 'AbortError') {
      return {
        success: false,
        error: 'Request timeout',
      }
    }

    return {
      success: false,
      error: error instanceof Error ? error.message : 'Unknown error',
    }
  }
}

/**
 * Execute an extension command
 */
export async function executeExtensionCommand<T = unknown>(
  extensionId: string,
  command: string,
  args: Record<string, unknown> = {}
): Promise<ApiResponse<T>> {
  return apiRequest<T>(`/extensions/${extensionId}/command`, {
    method: 'POST',
    body: { command, args },
  })
}

/**
 * Get extension metrics
 */
export async function getExtensionMetrics(
  extensionId: string
): Promise<ApiResponse<Record<string, unknown>>> {
  return apiRequest<Record<string, unknown>>(`/extensions/${extensionId}/metrics`)
}

/**
 * Get extension information
 */
export async function getExtensionInfo(extensionId: string): Promise<ApiResponse<ExtensionInfo>> {
  return apiRequest<ExtensionInfo>(`/extensions/${extensionId}`)
}

// ============================================================================
// Component Utilities
// ============================================================================

/**
 * Create a styled extension component
 */
export function createExtensionComponent<P extends ExtensionComponentProps>(
  component: ComponentType<P>,
  metadata: ComponentMetadata
): {
  component: ComponentType<P>
  metadata: ComponentMetadata
} {
  return {
    component,
    metadata,
  }
}

/**
 * CSS variable helper for theming
 */
export function createCssVariables(vars: Record<string, string | number>): string {
  return Object.entries(vars)
    .map(([key, value]) => `--${key}: ${value};`)
    .join(' ')
}

/**
 * Dark mode CSS variable overrides
 */
export function createDarkModeOverrides(
  vars: Record<string, string | number>
): Record<string, string | number> {
  const darkVars: Record<string, string | number> = {}
  for (const [key, value] of Object.entries(vars)) {
    // Adjust colors for dark mode
    if (typeof value === 'string' && value.startsWith('hsl')) {
      darkVars[key] = value // Could add dark mode adjustment logic here
    } else {
      darkVars[key] = value
    }
  }
  return darkVars
}

// ============================================================================
// Export Types
// ============================================================================

export type {
  ExtensionComponentProps,
  DataSource,
  ApiResponse,
  ExtensionInfo,
  CommandInfo,
  MetricInfo,
  ParameterSchema,
  ComponentMetadata,
}

export {
  getAuthToken,
  getApiBase,
  apiRequest,
  executeExtensionCommand,
  getExtensionMetrics,
  getExtensionInfo,
  createExtensionComponent,
  createCssVariables,
  createDarkModeOverrides,
}
