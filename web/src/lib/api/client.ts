/** HTTP client: base-URL resolution, auth token plumbing, fetchAPI. Split from api.ts 2026-09. */

// Polyfill for AbortSignal.timeout (for older WebKit/Safari)
if (typeof AbortSignal !== 'undefined' && !AbortSignal.timeout) {
  AbortSignal.timeout = function (ms: number): AbortSignal {
    const controller = new AbortController()
    setTimeout(() => controller.abort(new DOMException('TimeoutError', 'TimeoutError')), ms)
    return controller.signal
  }
}

import { notifyFromError, notifySuccess } from '../notify'
import { tokenManager as unifiedTokenManager } from '../auth'
import { setApiKey as _setApiKey, clearApiKey as _clearApiKey, getApiKey as _getApiKey, resetToDefault as _resetToDefault, buildWsUrl as _buildWsUrl } from '../urls'

export function isTauriEnv(): boolean {
  return typeof window !== 'undefined' && '__TAURI__' in window
}

// Dynamic URL management — imported from urls.ts (re-exported for convenience)
export const setApiKey = _setApiKey
export const clearApiKey = _clearApiKey
export const getApiKey = _getApiKey
export const resetToDefault = _resetToDefault
export const buildWsUrl = _buildWsUrl

/**
 * Get API base URL based on environment.
 * Supports runtime switching via setApiBase().
 */
export function getApiBase(): string {
  // Dynamic override takes priority (set by InstanceSlice)
  const dynamicBase = _dynamicApiBase
  if (dynamicBase) return dynamicBase

  // Check for explicit environment variable
  const envApiBase = import.meta.env.VITE_API_BASE_URL
  if (envApiBase) return envApiBase

  // Tauri desktop: direct connection
  if (isTauriEnv()) return 'http://localhost:9375/api'

  // Web (dev/prod): use relative path
  return '/api'
}

/** Get server origin URL based on environment */
export function getServerOrigin(): string {
  const base = getApiBase()
  try {
    const url = new URL(base)
    return url.origin
  } catch {
    if (typeof window !== 'undefined') return window.location.origin
    return 'http://localhost:9375'
  }
}

// Internal state for dynamic URL override
let _dynamicApiBase = ''

/** Set dynamic API base (used by InstanceSlice for instance switching) */
export function setApiBase(url: string): void {
  _dynamicApiBase = url
}

// ============================================================================
// 401 Handling Callback Registry
// ============================================================================

type UnauthorizedCallback = () => void
const unauthorizedCallbacks: Set<UnauthorizedCallback> = new Set()

export function onUnauthorized(callback: UnauthorizedCallback) {
  unauthorizedCallbacks.add(callback)
  return () => unauthorizedCallbacks.delete(callback)
}

// Trigger all registered callbacks when 401 is encountered
function triggerUnauthorizedCallbacks() {
  unauthorizedCallbacks.forEach(cb => {
    try {
      cb()
    } catch (error) {
      console.error('Error in unauthorized callback:', error)
    }
  })
}

// Throttle 401 toasts: when a session expires, multiple concurrent API calls
// return 401 almost simultaneously. Only show one "Unauthorized" toast per window.
let lastUnauthorizedToastTime = 0
function shouldShowUnauthorizedToast(): boolean {
  const now = Date.now()
  if (now - lastUnauthorizedToastTime < 3000) {
    return false
  }
  lastUnauthorizedToastTime = now
  return true
}

// ============================================================================
// Re-export Token Manager from auth module
// ============================================================================

/**
 * Token Manager - unified authentication token management.
 * @deprecated Use `import { tokenManager } from '@/lib/auth'` instead.
 * This export is maintained for backward compatibility.
 */
export const tokenManager = unifiedTokenManager

// ============================================================================
// Enhanced Fetch with Auth
// ============================================================================

export interface FetchOptions extends RequestInit {
  skipAuth?: boolean
  skipGlobalError?: boolean
  skipErrorToast?: boolean  // Skip automatic error toast notification
  successMessage?: string   // Auto-show success toast with this message
  signal?: AbortSignal      // For timeout/cancellation support
}

