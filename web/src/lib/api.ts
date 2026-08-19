// API Client with centralized authentication

// Polyfill for AbortSignal.timeout (for older WebKit/Safari)
if (typeof AbortSignal !== 'undefined' && !AbortSignal.timeout) {
  AbortSignal.timeout = function (ms: number): AbortSignal {
    const controller = new AbortController()
    setTimeout(() => controller.abort(new DOMException('TimeoutError', 'TimeoutError')), ms)
    return controller.signal
  }
}

import type {
  UserInfo,
  LoginResponse,
  RegisterRequest,
  ChangePasswordRequest,
  Device,
  DeviceType,
  Alert,
  AddDeviceRequest,
  MqttStatus,
  ExternalBroker,
  TelemetryDataResponse,
  TelemetrySummaryResponse,
  DeviceCurrentStateResponse,
  BatchCurrentValuesResponse,
  CommandHistoryResponse,
  CommandDto,
  CommandListResponse,
  CommandStatsResponse,
  Rule,
  MemoryEntry,
  Extension,
  ExtensionTypeDto,
  ExtensionLogEntry,
  ExtensionCapabilityDto,
  ExtensionHealthResponse,
  // Unified Extension Types
  ExtensionCommandDescriptor,
  ExtensionExecuteRequest,
  ExtensionExecuteResponse,
  ExtensionDataSourceInfo,
  ExtensionQueryParams,
  ExtensionQueryResult,
  Tool,
  ToolSchema,
  ToolMetrics,
  ToolExecutionResult,
  SearchResult,
  SearchSuggestion,
  Event as NeoMindEvent,
  ChatSession,
  SessionHistoryResponse,
  LlmBackendInstance,
  CreateLlmBackendRequest,
  UpdateLlmBackendRequest,
  LlmBackendListResponse,
  BuiltinLlmStatus,
  BackendTypeDefinition,
  BackendTestResult,
  AdapterType,
  AlertChannel,
  ChannelListResponse,
  ChannelStats,
  ChannelTypeInfo,
  ChannelTestResult,
  ChannelSchemaResponse,
  CreateChannelRequest,
  // Message Types
  NotificationMessage,
  MessageListResponse,
  MessageStats,
  CreateMessageRequest,
  BulkMessageRequest,
  CleanupMessagesRequest,
  MessageChannel,
  MessageChannelListResponse,
  CreateMessageChannelRequest,
  MessageType,
  ChannelFilter,
  DraftDevice,
  SuggestedDeviceType,
  // AI Agent Types
  AiAgentDetail,
  AgentMemory,
  AgentToolCatalogItem,
  AgentStats,
  AgentExecution,
  AgentExecutionDetail,
  CreateAgentRequest,
  UpdateAgentRequest,
  ExecuteAgentRequest,
  ValidateLlmRequest,
  ValidateLlmResponse,
  AgentListResponse,
  AgentExecutionsResponse,
  ParsedIntent,
  AgentAvailableResources,
  UserMessage,
  // Dashboard Types
  DashboardResponse,
  CreateDashboardRequest,
  UpdateDashboardRequest,
  DashboardTemplateResponse,
  // Data Source Types
  TransformDataSourceInfo,
  UnifiedDataSourceInfo,
  // Memory System Types
  MemorySystemConfig,
} from '@/types'
import type { SkillSummary, SkillDetail } from '@/types/skill'
import type { ReasoningCapabilities } from '@/types/llm-backend'
import { notifyFromError, notifySuccess } from './notify'
import { tokenManager as unifiedTokenManager } from './auth'

// In Tauri, we need to use the full URL since the backend runs on port 9375
// In development/web, we can use relative path

/** Check if running in Tauri desktop environment */
export function isTauriEnv(): boolean {
  return typeof window !== 'undefined' && '__TAURI__' in window
}

// Dynamic URL management — imported from urls.ts (re-exported for convenience)
import { setApiKey as _setApiKey, clearApiKey as _clearApiKey, getApiKey as _getApiKey, resetToDefault as _resetToDefault, buildWsUrl as _buildWsUrl } from './urls'
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