export async function fetchAPI<T>(
  path: string,
  options: FetchOptions = {}
): Promise<T> {
  const {
    skipAuth = false,
    skipGlobalError = false,
    skipErrorToast = false,
    successMessage,
    signal,
    ...fetchOptions
  } = options

  const headers: Record<string, string> = {
    'Content-Type': 'application/json',
    ...(fetchOptions.headers as Record<string, string> || {}),
  }

  // Add JWT token authentication
  if (!skipAuth) {
    const token = tokenManager.getToken()
    if (token) {
      headers['Authorization'] = `Bearer ${token}`
    }
    // Also add API key if set (for remote instance auth)
    const apiKey = getApiKey()
    if (apiKey) {
      headers['X-API-Key'] = apiKey
    }
  }

  // Ensure headers is not undefined for headers as Record<string, string>
  const finalHeaders = headers as Record<string, string>

  // Auto-retry gateway errors (502/503/504) and rate limiting (429)
  // Not 500 (application bugs) — avoids duplicate operations on POST/PUT
  const MAX_RETRIES = 3
  const RETRY_DELAYS = [500, 1500, 3000] // Progressive backoff
  const RETRYABLE_STATUS = [429, 502, 503, 504]
  let response = await fetch(`${getApiBase()}${path}`, {
    ...fetchOptions,
    headers: finalHeaders,
    signal,
  })

  if (RETRYABLE_STATUS.includes(response.status) && !signal?.aborted) {
    for (let i = 0; i < MAX_RETRIES; i++) {
      // For 429, wait longer before retrying
      const delay = response.status === 429 ? RETRY_DELAYS[i] * 2 : RETRY_DELAYS[i]
      await new Promise(r => setTimeout(r, delay))
      response = await fetch(`${getApiBase()}${path}`, {
        ...fetchOptions,
        headers: finalHeaders,
        signal,
      })
      if (response.ok || !RETRYABLE_STATUS.includes(response.status)) break
    }
  }

  // Parse error response to extract meaningful message
  const parseErrorMessage = async (response: Response): Promise<string> => {
    try {
      const text = await response.text()
      if (!text) return `API Error: ${response.status}`

      const json = JSON.parse(text)

      // Format: { code: "...", message: "..." } - Unified ErrorResponse format
      if (json.message) {
        // If there's a code, include it for context but show the message
        if (json.code && json.code !== 'INTERNAL_ERROR') {
          return `${json.code}: ${json.message}`
        }
        return json.message
      }

      // Handle different error response formats (legacy)
      if (json.error) {
        // Format: { error: { code: "...", message: "..." } }
        if (typeof json.error === 'object' && json.error.message) {
          return json.error.message
        }
        // Format: { error: "Error message" }
        if (typeof json.error === 'string') {
          return json.error
        }
      }

      // Format: { detail: "..." }
      if (json.detail) {
        return json.detail
      }

      return text
    } catch {
      return `API Error: ${response.status}`
    }
  }

  // Handle 401 Unauthorized - trigger callbacks and throw error
  if (response.status === 401) {
    if (!skipGlobalError) {
      triggerUnauthorizedCallbacks()
    }
    const message = await parseErrorMessage(response)
    if (!skipErrorToast && shouldShowUnauthorizedToast()) {
      notifyFromError(message, 'Unauthorized')
    }
    const err = new Error(message)
    ;(err as any).status = 401
    throw err
  }

  // Handle other errors
  if (!response.ok) {
    const message = await parseErrorMessage(response)
    if (!skipErrorToast) {
      notifyFromError(message)
    }
    const err = new Error(message)
    ;(err as any).status = response.status
    throw err
  }

  // Parse JSON response
  const json = await response.json()

  // Auto-unwrap ApiResponse structure if present
  // Backend returns: { success: boolean, data: T, error: null, meta: {...} }
  if (json && typeof json === 'object' && 'success' in json && 'data' in json) {
    if (json.success === true && json.data !== null) {
      // Show success toast if message provided
      if (successMessage) {
        notifySuccess(successMessage)
      }
      return json.data as T
    }
    if (json.success === false && json.error) {
      const errorMsg = typeof json.error === 'object'
        ? json.error.message || json.error.code || 'API Error'
        : json.error
      if (!skipErrorToast) {
        notifyFromError(errorMsg)
      }
      throw new Error(errorMsg)
    }
  }

  // Show success toast for non-wrapped responses if message provided
  if (successMessage && (fetchOptions.method === 'POST' || fetchOptions.method === 'PUT' || fetchOptions.method === 'DELETE')) {
    notifySuccess(successMessage)
  }

  // Return as-is if not an ApiResponse wrapper
  return json as T
}