interface FetchOptions extends RequestInit {
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

// ========== IM Bridges API ==========
export interface ImBridge {
  id: string
  platform: string
  status: string
}
export interface ImInvite {
  token: string
  created_at: number
  used: boolean
  bound_chat_id: string | null
  bound_at: number | null
}
export interface ImInviteCreated {
  token: string
  deep_link: string | null
}
export interface ImSession {
  chat_id: string
  bound_agent_id: string
  neo_session_id: string
  last_active: number
  created_at: number
}

// ============================================================================
// API Methods
// ============================================================================

export const api = {
  // ========== Generic HTTP Methods ==========
  /**
   * Generic GET request
   */
  get: <T>(path: string, options: Omit<FetchOptions, 'method' | 'body'> = {}) =>
    fetchAPI<T>(path, { ...options, method: 'GET' }),

  /**
   * Generic POST request
   */
  post: <T>(path: string, body: unknown, options: Omit<FetchOptions, 'method' | 'body'> = {}) =>
    fetchAPI<T>(path, { ...options, method: 'POST', body: JSON.stringify(body) }),

  /**
   * Generic PUT request
   */
  put: <T>(path: string, body: unknown, options: Omit<FetchOptions, 'method' | 'body'> = {}) =>
    fetchAPI<T>(path, { ...options, method: 'PUT', body: JSON.stringify(body) }),

  /**
   * Generic DELETE request
   */
  delete: <T>(path: string, options: Omit<FetchOptions, 'method' | 'body'> = {}) =>
    fetchAPI<T>(path, { ...options, method: 'DELETE' }),

  // ========== Authentication API ==========
  login: (username: string, password: string, rememberMe: boolean = false) =>
    fetchAPI<LoginResponse>('/auth/login', {
      method: 'POST',
      body: JSON.stringify({ username, password }),
      skipAuth: true,
      skipGlobalError: true,
    }).then(res => {
      // Store token
      tokenManager.setToken(res.token, rememberMe)
      return res
    }),
  register: (username: string, password: string) =>
    fetchAPI<LoginResponse>('/auth/register', {
      method: 'POST',
      body: JSON.stringify({ username, password }),
      skipAuth: true,
      skipGlobalError: true,
    }).then(res => {
      // Store token
      tokenManager.setToken(res.token, false)
      return res
    }),
  logout: () =>
    fetchAPI<{ message: string }>('/auth/logout', {
      method: 'POST',
    }).then(res => {
      // Clear token
      tokenManager.clearToken()
      return res
    }),
  getCurrentUser: () =>
    fetchAPI<UserInfo>('/auth/me'),
  changePassword: (req: ChangePasswordRequest) =>
    fetchAPI<{ message: string }>('/auth/change-password', {
      method: 'POST',
      body: JSON.stringify(req),
    }),
  listUsers: () =>
    fetchAPI<{ users: UserInfo[] }>('/users'),
  createUser: (req: RegisterRequest) =>
    fetchAPI<{ user: UserInfo }>('/users', {
      method: 'POST',
      body: JSON.stringify(req),
    }),
  deleteUser: (username: string) =>
    fetchAPI<{ message: string }>(`/users/${username}`, {
      method: 'DELETE',
    }),

  // Devices
  getDevices: () => fetchAPI<{ devices: Device[]; count: number }>('/devices'),
  getDevice: (id: string) => fetchAPI<Device>(`/devices/${id}`),
  getDeviceCurrent: (id: string) => fetchAPI<DeviceCurrentStateResponse>(`/devices/${id}/current`),
  getDevicesCurrentBatch: (deviceIds: string[], signal?: AbortSignal) =>
    fetchAPI<BatchCurrentValuesResponse>('/devices/current-batch', {
      method: 'POST',
      body: JSON.stringify({ device_ids: deviceIds }),
      skipErrorToast: true, // Skip error toast if endpoint not implemented
      signal,
    }),
  addDevice: (req: AddDeviceRequest) =>
    fetchAPI<{ device_id: string; added: boolean }>('/devices', {
      method: 'POST',
      body: JSON.stringify(req),
    }),
  updateDevice: (id: string, req: Partial<AddDeviceRequest>) =>
    fetchAPI<{ device_id: string; updated: boolean }>(`/devices/${id}`, {
      method: 'PUT',
      body: JSON.stringify(req),
    }),
  deleteDevice: (id: string) =>
    fetchAPI<{ device_id: string; deleted: boolean }>(`/devices/${id}`, {
      method: 'DELETE',
    }),
  sendCommand: (deviceId: string, command: string, params: Record<string, unknown> = {}) =>
    fetchAPI<{ device_id: string; command: string; sent: boolean }>(`/devices/${deviceId}/command/${command}`, {
      method: 'POST',
      body: JSON.stringify({ params }),
    }),

  // Device Types
  getDeviceTypes: () => fetchAPI<{ device_types: DeviceType[]; count: number }>('/device-types'),
  getDeviceType: (id: string) => fetchAPI<DeviceType>(`/device-types/${id}`),
  addDeviceType: (definition: DeviceType) =>
    fetchAPI<{ error?: string }>('/device-types', {
      method: 'POST',
      body: JSON.stringify(definition),
    }),
  deleteDeviceType: (id: string) =>
    fetchAPI<{ error?: string }>(`/device-types/${id}`, {
      method: 'DELETE',
    }),
  validateDeviceType: (definition: DeviceType) =>
    fetchAPI<{ valid: boolean; errors?: string[]; warnings?: string[]; message: string }>('/device-types', {
      method: 'PUT',
      body: JSON.stringify(definition),
    }),
  generateMDL: (req: { device_name: string; description?: string; uplink_example: string; downlink_example?: string }) =>
    fetchAPI<DeviceType>('/devices/generate-mdl', {
      method: 'POST',
      body: JSON.stringify(req),
    }),
  generateDeviceTypeFromSamples: (req: {
    device_id?: string
    manufacturer?: string
    samples: Array<{ timestamp: number; data: Record<string, unknown> }>
    min_coverage?: number
    min_confidence?: number
  }) =>
    fetchAPI<{
      id: string
      name: string
      description: string
      category: string
      manufacturer: string
      metrics: Array<{
        name: string
        path: string
        display_name: string
        description: string
        data_type: string
        semantic_type: string
        unit: string | null
        readable: boolean
        writable: boolean
        confidence: number
      }>
      commands: Array<{
        name: string
        display_name: string
        description: string
        parameters: Array<{ name: string; type: string; required: boolean }>
        confidence: number
      }>
      confidence: number
    }>('/device-types/generate-from-samples', {
      method: 'POST',
      body: JSON.stringify(req),
    }),

  // ========== Draft Devices API (Auto-onboarding) ==========
  // List all draft devices discovered through auto-onboarding
  getDraftDevices: () =>
    fetchAPI<{ items: Array<{
      id: string
      device_id: string
      source: string
      status: string
      sample_count: number
      max_samples: number
      generated_type?: {
        device_type: string
        name: string
        description: string
        category: string
        metrics: Array<{
          name: string
          path: string
          semantic_type: string
          display_name: string
          confidence: number
        }>
        confidence: number
        summary: {
          samples_analyzed: number
          fields_discovered: number
          metrics_generated: number
          inferred_category: string
          insights: string[]
          warnings: string[]
          recommendations: string[]
        }
      }
      discovered_at: number
      updated_at: number
      error_message?: string
      user_name?: string
    }>; count: number }>('/devices/drafts'),

  // Get a specific draft device
  getDraftDevice: (deviceId: string) =>
    fetchAPI<{
      id: string
      device_id: string
      source: string
      status: string
      sample_count: number
      max_samples: number
      generated_type?: {
        device_type: string
        name: string
        description: string
        category: string
        metrics: Array<{
          name: string
          path: string
          semantic_type: string
          display_name: string
          confidence: number
        }>
        confidence: number
        summary: {
          samples_analyzed: number
          fields_discovered: number
          metrics_generated: number
          inferred_category: string
          insights: string[]
          warnings: string[]
          recommendations: string[]
        }
      }
      discovered_at: number
      updated_at: number
      error_message?: string
      user_name?: string
    }>(`/devices/drafts/${deviceId}`),

  // Approve a draft device - register it as a real device
  approveDraftDevice: (deviceId: string) =>
    fetchAPI<{
      original_device_id: string
      system_device_id: string
      device_type: string
      recommended_topic: string
      registered: boolean
      message: string
    }>(`/devices/drafts/${deviceId}/approve`, {
      method: 'POST',
    }),

  // Reject a draft device
  rejectDraftDevice: (deviceId: string, request: { reason: string }) =>
    fetchAPI<{ device_id: string; rejected: boolean }>(`/devices/drafts/${deviceId}/reject`, {
      method: 'POST',
      body: JSON.stringify(request),
    }),

  // Trigger manual analysis of a draft device
  triggerDraftAnalysis: (deviceId: string) =>
    fetchAPI<{ device_id: string; analysis_triggered: boolean }>(`/devices/drafts/${deviceId}/analyze`, {
      method: 'POST',
    }),

  // Update draft device (user edits)
  updateDraftDevice: (deviceId: string, request: { name?: string; description?: string }) =>
    fetchAPI<{ device_id: string; updated: boolean }>(`/devices/drafts/${deviceId}`, {
      method: 'PUT',
      body: JSON.stringify(request),
    }),

  // Enhance draft device with LLM (manual trigger for Chinese names, descriptions, units)
  enhanceDraftWithLLM: (deviceId: string) =>
    fetchAPI<DraftDevice>(`/devices/drafts/${deviceId}/enhance`, {
      method: 'POST',
    }),

  // Clean up old draft devices
  cleanupDraftDevices: () =>
    fetchAPI<{ cleaned: number; message: string }>('/devices/drafts/cleanup', {
      method: 'POST',
    }),

  // Get all registered type signatures (for type reuse)
  getTypeSignatures: () =>
    fetchAPI<{ signatures: Record<string, string>; count: string }>('/devices/drafts/type-signatures'),

  // Get suggested device types for a draft device
  suggestDeviceTypes: (deviceId: string) =>
    fetchAPI<{
      suggestions: SuggestedDeviceType[]
      exact_match: string | null
    }>(`/devices/drafts/${deviceId}/suggest-types`),

  // Approve draft device with optional existing type assignment or new type details
  approveDraftDeviceWithType: (
    deviceId: string,
    existingType?: string,
    newTypeInfo?: { device_type: string; name: string; description: string },
    deviceName?: string
  ) => {
    const body: Record<string, unknown> = {}
    if (existingType) {
      body.existing_type = existingType
    }
    if (newTypeInfo) {
      body.new_type = newTypeInfo
    }
    if (deviceName) {
      body.device_name = deviceName
    }
    return fetchAPI<{
      original_device_id: string
      system_device_id: string
      device_type: string
      recommended_topic: string
      registered: boolean
      message: string
    }>(`/devices/drafts/${deviceId}/approve`, {
      method: 'POST',
      body: JSON.stringify(body),
    })
  },

  // ========== Auto-onboarding Configuration ==========
  // Get auto-onboarding configuration (simplified to 3 fields)
  getOnboardConfig: () =>
    fetchAPI<{
      enabled: boolean
      max_samples: number
      draft_retention_secs: number
    }>('/devices/drafts/config'),

  // Update auto-onboarding configuration
  updateOnboardConfig: (config: {
    enabled?: boolean
    max_samples?: number
    draft_retention_secs?: number
  }) =>
    fetchAPI<{ message: string }>('/devices/drafts/config', {
      method: 'PUT',
      body: JSON.stringify(config),
    }),

  // Upload device data for auto-onboarding analysis
  uploadDeviceData: (request: {
    device_id?: string
    source?: string
    data: unknown[]
  }) =>
    fetchAPI<{ message: string }>('/devices/drafts/upload', {
      method: 'POST',
      body: JSON.stringify(request),
    }),

  // Messages (replaces Alerts) - response format: { messages: NotificationMessage[], total: number }
  getMessages: (params?: Record<string, string>) =>
    fetchAPI<{ messages: NotificationMessage[]; total: number }>(
      `/messages${params ? `?${new URLSearchParams(params)}` : ''}`
    ),
  getMessage: (id: string) => fetchAPI<NotificationMessage>(`/messages/${id}`),
  createMessage: (req: {
    category?: string
    title: string
    message: string
    severity?: string
    source?: string
    source_type?: string
    source_id?: string
    tags?: string[]
    message_type?: string
    payload?: Record<string, unknown>
    metadata?: Record<string, unknown>
  }) =>
    fetchAPI<{ id: string; message: string; message_zh: string }>('/messages', {
      method: 'POST',
      body: JSON.stringify({
        category: req.category || 'alert',
        title: req.title,
        message: req.message,
        severity: req.severity || 'info',
        source: req.source || 'api',
        source_type: req.source_type,
        source_id: req.source_id,
        tags: req.tags,
        message_type: req.message_type,
        payload: req.payload,
        metadata: req.metadata,
      }),
    }),
  acknowledgeMessage: (id: string) =>
    fetchAPI<{ acknowledged: boolean; message_id: string }>(`/messages/${id}/acknowledge`, {
      method: 'POST',
    }),
  resolveMessage: (id: string) =>
    fetchAPI<{ resolved: boolean; message_id: string }>(`/messages/${id}/resolve`, {
      method: 'POST',
    }),
  archiveMessage: (id: string) =>
    fetchAPI<{ archived: boolean; message_id: string }>(`/messages/${id}/archive`, {
      method: 'POST',
    }),
  deleteMessage: (id: string) =>
    fetchAPI<{ message: string; message_zh: string }>(`/messages/${id}`, {
      method: 'DELETE',
    }),
  getMessageStats: () => fetchAPI<{ total: number; active: number; by_category: Record<string, number>; by_severity: Record<string, number>; by_status: Record<string, number> }>('/messages/stats'),

  // ========== Message Channels API (replaces Alert Channels) ==========
  listMessageChannels: () => fetchAPI<ChannelListResponse>('/messages/channels'),
  getMessageChannel: (name: string) => fetchAPI<AlertChannel>(`/messages/channels/${encodeURIComponent(name)}`),
  listChannelTypes: () => fetchAPI<{ types: ChannelTypeInfo[]; count: number }>('/messages/channels/types'),
  getChannelSchema: (type: string) =>
    fetchAPI<ChannelSchemaResponse>(`/messages/channels/types/${encodeURIComponent(type)}/schema`),
  createMessageChannel: (req: CreateChannelRequest) =>
    fetchAPI<{ message: string; message_zh: string; channel: AlertChannel }>('/messages/channels', {
      method: 'POST',
      body: JSON.stringify(req),
    }),
  deleteMessageChannel: (name: string) =>
    fetchAPI<{ message: string; message_zh: string; name: string }>(
      `/messages/channels/${encodeURIComponent(name)}`,
      { method: 'DELETE' }
    ),
  updateMessageChannel: (name: string, config: Record<string, unknown>) =>
    fetchAPI<{ message: string; message_zh: string; channel: AlertChannel }>(
      `/messages/channels/${encodeURIComponent(name)}`,
      { method: 'PUT', body: JSON.stringify({ config }) }
    ),
  testMessageChannel: (name: string) =>
    fetchAPI<ChannelTestResult>(`/messages/channels/${encodeURIComponent(name)}/test`, {
      method: 'POST',
    }),
  updateChannelEnabled: (name: string, enabled: boolean) =>
    fetchAPI<{ message: string }>(`/messages/channels/${encodeURIComponent(name)}/enabled`, {
      method: 'PUT',
      body: JSON.stringify({ enabled }),
    }),
  getChannelStats: () => fetchAPI<ChannelStats>('/messages/channels/stats'),
  // Channel Filter API
  getChannelFilter: (name: string) =>
    fetchAPI<ChannelFilter>(`/messages/channels/${encodeURIComponent(name)}/filter`),
  updateChannelFilter: (name: string, filter: ChannelFilter) =>
    fetchAPI<{ message: string; message_zh: string; channel: string; filter: ChannelFilter }>(
      `/messages/channels/${encodeURIComponent(name)}/filter`,
      {
        method: 'PUT',
        body: JSON.stringify(filter),
      }
    ),
  // Recipient management for email channels
  listChannelRecipients: (name: string) =>
    fetchAPI<{ channel: string; recipients: string[]; count: number }>(
      `/messages/channels/${encodeURIComponent(name)}/recipients`
    ),
  addChannelRecipient: (name: string, email: string) =>
    fetchAPI<{ message: string; message_zh: string; channel: string; recipients: string[] }>(
      `/messages/channels/${encodeURIComponent(name)}/recipients`,
      { method: 'POST', body: JSON.stringify({ email }) }
    ),
  removeChannelRecipient: (name: string, email: string) =>
    fetchAPI<{ message: string; message_zh: string; channel: string; recipients: string[] }>(
      `/messages/channels/${encodeURIComponent(name)}/recipients/${encodeURIComponent(email)}`,
      { method: 'DELETE' }
    ),
  cleanupMessages: (req: { older_than_days: number }) =>
    fetchAPI<{ cleaned: number; message: string; message_zh: string }>('/messages/cleanup', {
      method: 'POST',
      body: JSON.stringify(req),
    }),

  // ========== LLM Backends API ==========
  listLlmBackends: (params?: { type?: string; active_only?: boolean }) =>
    fetchAPI<LlmBackendListResponse>(
      `/llm-backends${params ? `?${new URLSearchParams(
        Object.entries(params).reduce((acc, [key, value]) => {
          if (value !== undefined) acc[key] = String(value)
          return acc
        }, {} as Record<string, string>)
      )}` : ''}`
    ),
  getLlmBackend: (id: string) =>
    fetchAPI<{ backend: LlmBackendInstance }>(`/llm-backends/${id}`),
  createLlmBackend: (backend: CreateLlmBackendRequest) =>
    fetchAPI<{ id: string; message: string }>('/llm-backends', {
      method: 'POST',
      body: JSON.stringify(backend),
    }),
  updateLlmBackend: (id: string, backend: UpdateLlmBackendRequest) =>
    fetchAPI<{ id: string; message: string }>(`/llm-backends/${id}`, {
      method: 'PUT',
      body: JSON.stringify(backend),
    }),
  deleteLlmBackend: (id: string) =>
    fetchAPI<{ message: string }>(`/llm-backends/${id}`, {
      method: 'DELETE',
    }),
  activateLlmBackend: (id: string) =>
    fetchAPI<{ id: string; message: string }>(`/llm-backends/${id}/activate`, {
      method: 'POST',
    }),
  testLlmBackend: (id: string) =>
    fetchAPI<{ backend_id: string; result: BackendTestResult }>(`/llm-backends/${id}/test`, {
      method: 'POST',
    }),
  listLlmBackendTypes: () =>
    fetchAPI<{ types: BackendTypeDefinition[] }>('/llm-backends/types'),
  getLlmBackendSchema: (backendType: string) =>
    fetchAPI<{ backend_type: string; schema: Record<string, unknown> }>(`/llm-backends/types/${backendType}/schema`),
  getLlmBackendStats: () =>
    fetchAPI<{ total_backends: number; active_backends: number; by_type: Record<string, number> }>('/llm-backends/stats'),
  /**
   * Fetch available models from an Ollama server
   * GET /api/llm-backends/ollama/models?endpoint=http://localhost:11434
   */
  listOllamaModels: (endpoint?: string) =>
    fetchAPI<{
      models: Array<{
        name: string
        size?: number
        modified_at?: string
        digest?: string
        details?: {
          format?: string
          family?: string
          families?: string[]
          parameter_size?: string
          quantization_level?: string
        }
        supports_multimodal: boolean
        supports_thinking: boolean
        supports_tools: boolean
        max_context: number
        reasoning?: ReasoningCapabilities
      }>
      count: number
    }>(`/llm-backends/ollama/models${endpoint ? `?endpoint=${encodeURIComponent(endpoint)}` : ''}`),

  /**
   * Fetch server info from a llama.cpp server
   * GET /api/llm-backends/llamacpp/server-info?endpoint=http://127.0.0.1:8080
   */
  listLlamaCppServerInfo: (endpoint?: string, apiKey?: string) => {
    const params = new URLSearchParams()
    if (endpoint) params.set('endpoint', endpoint)
    if (apiKey) params.set('api_key', apiKey)
    const qs = params.toString()
    return fetchAPI<{
      status: string
      health: { status: string; latency_ms: number }
      server: {
        model_name?: string
        n_ctx?: number
        total_slots?: number
        version?: string
      }
      capabilities: {
        supports_streaming: boolean
        supports_multimodal: boolean
        supports_thinking: boolean
        supports_tools: boolean
        max_context: number
      }
    }>(`/llm-backends/llamacpp/server-info${qs ? `?${qs}` : ''}`)
  },

  /**
   * Set or clear the user override on a backend's multimodal/vision capability.
   * PATCH /api/llm-backends/:id/capabilities
   *
   * - `multimodal: true/false` pins the value, auto-detection is skipped.
   * - `multimodal: null` clears the override, backend re-runs layered detection.
   */
  updateLlmBackendCapabilitiesOverride: (id: string, body: { multimodal: boolean | null }) =>
    fetchAPI<{
      id: string
      supports_multimodal: boolean
      multimodal_user_override: boolean | null
      multimodal_source: string | null
      message: string
    }>(`/llm-backends/${id}/capabilities`, {
      method: 'PATCH',
      body: JSON.stringify(body),
      // Caller handles error toast via useErrorHandler — skip fetchAPI's
      // automatic notifyFromError to avoid double-toasting on PATCH failure.
      skipErrorToast: true,
    }),

  // ========== Builtin LLM API (bundled LFM2.5-2.6B) ==========
  /**
   * Status of the built-in bundled model + server.
   * GET /api/builtin-llm/status
   */
  getBuiltinLlmStatus: () =>
    fetchAPI<BuiltinLlmStatus>('/builtin-llm/status'),
  /**
   * Start / resume the model download (single-flight on the server).
   * POST /api/builtin-llm/download
   */
  downloadBuiltinLlm: () =>
    fetchAPI<{ started: boolean; already_running: boolean }>('/builtin-llm/download', {
      method: 'POST',
    }),
  /**
   * Delete the downloaded model files (stops the server first).
   * DELETE /api/builtin-llm/model
   */
  deleteBuiltinLlmModel: () =>
    fetchAPI<{ deleted: boolean }>('/builtin-llm/model', {
      method: 'DELETE',
    }),
  /**
   * Ensure the bundled llama-server is running (starts it if stopped).
   * POST /api/builtin-llm/restart
   */
  restartBuiltinLlm: () =>
    fetchAPI<{ restarted: boolean; already_running: boolean; endpoint?: string }>(
      '/builtin-llm/restart',
      { method: 'POST' }
    ),
  /**
   * Activate the built-in backend as the active LLM backend.
   * POST /api/builtin-llm/activate
   */
  activateBuiltinLlm: () =>
    fetchAPI<{ id: string; message: string }>('/builtin-llm/activate', {
      method: 'POST',
    }),

  // ========== IM Bridges API ==========
  listImBridges: () => fetchAPI<{ bridges: ImBridge[] }>('/im-bridges'),
  // Mirrors the backend `CreateBridgeRequest` (im_bridges.rs): credential
  // fields are all optional because they are platform-specific — `bot_token`
  // + `api_base` for Telegram, `app_id` + `app_secret` + `domain` for Feishu.
  createImBridge: (req: {
    platform: string
    bot_token?: string
    api_base?: string
    app_id?: string
    app_secret?: string
    domain?: string
  }) => fetchAPI<ImBridge>('/im-bridges', { method: 'POST', body: JSON.stringify(req) }),
  deleteImBridge: (id: string) =>
    fetchAPI<{ id: string; status: string }>(`/im-bridges/${encodeURIComponent(id)}`, { method: 'DELETE' }),
  createImInvite: (id: string) =>
    fetchAPI<ImInviteCreated>(`/im-bridges/${encodeURIComponent(id)}/invites`, { method: 'POST' }),
  listImInvites: (id: string) =>
    fetchAPI<{ invites: ImInvite[] }>(`/im-bridges/${encodeURIComponent(id)}/invites`),
  revokeImInvite: (id: string, token: string) =>
    fetchAPI(`/im-bridges/${encodeURIComponent(id)}/invites/${encodeURIComponent(token)}`, { method: 'DELETE' }),
  listImAllowlist: (id: string) =>
    fetchAPI<{ allowlist: string[] }>(`/im-bridges/${encodeURIComponent(id)}/allowlist`),
  removeImAllowed: (id: string, chat_id: string) =>
    fetchAPI(`/im-bridges/${encodeURIComponent(id)}/allowlist/${encodeURIComponent(chat_id)}`, { method: 'DELETE' }),
  listImSessions: (id: string) =>
    fetchAPI<{ sessions: ImSession[] }>(`/im-bridges/${encodeURIComponent(id)}/sessions`),
  resetImSession: (id: string, chat_id: string) =>
    fetchAPI(`/im-bridges/${encodeURIComponent(id)}/sessions/${encodeURIComponent(chat_id)}/reset`, { method: 'POST' }),

  // ========== MQTT / Brokers API ==========
  // Used by UnifiedDeviceConnectionsTab to display connection status

  getMqttStatus: () => fetchAPI<{ status: MqttStatus }>('/mqtt/status'),

  getBrokers: () => fetchAPI<{ brokers: ExternalBroker[]; count: number }>('/brokers'),
  getBroker: (id: string) => fetchAPI<{ broker: ExternalBroker }>(`/brokers/${id}`),
  createBroker: (broker: Omit<ExternalBroker, 'id' | 'updated_at' | 'connected' | 'last_error'> & { id?: string }) =>
    fetchAPI<{ broker: ExternalBroker; message?: string }>('/brokers', {
      method: 'POST',
      body: JSON.stringify(broker),
    }),
  updateBroker: (id: string, broker: Omit<ExternalBroker, 'id' | 'updated_at' | 'connected' | 'last_error'>) =>
    fetchAPI<{ broker: ExternalBroker; message?: string }>(`/brokers/${id}`, {
      method: 'PUT',
      body: JSON.stringify(broker),
    }),
  deleteBroker: (id: string) =>
    fetchAPI<{ message?: string }>(`/brokers/${id}`, {
      method: 'DELETE',
    }),
  testBroker: (id: string) =>
    fetchAPI<{ success: boolean; message?: string; broker_url?: string; broker?: ExternalBroker }>(`/brokers/${id}/test`, {
      method: 'POST',
    }),

  // Embedded Broker Config
  getEmbeddedBrokerConfig: () =>
    fetchAPI<{
      config: {
        listen: string
        port: number
        max_connections: number
        auth_enabled: boolean
        credentials: { username: string; password: string }[]
        tls_enabled: boolean
        tls_cert_path: string | null
        tls_key_path: string | null
        tls_ca_path: string | null
      }
    }>('/mqtt/broker-config').then((res) => res.config),

  updateEmbeddedBrokerConfig: (config: {
    listen?: string
    port?: number
    auth_enabled?: boolean
    tls_enabled?: boolean
  }) => fetchAPI<{ message: string; restart_required?: boolean }>('/mqtt/broker-config', {
    method: 'PUT',
    body: JSON.stringify(config),
  }),

  addMqttCredential: (username: string, password: string) =>
    fetchAPI<{ message: string }>('/mqtt/broker-config/credentials', {
      method: 'POST',
      body: JSON.stringify({ username, password }),
    }),

  deleteMqttCredential: (username: string) =>
    fetchAPI<{ message: string }>('/mqtt/broker-config/credentials/delete', {
      method: 'POST',
      body: JSON.stringify({ username }),
    }),

  uploadMqttTlsCert: (certPem: string, keyPem: string, caPem?: string) =>
    fetchAPI<{ message: string }>('/mqtt/broker-config/tls', {
      method: 'PUT',
      body: JSON.stringify({ cert_pem: certPem, key_pem: keyPem, ca_pem: caPem }),
    }),

  generateMqttTlsCert: () =>
    fetchAPI<{ message: string; ca_path: string }>('/mqtt/broker-config/tls/generate', {
      method: 'POST',
    }),

  downloadMqttCaCert: async () => {
    const base = getApiBase()
    const headers: Record<string, string> = {}
    const token = tokenManager.getToken()
    if (token) {
      headers['Authorization'] = `Bearer ${token}`
    }
    const apiKey = getApiKey()
    if (apiKey) {
      headers['X-API-Key'] = apiKey
    }
    const response = await fetch(`${base}/mqtt/broker-config/tls/ca-cert`, { headers })
    if (!response.ok) {
      // Parse structured error from response body
      try {
        const body = await response.json()
        const message = body?.error?.message || body?.message || `Download failed (${response.status})`
        throw new Error(message)
      } catch (e) {
        if (e instanceof Error) throw e
        throw new Error(`Failed to download CA certificate (${response.status})`)
      }
    }
    const blob = await response.blob()
    const url = window.URL.createObjectURL(blob)
    const a = document.createElement('a')
    a.href = url
    a.download = 'mqtt-ca.crt'
    document.body.appendChild(a)
    a.click()
    document.body.removeChild(a)
    window.URL.revokeObjectURL(url)
  },

  // Sessions
  // Note: Backend returns paginated response with data as array (auto-unwrapped by fetchAPI)
  listSessions: (page = 1, pageSize = 10) =>
    fetchAPI<ChatSession[]>(`/sessions?page=${page}&page_size=${pageSize}`),
  createSession: (sessionConfig?: { systemPromptSuffix?: string; allowedTools?: string[] }) =>
    fetchAPI<{ sessionId: string }>('/sessions', {
      method: 'POST',
      body: sessionConfig ? JSON.stringify({ sessionConfig }) : undefined,
    }),
  getSession: (id: string) => fetchAPI<{ sessionId: string; state: { id: string; created_at: number; last_activity: number; message_count: number } }>(`/sessions/${id}`),
  updateSession: (id: string, title?: string) =>
    fetchAPI<{ sessionId: string; updated: boolean }>(`/sessions/${id}`, {
      method: 'PUT',
      body: JSON.stringify({ title }),
    }),
  getSessionHistory: (id: string, options?: FetchOptions) => fetchAPI<SessionHistoryResponse>(`/sessions/${id}/history`, options),
  deleteSession: (id: string) =>
    fetchAPI<{ deleted: boolean; sessionId: string }>(`/sessions/${id}`, {
      method: 'DELETE',
    }),

  // Pending stream recovery (for WebSocket reconnection)
  getPendingStream: (id: string) =>
    fetchAPI<{ hasPending: boolean; sessionId: string; userMessage?: string; content?: string; thinking?: string; stage?: string; elapsed?: number; startedAt?: number }>(`/sessions/${id}/pending`),
  clearPendingStream: (id: string) =>
    fetchAPI<{ cleared: boolean; sessionId: string }>(`/sessions/${id}/pending`, {
      method: 'DELETE',
    }),

  // Device Telemetry
  getDeviceTelemetry: (deviceId: string, metric?: string, start?: number, end?: number, limit?: number, offset?: number, bucketed?: boolean) =>
    fetchAPI<TelemetryDataResponse>(
      `/devices/${deviceId}/telemetry?${new URLSearchParams({
        ...(metric && { metric }),
        ...(start && { start: start.toString() }),
        ...(end && { end: end.toString() }),
        ...(limit && { limit: limit.toString() }),
        ...(offset !== undefined && offset > 0 && { offset: offset.toString() }),
        ...(bucketed && { bucketed: 'true' }),
      })}`
    ),
  getDeviceTelemetrySummary: (deviceId: string, hours?: number) =>
    fetchAPI<TelemetrySummaryResponse>(
      `/devices/${deviceId}/telemetry/summary${hours ? `?hours=${hours}` : ''}`
    ),
  getDeviceCommandHistory: (deviceId: string, limit?: number) =>
    fetchAPI<CommandHistoryResponse>(
      `/devices/${deviceId}/commands${limit ? `?limit=${limit}` : ''}`
    ),

  // ========== Stats API ==========
  getSystemStats: () => fetchAPI<{ version: string; uptime: number; platform: string; arch: string; cpu_count: number; total_memory: number; used_memory: number; free_memory: number; available_memory: number; cpu_usage: number; gpus: Array<{ name: string; vendor: string; total_memory_mb: number | null; driver_version: string | null }>; disks: Array<{ name: string; mount: string; total: number; used: number; available: number }>; networks: Array<{ name: string; ip: string; mac: string; rx_bytes: number; tx_bytes: number }> }>('/stats/system'),
  getRuleStats: () => fetchAPI<{ stats: { total_rules: number; enabled_rules: number; disabled_rules: number; by_type: Record<string, number> } }>('/stats/rules'),

  /**
   * Download a ZIP archive of `neomind.log.*` files for diagnostic / support
   * flows. Triggers a browser download via blob URL.
   *
   * @param days Optional time-window filter. `1` = today only, `7` = last 7
   *   days, `30` = last 30 days. `undefined`/`0` = all time (default).
   * @returns the filename suggested by the server, or a fallback default.
   */
  downloadLogs: async (days?: number): Promise<{ filename: string; bytes: number }> => {
    const headers: Record<string, string> = {}
    const token = tokenManager.getToken()
    if (token) headers['Authorization'] = `Bearer ${token}`
    const apiKey = getApiKey()
    if (apiKey) headers['X-API-Key'] = apiKey

    const query = days && days > 0 ? `?days=${encodeURIComponent(days)}` : ''
    const response = await fetch(`${getApiBase()}/logs/download${query}`, { headers })
    if (!response.ok) {
      // Parse structured error JSON instead of dumping raw body to the toast —
      // matches `downloadMqttCaCert` pattern and avoids leaking backend
      // internals (file paths, stack traces) into user-visible notifications.
      try {
        const body = await response.json()
        const message =
          body?.error?.message || body?.message || `Failed to download logs (${response.status})`
        throw new Error(message)
      } catch (e) {
        if (e instanceof Error) throw e
        throw new Error(`Failed to download logs (${response.status})`)
      }
    }

    // Parse filename from Content-Disposition; fall back to timestamped default.
    const cd = response.headers.get('content-disposition') || ''
    const match = cd.match(/filename="?([^";]+)"?/i)
    const filename = match?.[1] || `neomind-logs-${new Date().toISOString().slice(0, 10)}.zip`

    const blob = await response.blob()
    // Trigger browser download via object URL.
    const url = URL.createObjectURL(blob)
    const a = document.createElement('a')
    a.href = url
    a.download = filename
    document.body.appendChild(a)
    a.click()
    a.remove()
    // Defer revoke so Safari has time to start the download.
    setTimeout(() => URL.revokeObjectURL(url), 1000)

    return { filename, bytes: blob.size }
  },

  // ========== Rules API ==========
  listRules: (params?: {
    enabled?: boolean
    limit?: number
    offset?: number
  }) =>
    fetchAPI<{ rules: Array<Rule>; count: number }>(
      `/rules${params ? `?${new URLSearchParams(
        Object.entries(params).reduce((acc, [key, value]) => {
          if (value !== undefined) acc[key] = String(value)
          return acc
        }, {} as Record<string, string>)
      )}` : ''}`
    ),
  getRule: (id: string) => fetchAPI<{ rule: Rule }>(`/rules/${id}`),
  createRule: (rule: Omit<Rule, 'id' | 'created_at' | 'updated_at'>) =>
    fetchAPI<{ rule: Rule; message?: string }>('/rules', {
      method: 'POST',
      body: JSON.stringify(rule),
    }),
  updateRule: (id: string, rule: Partial<Rule>) =>
    fetchAPI<{ rule: Rule; message?: string }>(`/rules/${id}`, {
      method: 'PUT',
      body: JSON.stringify(rule),
    }),
  deleteRule: (id: string) =>
    fetchAPI<{ message: string }>(`/rules/${id}`, {
      method: 'DELETE',
    }),
  enableRule: (id: string) =>
    fetchAPI<{ message: string }>(`/rules/${id}/enable`, {
      method: 'POST',
      body: JSON.stringify({ enabled: true }),
    }),
  disableRule: (id: string) =>
    fetchAPI<{ message: string }>(`/rules/${id}/enable`, {
      method: 'POST',
      body: JSON.stringify({ enabled: false }),
    }),
  testRule: (id: string, execute = false) =>
    fetchAPI<{ result: unknown; message?: string }>(`/rules/${id}/test${execute ? '?execute=true' : ''}`, {
      method: 'POST',
    }),
  validateRule: (rule: { name: string; trigger?: Record<string, unknown>; condition?: Record<string, unknown>; actions?: Array<Record<string, unknown>> }) =>
    fetchAPI<{ valid: boolean; errors?: string[]; parsed?: unknown }>('/rules/validate', {
      method: 'POST',
      body: JSON.stringify(rule),
    }),
  getRuleResources: () =>
    fetchAPI<{
      devices: Array<{
        id: string
        name: string
        device_type: string
        metrics: Array<{ name: string; data_type: string; unit?: string | null; min_value?: number | null; max_value?: number | null }>
        commands: Array<{ name: string; description: string }>
        properties: unknown[]
        online: boolean
      }>
      alert_channels: Array<{ id: string; name: string; channel_type: string; enabled: boolean }>
    }>('/rules/resources'),

  exportRules: (format?: 'json') =>
    fetchAPI<{ rules: unknown[]; export_date: string; total_count: number }>(`/rules/export${format ? `?format=${format}` : ''}`),
  importRules: (rules: unknown[]) =>
    fetchAPI<{ imported: number; skipped: number; errors: Array<{ rule: { name: string }; error: string }> }>('/rules/import', {
      method: 'POST',
      body: JSON.stringify({ rules }),
    }),
  getRuleHistory: (id: string) =>
    fetchAPI<{ rule_id: string; executions: Array<{
      rule_id: string
      rule_name: string
      success: boolean
      actions_executed: string[]
      error: string | null
      duration_ms: number
      triggered_at: string
    }> }>(`/rules/${id}/history`),

  // ========== Unified Automations API ==========
  // Matches backend: crates/api/src/handlers/automations.rs
  listAutomations: (params?: {
    type?: 'transform' | 'all'
    enabled?: boolean
    search?: string
  }) =>
    fetchAPI<{ automations: Array<import('@/types').Automation>; count: number }>(
      `/automations${params ? `?${new URLSearchParams(
        Object.entries(params).reduce((acc, [key, value]) => {
          if (value !== undefined) acc[key] = String(value)
          return acc
        }, {} as Record<string, string>)
      )}` : ''}`
    ),
  getAutomation: (id: string) =>
    fetchAPI<{ automation: import('@/types').Automation; definition: unknown }>(`/automations/${id}`),
  createAutomation: (req: {
    name: string
    description?: string
    type?: 'transform'
    enabled?: boolean
    definition: unknown
  }) =>
    fetchAPI<{ automation: import('@/types').Automation; message: string }>('/automations', {
      method: 'POST',
      body: JSON.stringify(req),
    }),
  updateAutomation: (id: string, req: {
    name?: string
    description?: string
    definition?: unknown
    enabled?: boolean
  }) =>
    fetchAPI<{ automation: import('@/types').Automation; message: string }>(`/automations/${id}`, {
      method: 'PUT',
      body: JSON.stringify(req),
    }),
  deleteAutomation: (id: string) =>
    fetchAPI<{ message: string }>(`/automations/${id}`, {
      method: 'DELETE',
    }),
  setAutomationStatus: (id: string, enabled: boolean) =>
    fetchAPI<{ message: string; enabled: boolean }>(`/automations/${id}/enable`, {
      method: 'POST',
      body: JSON.stringify({ enabled }),
    }),
  analyzeAutomationIntent: (description: string) =>
    fetchAPI<import('@/types').IntentResult>('/automations/analyze-intent', {
      method: 'POST',
      body: JSON.stringify({ description }),
    }),
  getAutomationExecutions: (id: string, limit?: number) =>
    fetchAPI<{ automation_id: string; executions: unknown[]; count: number }>(
      `/automations/${id}/executions${limit ? `?limit=${limit}` : ''}`
    ),
  listAutomationTemplates: () =>
    fetchAPI<{ templates: unknown[]; count: number }>('/automations/templates'),

  // ========== Transform API (Data Processing) ==========
  // Process device data through transforms
  processTransformData: (req: {
    device_id: string
    device_type?: string
    data: unknown
    timestamp?: number
  }) =>
    fetchAPI<{
      success: boolean
      metrics: Array<{
        device_id: string
        metric: string
        value: number
        timestamp: number
        quality: number | null
      }>
      count: number
      warnings: string[]
    }>('/automations/transforms/process', {
      method: 'POST',
      body: JSON.stringify(req),
    }),
  // Test a specific transform with sample data
  testTransform: (id: string, req: {
    device_id: string
    device_type?: string
    data: unknown
    timestamp?: number
  }) =>
    fetchAPI<{
      transform_id: string
      metrics: Array<{
        device_id: string
        metric: string
        value: number
        timestamp: number
        quality: number | null
      }>
      count: number
      warnings: string[]
    }>(`/automations/transforms/${id}/test`, {
      method: 'POST',
      body: JSON.stringify(req),
    }),
  // Test transform code directly without saving
  testTransformCode: (req: {
    code: string
    input_data: unknown
    output_prefix?: string
  }) =>
    fetchAPI<{
      success: boolean
      output: Record<string, unknown>
      output_with_prefix: Record<string, unknown>
      metrics: Array<{
        device_id: string
        metric: string
        value: number
        timestamp: number
        quality: number | null
      }>
      count: number
      error?: string
    }>('/automations/transforms/test-code', {
      method: 'POST',
      body: JSON.stringify(req),
    }),
  // List all transforms
  listTransforms: () =>
    fetchAPI<{ transforms: Array<import('@/types').TransformAutomation>; count: number }>('/automations/transforms'),
  // List all virtual metrics generated by transforms
  listVirtualMetrics: () =>
    fetchAPI<{ metrics: Array<{ device_id: string; metric: string; transform_id: string }>; count: number }>('/automations/transforms/metrics'),

  exportAutomations: () =>
    fetchAPI<{ automations: unknown[]; count: number; exported_at: string }>('/automations/export'),
  importAutomations: (automations: unknown[]) =>
    fetchAPI<{ message: string; imported: number; failed: number }>('/automations/import', {
      method: 'POST',
      body: JSON.stringify({ automations }),
    }),

  // ========== Memory API ==========
  getShortTermMemory: (limit?: number) =>
    fetchAPI<{ memories: Array<MemoryEntry> }>(
      `/memory/short-term${limit ? `?limit=${limit}` : ''}`
    ),
  addShortTermMemory: (content: string, importance?: number) =>
    fetchAPI<{ memory: MemoryEntry; message: string }>('/memory/short-term', {
      method: 'POST',
      body: JSON.stringify({ content, importance }),
    }),
  getMidTermMemory: (limit?: number) =>
    fetchAPI<{ memories: Array<MemoryEntry> }>(
      `/memory/mid-term${limit ? `?limit=${limit}` : ''}`
    ),
  searchMemory: (query: string, limit?: number) =>
    fetchAPI<{ memories: Array<MemoryEntry> }>(
      `/memory/search${limit ? `?limit=${limit}` : ''}`,
      {
        method: 'POST',
        body: JSON.stringify({ query }),
      }
    ),
  consolidateMemory: () =>
    fetchAPI<{ consolidated_count: number; message: string }>('/memory/consolidate', {
      method: 'POST',
    }),

  // ========== Extensions API ==========
  // Matches backend: crates/api/src/handlers/extensions.rs
  //
  // Extension system replaces the legacy Plugin system for dynamically loaded code.

  /**
   * List available extension types
   * GET /api/extensions/types
   */
  listExtensionTypes: () =>
    fetchAPI<ExtensionTypeDto[]>('/extensions/types'),

  /**
   * Get extension logs
   * GET /api/extensions/:id/logs
   */
  getExtensionLogs: (id: string) =>
    fetchAPI<ExtensionLogEntry[]>(`/extensions/${id}/logs`),

  /**
   * Clear extension logs
   * DELETE /api/extensions/:id/logs
   */
  clearExtensionLogs: (id: string) =>
    fetchAPI<{ message: string }>(`/extensions/${id}/logs`, { method: 'DELETE' }),

  /**
   * Unregister an extension
   * DELETE /api/extensions/:id
   */
  unregisterExtension: (id: string) =>
    fetchAPI<{ message: string; extension_id: string }>(`/extensions/${id}`, {
      method: 'DELETE',
    }),

  /**
   * Start an extension
   * POST /api/extensions/:id/start
   */
  startExtension: (id: string) =>
    fetchAPI<{ message: string; extension_id: string }>(`/extensions/${id}/start`, {
      method: 'POST',
    }),

  /**
   * Stop an extension
   * POST /api/extensions/:id/stop
   */
  stopExtension: (id: string) =>
    fetchAPI<{ message: string; extension_id: string }>(`/extensions/${id}/stop`, {
      method: 'POST',
    }),

  /**
   * Upload and install an extension package (.nep file)
   * POST /api/extensions/upload/file
   *
   * Uses JSON with base64-encoded file data (required by backend)
   * @param file - The .nep file to upload
   * @param signal - Optional AbortSignal for timeout/cancellation
   */
  uploadExtensionFile: async (file: File, signal?: AbortSignal): Promise<{
    extension_id: string
    name: string
    version: string
    message: string
  }> => {
    // Read file and convert to base64 using a reliable method
    const arrayBuffer = await file.arrayBuffer()
    const bytes = new Uint8Array(arrayBuffer)
    let binary = ''
    // Process in chunks to avoid call stack overflow for large files
    const chunkSize = 8192
    for (let i = 0; i < bytes.length; i += chunkSize) {
      const chunk = bytes.subarray(i, Math.min(i + chunkSize, bytes.length))
      binary += String.fromCharCode.apply(null, Array.from(chunk))
    }
    const base64Data = btoa(binary)

    // For large file uploads, use the dynamic API base (respects instance switching)
    const uploadApiBase = getApiBase()

    // Get auth token
    const token = tokenManager.getToken()
    const headers: Record<string, string> = {
      'Content-Type': 'application/json',
    }
    if (token) {
      headers['Authorization'] = `Bearer ${token}`
    }

    const response = await fetch(`${uploadApiBase}/extensions/upload/file`, {
      method: 'POST',
      headers,
      body: JSON.stringify({
        data: base64Data,
        filename: file.name,
      }),
      signal,
    })

    // Handle error responses
    if (!response.ok) {
      const text = await response.text()
      let errorMessage = `Upload failed: ${response.status}`
      try {
        const json = JSON.parse(text)
        if (json.message) {
          errorMessage = json.message
        }
      } catch {
        if (text) errorMessage = text
      }
      throw new Error(errorMessage)
    }

    return response.json()
  },

  /**
   * Reload an extension from file
   * POST /api/extensions/:id/reload
   */
  reloadExtension: (id: string) =>
    fetchAPI<{ message: string; extension_id: string; config_applied: boolean }>(`/extensions/${id}/reload`, {
      method: 'POST',
    }),

  /**
   * Toggle an extension's master tool-enable flag.
   * PATCH /api/extensions/:id/enabled
   * `enabled=false` hides ALL of this extension's tools from the LLM.
   */
  setExtensionEnabled: (id: string, enabled: boolean) =>
    fetchAPI<{ id: string; enabled: boolean }>(`/extensions/${id}/enabled`, {
      method: 'PATCH',
      body: JSON.stringify({ enabled }),
    }),

  /**
   * Toggle a single extension command's tool-enable flag.
   * PATCH /api/extensions/:id/commands/:cmd/enabled
   * Independent of the master `enabled` flag.
   */
  setExtensionCommandEnabled: (id: string, cmd: string, enabled: boolean) =>
    fetchAPI<{ id: string; command: string; enabled: boolean }>(
      `/extensions/${id}/commands/${encodeURIComponent(cmd)}/enabled`,
      {
        method: 'PATCH',
        body: JSON.stringify({ enabled }),
      },
    ),

  /**
   * Check extension health
   * GET /api/extensions/:id/health
   */
  getExtensionHealth: (id: string) =>
    fetchAPI<ExtensionHealthResponse>(`/extensions/${id}/health`),

  /**
   * Execute a command on an extension (legacy endpoint)
   * POST /api/extensions/:id/command
   */
  executeExtensionCommand: (id: string, command: string, args?: Record<string, unknown>, opts?: { skipErrorToast?: boolean }) =>
    fetchAPI<Record<string, unknown>>(`/extensions/${id}/command`, {
      method: 'POST',
      body: JSON.stringify({ command, args }),
      // Background fetches (e.g. dashboard auto-refresh) may invoke commands that
      // fail legitimately (required param not yet configured). Suppress the global
      // error toast there — callers already fall back to queryData/empty data.
      skipErrorToast: opts?.skipErrorToast,
    }),

  /**
   * Get all extension capabilities (for Agent tools, Transform operations, etc.)
   * GET /api/extensions/capabilities
   */
  getExtensionCapabilities: () =>
    fetchAPI<ExtensionCapabilityDto[]>('/extensions/capabilities'),

  /**
   * Get a specific extension's capabilities
   * GET /api/extensions/:id/capabilities
   */
  getExtensionCapabilitiesById: (id: string) =>
    fetchAPI<ExtensionCapabilityDto>(`/extensions/${id}/capabilities`),

  /**
   * Invoke an extension command (newer endpoint with better error handling)
   * POST /api/extensions/:id/invoke
   */
  invokeExtension: (id: string, command: string, args?: Record<string, unknown>) =>
    fetchAPI<Record<string, unknown>>(`/extensions/${id}/invoke`, {
      method: 'POST',
      body: JSON.stringify({ command, args }),
    }),

  // ========== Extension API (unified command-based) ==========

  /**
   * List all extensions with their commands
   * GET /api/extensions
   */
  listExtensions: (params?: {
    state?: string
  }) =>
    fetchAPI<Extension[]>(
      `/extensions${params ? `?${new URLSearchParams(
        Object.entries(params).reduce((acc, [key, value]) => {
          if (value !== undefined) acc[key] = String(value)
          return acc
        }, {} as Record<string, string>)
      )}` : ''}`
    ),

  /**
   * Get a specific extension with its commands
   * GET /api/extensions/:id
   */
  getExtension: (id: string) =>
    fetchAPI<Extension>(`/extensions/${id}`),

  /**
   * Get commands for an extension
   * GET /api/extensions/:id/commands
   */
  listCommands: (id: string) =>
    fetchAPI<ExtensionCommandDescriptor[]>(`/extensions/${id}/commands`),

  /**
   * Execute an extension command
   * POST /api/extensions/:id/command
   */
  executeCommand: (id: string, request: ExtensionExecuteRequest) =>
    fetchAPI<ExtensionExecuteResponse>(`/extensions/${id}/command`, {
      method: 'POST',
      body: JSON.stringify(request),
    }),

  /**
   * Get data sources for an extension
   * GET /api/extensions/:id/data-sources
   */
  listDataSources: (id: string) =>
    fetchAPI<ExtensionDataSourceInfo[]>(`/extensions/${id}/data-sources`),

  /**
   * Get historical data for an extension metric
   * GET /api/extensions/:id/metrics/:metric/data?start=&end=&limit=
   */
  getMetricData: (extensionId: string, metric: string, params?: { start?: number; end?: number; limit?: number }) =>
    fetchAPI<{
      source_id: string
      extension_id: string
      metric: string
      start: number
      end: number
      count: number
      data: Array<{ timestamp: number; value: unknown; quality: string }>
    }>(`/extensions/${extensionId}/metrics/${metric}/data${params ? `?${new URLSearchParams(params as any).toString()}` : ''}`),

  /**
   * Query data from an extension
   * Uses getMetricData to fetch historical data
   */
  queryData: async (params: ExtensionQueryParams): Promise<ExtensionQueryResult> => {
    const { extension_id, command, field: metric, start_time, end_time, limit } = params

    try {
      // Convert milliseconds to seconds for backend (backend expects seconds)
      const start_sec = start_time !== undefined ? Math.floor(start_time / 1000) : undefined
      const end_sec = end_time !== undefined ? Math.floor(end_time / 1000) : undefined

      const result = await fetchAPI<{
        source_id: string
        extension_id: string
        metric: string
        start: number
        end: number
        count: number
        data: Array<{ timestamp: number; value: unknown; quality: string }>
      }>(`/extensions/${extension_id}/metrics/${metric}/data${start_sec !== undefined || end_sec !== undefined || limit !== undefined ? `?${new URLSearchParams({
          ...(start_sec !== undefined && { start: start_sec.toString() }),
          ...(end_sec !== undefined && { end: end_sec.toString() }),
          ...(limit !== undefined && { limit: limit.toString() }),
        } as any).toString()}` : ''}`)

      return {
        source_id: result.source_id,
        data_points: result.data.map(p => ({
          timestamp: p.timestamp,
          value: p.value as any,
        })),
      }
    } catch (err) {
      console.error('[API] Failed to query extension data:', err)
      return {
        source_id: `${params.extension_id}:${params.command}:${params.field}`,
        data_points: [],
      }
    }
  },

  /**
   * Get all extension + transform data sources (for dashboard selectors, rules, etc.)
   * Reuses listUnifiedDataSources internally to avoid duplicate API calls.
   */
  listAllDataSources: async () => {
    try {
      const result = await api.listUnifiedDataSources({ limit: 500, skip_telemetry: 'true' })
      const mapped: Array<ExtensionDataSourceInfo | TransformDataSourceInfo> = []

      for (const ds of result.data) {
        if (ds.source_type === 'extension') {
          mapped.push({
            id: ds.id,
            extension_id: ds.source_name,
            command: '',
            field: ds.field,
            display_name: ds.source_display_name + ': ' + ds.field_display_name,
            data_type: (ds.data_type as any) || 'float',
            unit: ds.unit,
            description: ds.description || ds.field_display_name,
            aggregatable: true,
            default_agg_func: 'last' as const,
          })
        } else if (ds.source_type === 'transform') {
          mapped.push({
            id: ds.id,
            transform_id: ds.source_name,
            transform_name: ds.source_display_name,
            metric_name: ds.field,
            display_name: ds.field_display_name,
            data_type: ds.data_type,
            unit: ds.unit,
            description: ds.description || '',
            last_update: ds.last_update ?? undefined,
          })
        }
      }

      return mapped
    } catch {
      return [] as Array<ExtensionDataSourceInfo | TransformDataSourceInfo>
    }
  },

  /**
   * Get unified data sources from the /api/data/sources endpoint
   * Aggregates all data sources (devices, extensions, transforms, AI metrics) in a single call.
   * Supports server-side filtering and pagination.
   */
  listUnifiedDataSources: (params?: Record<string, string | number>) => {
    const qs = params && Object.keys(params).length > 0
      ? new URLSearchParams(
          Object.entries(params).map(([k, v]) => [k, String(v)])
        ).toString()
      : ''
    return fetchAPI<{ data: UnifiedDataSourceInfo[]; total: number; source_options: [string, string][] }>(qs ? `/data/sources?${qs}` : '/data/sources')
  },

  /**
   * Query telemetry time-series data for any source type
   * GET /api/telemetry?source=...&metric=...&start=...&end=...&limit=...
   */
  queryTelemetry: (source: string, metric: string, start: number, end: number, limit?: number, bucketed?: boolean) => {
    const qs = new URLSearchParams({
      source, metric,
      start: String(start),
      end: String(end),
      ...(limit ? { limit: String(limit) } : {}),
      ...(bucketed ? { bucketed: 'true' } : {}),
    }).toString()
    return fetchAPI<{ source_id: string; data: Array<{ timestamp: number; value: unknown; quality: number | null }>; count: number; total_count?: number }>(`/telemetry?${qs}`)
  },

  // ========== Bulk Operations API ==========
  bulkCreateMessages: (messages: Array<{ title: string; message: string; severity?: string; category?: string }>) =>
    fetchAPI<{ created: number; ids: string[] }>('/bulk/messages', {
      method: 'POST',
      body: JSON.stringify({ messages }),
    }),
  bulkResolveMessages: (ids: string[]) =>
    fetchAPI<{ resolved: number }>('/messages/resolve', {
      method: 'POST',
      body: JSON.stringify({ message_ids: ids }),
    }),
  bulkAcknowledgeMessages: (ids: string[]) =>
    fetchAPI<{ acknowledged: number }>('/messages/acknowledge', {
      method: 'POST',
      body: JSON.stringify({ message_ids: ids }),
    }),
  bulkDeleteMessages: (ids: string[]) =>
    fetchAPI<{ deleted: number }>('/messages/delete', {
      method: 'POST',
      body: JSON.stringify({ message_ids: ids }),
    }),
  bulkDeleteSessions: (ids: string[]) =>
    fetchAPI<{ total: number; succeeded: number; failed: number }>('/bulk/sessions/delete', {
      method: 'POST',
      body: JSON.stringify({ session_ids: ids }),
    }),
  bulkDeleteDevices: (ids: string[]) =>
    fetchAPI<{ deleted?: number; succeeded?: number }>('/bulk/devices/delete', {
      method: 'POST',
      body: JSON.stringify({ device_ids: ids }),
    }),
  bulkDeleteDeviceTypes: (ids: string[]) =>
    fetchAPI<{
      deleted?: number
      succeeded?: number
      failed?: number
      results?: Array<{ success: boolean; id?: string; error?: string }>
    }>('/bulk/device-types/delete', {
      method: 'POST',
      body: JSON.stringify({ type_ids: ids }),
    }),
  bulkDeviceCommand: (deviceIds: string[], command: string, params: Record<string, unknown>) =>
    fetchAPI<{ results: Array<{ device_id: string; success: boolean }> }>('/bulk/devices/command', {
      method: 'POST',
      body: JSON.stringify({ device_ids: deviceIds, command, params }),
    }),

  // ========== Config Import/Export API ==========
  exportConfig: () =>
    fetchAPI<{ config: Record<string, unknown>; exported_at: number }>('/config/export'),
  importConfig: (config: Record<string, unknown>, merge?: boolean) =>
    fetchAPI<{ message: string; imported: number }>('/config/import', {
      method: 'POST',
      body: JSON.stringify({ config, merge }),
    }),
  validateConfig: (config: Record<string, unknown>) =>
    fetchAPI<{ valid: boolean; errors?: string[] }>('/config/validate', {
      method: 'POST',
      body: JSON.stringify(config),
    }),

  // ========== Search API ==========
  globalSearch: (q: string, types?: string[], limit?: number) =>
    fetchAPI<{ results: SearchResult[]; count: number }>(
      `/search?q=${encodeURIComponent(q)}${types ? `&types=${types.join(',')}` : ''}${limit ? `&limit=${limit}` : ''}`
    ),
  getSearchSuggestions: (q: string) =>
    fetchAPI<{ suggestions: SearchSuggestion[] }>(`/search/suggestions?q=${encodeURIComponent(q)}`),

  // ========== AI Agents API ==========
  // Matches backend: crates/api/src/handlers/agents.rs
  //
  // AI Agents are user-defined automation agents that can:
  // - Monitor devices and metrics
  // - Execute commands based on conditions
  // - Maintain persistent memory across executions
  // - Provide transparent decision process recording

  /**
   * List all AI Agents
   * GET /api/agents
   */
  listAgents: () =>
    fetchAPI<AgentListResponse>('/agents'),

  /**
   * List agent summaries (id, name, status only) for dropdowns
   * GET /api/agents?view=summary
   */
  listAgentSummaries: () =>
    fetchAPI<AgentListResponse>('/agents?view=summary'),

  /**
   * Get an AI Agent by ID
   * GET /api/agents/:id
   */
  getAgent: (id: string) =>
    fetchAPI<AiAgentDetail>(`/agents/${id}`),

  /**
   * Create a new AI Agent
   * POST /api/agents
   */
  createAgent: (req: CreateAgentRequest) =>
    fetchAPI<{ id: string; name: string; status: string }>('/agents', {
      method: 'POST',
      body: JSON.stringify(req),
    }),

  /**
   * Update an AI Agent
   * PUT /api/agents/:id
   */
  updateAgent: (id: string, req: UpdateAgentRequest) =>
    fetchAPI<{ id: string }>(`/agents/${id}`, {
      method: 'PUT',
      body: JSON.stringify(req),
    }),

  /**
   * Delete an AI Agent
   * DELETE /api/agents/:id
   */
  deleteAgent: (id: string) =>
    fetchAPI<{ ok: boolean }>(`/agents/${id}`, {
      method: 'DELETE',
    }),

  /**
   * Execute an AI Agent immediately
   * POST /api/agents/:id/execute
   */
  executeAgent: (id: string, req?: ExecuteAgentRequest) =>
    fetchAPI<{ execution_id: string; agent_id: string; status: string }>(`/agents/${id}/execute`, {
      method: 'POST',
      body: JSON.stringify(req || {}),
    }),

  /**
   * Invoke an AI Agent synchronously — waits for completion and returns results
   * POST /api/agents/:id/invoke
   */
  invokeAgent: (id: string, req?: ExecuteAgentRequest) =>
    fetchAPI<{
      execution_id: string
      agent_id: string
      agent_name: string
      status: string
      duration_ms: number
      conclusion: string
      confidence: number
      actions: Array<{ action: string; reasoning: string; description: string }>
      has_error: boolean
      error?: string
    }>(`/agents/${id}/invoke`, {
      method: 'POST',
      body: JSON.stringify(req || {}),
    }),

  /**
   * Update agent status
   * POST /api/agents/:id/status
   */
  setAgentStatus: (id: string, status: string) =>
    fetchAPI<{ id: string; status: string }>(`/agents/${id}/status`, {
      method: 'POST',
      body: JSON.stringify({ status }),
    }),

  /**
   * Get execution history for an agent
   * GET /api/agents/:id/executions
   */
  getAgentExecutions: (id: string, limit = 50) =>
    fetchAPI<AgentExecutionsResponse>(`/agents/${id}/executions?limit=${limit}`),

  /**
   * Get a specific execution record
   * GET /api/agents/:id/executions/:execution_id
   */
  getAgentExecution: (id: string, executionId: string) =>
    fetchAPI<AgentExecutionDetail>(`/agents/${id}/executions/${executionId}`),

  /**
   * Get execution with full details (alias for getAgentExecution)
   * Returns AgentExecutionDetail with decision_process and result
   */
  getExecution: (id: string, executionId: string) =>
    fetchAPI<AgentExecutionDetail>(`/agents/${id}/executions/${executionId}`),

  /**
   * Batch get execution details — eliminates N+1 API calls
   * POST /api/agents/:id/executions/details
   */
  batchGetExecutions: (agentId: string, ids: string[]) =>
    fetchAPI<{ agent_id: string; details: Record<string, AgentExecutionDetail> }>(`/agents/${agentId}/executions/details`, {
      method: 'POST',
      body: JSON.stringify({ ids }),
    }),

  /**
   * Get agent memory
   * GET /api/agents/:id/memory
   */
  getAgentMemory: (id: string) =>
    fetchAPI<AgentMemory>(`/agents/${id}/memory`),

  /**
   * Get available agent tools (read-only catalog of the server's ToolRegistry).
   * GET /api/agents/tools
   */
  getAgentTools: () =>
    fetchAPI<{ tools: AgentToolCatalogItem[]; count: number }>(`/agents/tools`),

  /**
   * Clear agent memory
   * DELETE /api/agents/:id/memory
   */
  clearAgentMemory: (id: string) =>
    fetchAPI<{ ok: boolean }>(`/agents/${id}/memory`, {
      method: 'DELETE',
    }),

  /**
   * Get agent statistics
   * GET /api/agents/:id/stats
   */
  getAgentStats: (id: string) =>
    fetchAPI<AgentStats>(`/agents/${id}/stats`),

  /**
   * Get user messages for an agent
   * GET /api/agents/:id/messages
   */
  getAgentUserMessages: (id: string) =>
    fetchAPI<UserMessage[]>(`/agents/${id}/messages`),

  /**
   * Add a user message to an agent
   * POST /api/agents/:id/messages
   */
  addAgentUserMessage: (id: string, content: string, messageType?: string) =>
    fetchAPI<UserMessage>(`/agents/${id}/messages`, {
      method: 'POST',
      body: JSON.stringify({ content, message_type: messageType }),
    }),

  /**
   * Delete a specific user message
   * DELETE /api/agents/:id/messages/:message_id
   */
  deleteAgentUserMessage: (id: string, messageId: string) =>
    fetchAPI<{ ok: boolean }>(`/agents/${id}/messages/${messageId}`, {
      method: 'DELETE',
    }),

  /**
   * Clear all user messages for an agent
   * DELETE /api/agents/:id/messages
   */
  clearAgentUserMessages: (id: string) =>
    fetchAPI<{ ok: boolean; count: number }>(`/agents/${id}/messages`, {
      method: 'DELETE',
    }),

  /**
   * Get agent available resources (devices, metrics, commands)
   * This helps the AI understand what assets are available in the system
   * GET /api/agents/:id/available-resources
   */
  getAgentAvailableResources: (id: string) =>
    fetchAPI<AgentAvailableResources>(`/agents/${id}/available-resources`),

  /**
   * Parse natural language intent
   * POST /api/agents/parse-intent
   */
  parseAgentIntent: (prompt: string, llmBackendId?: string) =>
    fetchAPI<ParsedIntent>('/agents/parse-intent', {
      method: 'POST',
      body: JSON.stringify({ prompt, llm_backend_id: llmBackendId }),
    }),

  /**
   * Validate LLM backend availability and configuration
   * POST /api/agents/validate-llm
   */
  validateLlmBackend: (req: ValidateLlmRequest) =>
    fetchAPI<ValidateLlmResponse>('/agents/validate-llm', {
      method: 'POST',
      body: JSON.stringify(req),
    }),

  // ==========================================================================
  // Skills APIs
  // ==========================================================================

  listSkills: (page = 1, pageSize = 10) =>
    fetchAPI<{ skills: SkillSummary[]; total: number; page: number; page_size: number; total_pages: number }>(
      `/skills?page=${page}&page_size=${pageSize}`
    ),
  getSkill: (id: string) =>
    fetchAPI<SkillDetail>(`/skills/${id}`),
  createSkill: (content: string) =>
    fetchAPI<{ message: string }>('/skills', {
      method: 'POST',
      body: JSON.stringify({ content }),
    }),
  updateSkill: (id: string, content: string) =>
    fetchAPI<{ message: string }>(`/skills/${id}`, {
      method: 'PUT',
      body: JSON.stringify({ content }),
    }),
  deleteSkill: (id: string) =>
    fetchAPI<{ message: string }>(`/skills/${id}`, {
      method: 'DELETE',
    }),

  // ==========================================================================
  // Dashboard APIs
  // ==========================================================================

  /**
   * List all dashboards
   * GET /api/dashboards
   */
  getDashboards: () =>
    fetchAPI<{ dashboards: DashboardResponse[]; count: number }>('/dashboards'),

  /**
   * Get a dashboard by ID
   * GET /api/dashboards/:id
   */
  getDashboard: (id: string) =>
    fetchAPI<DashboardResponse>(`/dashboards/${id}`),

  /**
   * Create a new dashboard
   * POST /api/dashboards
   */
  createDashboard: (dashboard: CreateDashboardRequest) =>
    fetchAPI<DashboardResponse>('/dashboards', {
      method: 'POST',
      body: JSON.stringify(dashboard),
    }),

  /**
   * Update a dashboard
   * PUT /api/dashboards/:id
   */
  updateDashboard: (id: string, dashboard: UpdateDashboardRequest) =>
    fetchAPI<DashboardResponse>(`/dashboards/${id}`, {
      method: 'PUT',
      body: JSON.stringify(dashboard),
    }),

  /**
   * Delete a dashboard
   * DELETE /api/dashboards/:id
   */
  deleteDashboard: (id: string) =>
    fetchAPI<{ ok: boolean }>(`/dashboards/${id}`, {
      method: 'DELETE',
    }),

  /**
   * Duplicate a dashboard
   * POST /api/dashboards/:id/duplicate
   */
  duplicateDashboard: (id: string) =>
    fetchAPI<DashboardResponse>(`/dashboards/${id}/duplicate`, {
      method: 'POST',
    }),

  /**
   * Reorder dashboards (persist manual sidebar order)
   * PUT /api/dashboards/reorder
   * @param dashboardIds Desired order, index 0 = top of list
   */
  reorderDashboards: (dashboardIds: string[]) =>
    fetchAPI<{ ok: boolean; count: number }>('/dashboards/reorder', {
      method: 'PUT',
      body: JSON.stringify({ dashboard_ids: dashboardIds }),
    }),

  /**
   * Set default dashboard
   * POST /api/dashboards/:id/default
   */
  setDefaultDashboard: (id: string) =>
    fetchAPI<{ id: string }>(`/dashboards/${id}/default`, {
      method: 'POST',
    }),

  /**
   * List dashboard templates
   * GET /api/dashboards/templates
   */
  getDashboardTemplates: () =>
    fetchAPI<DashboardTemplateResponse[]>('/dashboards/templates'),

  /**
   * Get a template by ID
   * GET /api/dashboards/templates/:id
   */
  getDashboardTemplate: (id: string) =>
    fetchAPI<DashboardTemplateResponse>(`/dashboards/templates/${id}`),

  // ==========================================================================
  // Timezone Settings API
  // ==========================================================================

  /**
   * Get the current global timezone setting
   * GET /api/settings/timezone
   */
  getTimezone: () =>
    fetchAPI<{ timezone: string; is_default: boolean }>('/settings/timezone'),

  /**
   * Update the global timezone setting
   * PUT /api/settings/timezone
   */
  updateTimezone: (timezone: string) =>
    fetchAPI<{ success: boolean; timezone: string }>('/settings/timezone', {
      method: 'PUT',
      body: JSON.stringify({ timezone }),
    }),

  /**
   * List available timezone options
   * GET /api/settings/timezones
   */
  listTimezones: () =>
    fetchAPI<{ timezones: Array<{ id: string; name: string }> }>('/settings/timezones'),

  // ==========================================================================
  // System Memory API - File-based (New)
  // ==========================================================================

  /**
   * Get memory file content (user or knowledge)
   * GET /api/memory/file/:target
   */
  getMemoryFile: (target: string) =>
    fetchAPI<{
      success: boolean
      target: string
      content: string
      chars: number
    }>(`/memory/file/${target}`),

  /**
   * Update memory file content (user or knowledge)
   * PUT /api/memory/file/:target
   */
  updateMemoryFile: (target: string, content: string) =>
    fetchAPI<{ success: boolean; message: string; chars: number }>(`/memory/file/${target}`, {
      method: 'PUT',
      body: JSON.stringify({ content }),
    }),

  /**
   * Get all memory statistics
   * GET /api/memory/stats
   */
  getMemoryStats: () =>
    fetchAPI<{
      files: Record<
        string,
        { chars: number; modified_at: number }
      >
      custom_files?: Array<{ name: string; chars: number }>
    }>('/memory/stats'),

  /**
   * Get memory configuration
   * GET /api/memory/config
   */
  getMemoryConfig: () =>
    fetchAPI<MemorySystemConfig>('/memory/config'),

  /**
   * Update memory configuration
   * PUT /api/memory/config
   */
  updateMemoryConfig: (config: Partial<MemorySystemConfig>) =>
    fetchAPI<{ success: boolean; config: MemorySystemConfig }>('/memory/config', {
      method: 'PUT',
      body: JSON.stringify({ config }),
    }),

  /**
   * Trigger manual compression
   * POST /api/memory/compress
   * Note: This operation may take a long time, so we use a 5-minute timeout
   */
  triggerMemoryCompress: () => {
    // Create timeout signal with fallback for older browsers
    let signal: AbortSignal | undefined
    try {
      signal = AbortSignal.timeout(5 * 60 * 1000) // 5 minutes timeout
    } catch (e) {
      console.warn('AbortSignal.timeout not supported, request will have no timeout', e)
      signal = undefined
    }

    return fetchAPI<{
      success: boolean
      compressed: number
      deleted: number
      message: string
    }>('/memory/compress', {
      method: 'POST',
      signal,
    })
  },

  /**
   * Export all memory as Markdown
   * GET /api/memory/export
   */
  exportAllMemory: () =>
    fetchAPI<string>('/memory/export', {
      headers: { Accept: 'text/markdown' },
    }),

  // ==========================================================================
  // Custom Memory Files API
  // ==========================================================================

  /**
   * List all custom memory files
   * GET /api/memory/custom
   */
  listCustomMemoryFiles: () =>
    fetchAPI<{
      success: boolean
      files: Array<{ name: string; chars: number }>
    }>('/memory/custom'),

  /**
   * Get a custom memory file
   * GET /api/memory/custom/:name
   */
  getCustomMemoryFile: (name: string) =>
    fetchAPI<{
      success: boolean
      name: string
      content: string
      chars: number
    }>(`/memory/custom/${name}`),

  /**
   * Create or update a custom memory file
   * PUT /api/memory/custom/:name
   */
  updateCustomMemoryFile: (name: string, content: string) =>
    fetchAPI<{ success: boolean; message: string; chars: number }>(`/memory/custom/${name}`, {
      method: 'PUT',
      body: JSON.stringify({ content }),
    }),

  /**
   * Delete a custom memory file
   * DELETE /api/memory/custom/:name
   */
  deleteCustomMemoryFile: (name: string) =>
    fetchAPI<{ success: boolean; message: string }>(`/memory/custom/${name}`, {
      method: 'DELETE',
    }),

  // ==========================================================================
  // System Memory API - File-based (Legacy)
  // ==========================================================================

  /**
   * List all memory files
   * GET /api/memory
   */
  listMemoryFiles: <T>() => fetchAPI<T>('/memory'),

  /**
   * Get memory file content
   * GET /api/memory/:source_type/:id
   */
  getMemoryContent: (sourceType: string, id: string) =>
    fetchAPI<{ id: string; source_type: string; content: string }>(`/memory/${sourceType}/${id}`),

  /**
   * Update memory file content
   * PUT /api/memory/:source_type/:id
   */
  updateMemoryContent: (sourceType: string, id: string, content: string) =>
    fetchAPI<{ success: boolean; message: string }>(`/memory/${sourceType}/${id}`, {
      method: 'PUT',
      body: JSON.stringify({ content }),
    }),

  /**
   * Delete a memory file
   * DELETE /api/memory/:source_type/:id
   */
  deleteMemoryFile: (sourceType: string, id: string) =>
    fetchAPI<{ success: boolean; message: string }>(`/memory/${sourceType}/${id}`, {
      method: 'DELETE',
    }),

  /**
   * Export all memory as Markdown (Legacy alias)
   * GET /api/memory/export
   */
  exportMemory: () =>
    fetchAPI<string>('/memory/export', {
      headers: { Accept: 'text/markdown' },
    }),

  // ========== Data Push API ==========

  /** List all push targets */
  listPushTargets: () =>
    fetchAPI<{ targets: import('@/types').PushTarget[]; total: number }>('/data-push'),

  /** Get a push target by ID */
  getPushTarget: (id: string) =>
    fetchAPI<import('@/types').PushTarget>(`/data-push/${id}`),

  /** Create a new push target */
  createPushTarget: (data: import('@/types').CreatePushTargetRequest) =>
    fetchAPI<{ id: string; name: string; target_type: string; enabled: boolean }>('/data-push', {
      method: 'POST',
      body: JSON.stringify(data),
    }),

  /** Update a push target */
  updatePushTarget: (id: string, data: import('@/types').UpdatePushTargetRequest) =>
    fetchAPI<{ id: string; name: string; enabled: boolean }>(`/data-push/${id}`, {
      method: 'PUT',
      body: JSON.stringify(data),
    }),

  /** Delete a push target */
  deletePushTarget: (id: string) =>
    fetchAPI<{ message: string }>(`/data-push/${id}`, {
      method: 'DELETE',
    }),

  /** Test a push target */
  testPushTarget: (id: string) =>
    fetchAPI<import('@/types').DeliveryLog>(`/data-push/${id}/test`, {
      method: 'POST',
      body: JSON.stringify({}),
    }),

  /** Start a push target */
  startPushTarget: (id: string) =>
    fetchAPI<{ message: string }>(`/data-push/${id}/start`, {
      method: 'POST',
      body: JSON.stringify({}),
    }),

  /** Stop a push target */
  stopPushTarget: (id: string) =>
    fetchAPI<{ message: string }>(`/data-push/${id}/stop`, {
      method: 'POST',
      body: JSON.stringify({}),
    }),

  /** List delivery logs for a push target */
  listPushDeliveryLogs: (id: string, limit?: number, offset?: number) => {
    const params = new URLSearchParams()
    if (limit) params.set('limit', String(limit))
    if (offset) params.set('offset', String(offset))
    const qs = params.toString() ? `?${params.toString()}` : ''
    return fetchAPI<{ logs: import('@/types').DeliveryLog[]; total: number }>(`/data-push/${id}/logs${qs}`)
  },

  /** Get push statistics */
  getPushStats: () =>
    fetchAPI<import('@/types').PushStats>('/data-push/stats'),
}
