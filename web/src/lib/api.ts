// API Client with centralized authentication
import type {
  UserInfo,
  LoginResponse,
  RegisterRequest,
  ChangePasswordRequest,
  Device,
  DeviceType,
  Alert,
  AddDeviceRequest,
  LlmSettings,
  MqttSettings,
  ExternalBroker,
  MqttStatus,
  HassDiscoveryStatus,
  HassDiscoveryRequest,
  HassDiscoveryResponse,
  HassProcessRequest,
  HassProcessResponse,
  HassDiscoveredDevice,
  TelemetryDataResponse,
  TelemetrySummaryResponse,
  CommandHistoryResponse,
  CommandDto,
  CommandListResponse,
  CommandStatsResponse,
  DecisionDto,
  DecisionListResponse,
  DecisionStatsResponse,
  Rule,
  Workflow,
  WorkflowExecution,
  Scenario,
  ScenarioTemplate,
  MemoryEntry,
  Plugin,
  PluginStatsDto,
  AdapterPluginDto,
  AdapterDeviceDto,
  Tool,
  ToolSchema,
  ToolMetrics,
  ToolExecutionResult,
  SearchResult,
  SearchSuggestion,
  EventStats,
  Event as NeoTalkEvent,
  SessionListResponse,
  SessionHistoryResponse,
  LlmBackendInstance,
  CreateLlmBackendRequest,
  UpdateLlmBackendRequest,
  LlmBackendListResponse,
  BackendTypeDefinition,
  BackendTestResult,
} from '@/types'

const API_BASE = '/api'

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

// ============================================================================
// JWT Token Manager (for user authentication)
// ============================================================================

const TOKEN_KEY = 'neotalk_token'
const TOKEN_KEY_SESSION = 'neotalk_token_session'

export const tokenManager = {
  getToken: (): string | null => {
    return localStorage.getItem(TOKEN_KEY) || sessionStorage.getItem(TOKEN_KEY_SESSION)
  },
  setToken: (token: string, remember: boolean = false): void => {
    if (remember) {
      localStorage.setItem(TOKEN_KEY, token)
      sessionStorage.removeItem(TOKEN_KEY_SESSION)
    } else {
      sessionStorage.setItem(TOKEN_KEY_SESSION, token)
      localStorage.removeItem(TOKEN_KEY)
    }
  },
  clearToken: (): void => {
    localStorage.removeItem(TOKEN_KEY)
    sessionStorage.removeItem(TOKEN_KEY_SESSION)
  },
  hasToken: (): boolean => {
    return !!(localStorage.getItem(TOKEN_KEY) || sessionStorage.getItem(TOKEN_KEY_SESSION))
  },
}

// ============================================================================
// Enhanced Fetch with Auth
// ============================================================================

interface FetchOptions extends RequestInit {
  skipAuth?: boolean
  skipGlobalError?: boolean
}

export async function fetchAPI<T>(
  path: string,
  options: FetchOptions = {}
): Promise<T> {
  const { skipAuth = false, skipGlobalError = false, ...fetchOptions } = options

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
  }

  // Ensure headers is not undefined for headers as Record<string, string>
  const finalHeaders = headers as Record<string, string>

  const response = await fetch(`${API_BASE}${path}`, {
    ...fetchOptions,
    headers: finalHeaders,
  })

  // Handle 401 Unauthorized - trigger callbacks and throw error
  if (response.status === 401) {
    if (!skipGlobalError) {
      triggerUnauthorizedCallbacks()
    }
    // Try to extract error message from response body
    const text = await response.text()
    if (text) {
      try {
        const json = JSON.parse(text)
        throw new Error(json.error || text)
      } catch {
        throw new Error(text)
      }
    }
    throw new Error('UNAUTHORIZED')
  }

  // Handle other errors
  if (!response.ok) {
    const text = await response.text()
    throw new Error(text || `API Error: ${response.status}`)
  }

  // Parse JSON response
  const json = await response.json()

  // Auto-unwrap ApiResponse structure if present
  // Backend returns: { success: boolean, data: T, error: null, meta: {...} }
  if (json && typeof json === 'object' && 'success' in json && 'data' in json) {
    if (json.success === true && json.data !== null) {
      return json.data as T
    }
    if (json.success === false && json.error) {
      throw new Error(json.error.message || json.error.code || 'API Error')
    }
  }

  // Return as-is if not an ApiResponse wrapper
  return json as T
}

// ============================================================================
// API Methods
// ============================================================================

export const api = {
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
  addDevice: (req: AddDeviceRequest) =>
    fetchAPI<{ device_id: string; added: boolean }>('/devices', {
      method: 'POST',
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

  // Device Discovery
  discoverDevices: (host: string, ports?: number[], timeoutMs?: number) =>
    fetchAPI<{
      devices: Array<{
        id: string
        device_type: string | null
        host: string
        port: number
        confidence: number
        info: Record<string, string>
      }>
      count: number
      host: string
    }>('/devices/discover', {
      method: 'POST',
      body: JSON.stringify({ host, ports, timeout_ms: timeoutMs }),
    }),

  // Alerts
  getAlerts: () => fetchAPI<{ alerts: Alert[]; count: number }>('/alerts'),
  getAlert: (id: string) => fetchAPI<Alert>(`/alerts/${id}`),
  createAlert: (req: { title: string; message: string; severity?: string; source?: string }) =>
    fetchAPI<{ id: string; title: string; message: string; severity: string }>('/alerts', {
      method: 'POST',
      body: JSON.stringify(req),
    }),
  acknowledgeAlert: (id: string) =>
    fetchAPI<{ acknowledged: boolean; alertId: string }>(`/alerts/${id}/acknowledge`, {
      method: 'POST',
    }),

  // Settings
  getLlmSettings: () => fetchAPI<LlmSettings>('/settings/llm'),
  updateLlmSettings: (settings: LlmSettings) =>
    fetchAPI<{ error?: string }>('/settings/llm', {
      method: 'POST',
      body: JSON.stringify(settings),
    }),
  testLlm: (settings: LlmSettings) =>
    fetchAPI<{ error?: string }>('/settings/llm/test', {
      method: 'POST',
      body: JSON.stringify(settings),
    }),
  listOllamaModels: () => fetchAPI<{ models: string[]; endpoint: string }>('/settings/llm/models'),

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

  // MQTT Settings
  getMqttSettings: () => fetchAPI<{ settings: MqttSettings }>('/settings/mqtt'),
  updateMqttSettings: (settings: MqttSettings) =>
    fetchAPI<{ message?: string; settings: MqttSettings }>('/settings/mqtt', {
      method: 'POST',
      body: JSON.stringify(settings),
    }),
  getMqttStatus: () => fetchAPI<{ status: MqttStatus }>('/mqtt/status'),

  // External Brokers API
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

  // Sessions
  listSessions: (page = 1, pageSize = 20) =>
    fetchAPI<SessionListResponse>(`/sessions?page=${page}&page_size=${pageSize}`),
  createSession: () =>
    fetchAPI<{ sessionId: string }>('/sessions', {
      method: 'POST',
    }),
  getSession: (id: string) => fetchAPI<{ sessionId: string; state: { id: string; created_at: number; last_activity: number; message_count: number } }>(`/sessions/${id}`),
  getSessionHistory: (id: string) => fetchAPI<SessionHistoryResponse>(`/sessions/${id}/history`),
  deleteSession: (id: string) =>
    fetchAPI<{ deleted: boolean; sessionId: string }>(`/sessions/${id}`, {
      method: 'DELETE',
    }),

  // HASS Discovery
  getHassDiscoveryStatus: () => fetchAPI<HassDiscoveryStatus>('/devices/hass/status'),
  startHassDiscovery: (req: HassDiscoveryRequest) =>
    fetchAPI<HassDiscoveryResponse>('/devices/hass/discover', {
      method: 'POST',
      body: JSON.stringify(req),
    }),
  stopHassDiscovery: () =>
    fetchAPI<{ stopped: boolean }>('/devices/hass/stop', {
      method: 'POST',
    }),
  getHassDiscoveredDevices: () => fetchAPI<{ devices: HassDiscoveredDevice[]; count: number }>('/devices/hass/discovered'),
  clearHassDiscoveredDevices: () =>
    fetchAPI<{ cleared: boolean }>('/devices/hass/discovered', {
      method: 'DELETE',
    }),
  processHassDiscovery: (req: HassProcessRequest) =>
    fetchAPI<HassProcessResponse>('/devices/hass/process', {
      method: 'POST',
      body: JSON.stringify(req),
    }),
  registerAggregatedHassDevice: (deviceId: string) =>
    fetchAPI<{ device_id: string; name?: string; entity_count: number; total_metrics: number; total_commands: number; errors?: string[] }>('/devices/hass/register', {
      method: 'POST',
      body: JSON.stringify({ device_id: deviceId }),
    }),
  unregisterHassDevice: (deviceId: string) =>
    fetchAPI<{ device_id: string; unregistered: boolean }>(`/devices/hass/unregister/${encodeURIComponent(deviceId)}`, {
      method: 'DELETE',
    }),

  // HASS Integration API (for REST API connection)
  connectHass: (req: { url: string; token: string; verify_ssl?: boolean; auto_import?: boolean }) =>
    fetchAPI<{ settings: { enabled: boolean; url: string; auto_import: boolean } }>('/integration/hass/connect', {
      method: 'POST',
      body: JSON.stringify(req),
    }),
  disconnectHass: () =>
    fetchAPI<{ disconnected: boolean }>('/integration/hass/disconnect', {
      method: 'DELETE',
    }),
  getHassStatus: () =>
    fetchAPI<{ status: { enabled: boolean; connected: boolean; url: string; last_sync?: number; entity_count: number } }>('/integration/hass'),

  // Device Telemetry
  getDeviceTelemetry: (deviceId: string, metric?: string, start?: number, end?: number, limit?: number) =>
    fetchAPI<TelemetryDataResponse>(
      `/devices/${deviceId}/telemetry?${new URLSearchParams({
        ...(metric && { metric }),
        ...(start && { start: start.toString() }),
        ...(end && { end: end.toString() }),
        ...(limit && { limit: limit.toString() }),
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

  // ========== Commands API ==========
  listCommands: (params?: {
    device_id?: string
    status?: string
    source?: string
    limit?: number
    offset?: number
  }) =>
    fetchAPI<CommandListResponse>(
      `/commands${params ? `?${new URLSearchParams(
        Object.entries(params).reduce((acc, [key, value]) => {
          if (value !== undefined) acc[key] = String(value)
          return acc
        }, {} as Record<string, string>)
      )}` : ''}`
    ),
  getCommand: (id: string) => fetchAPI<{ command: CommandDto }>(`/commands/${id}`),
  retryCommand: (id: string) =>
    fetchAPI<{ message: string; command_id: string }>(`/commands/${id}/retry`, {
      method: 'POST',
    }),
  cancelCommand: (id: string) =>
    fetchAPI<{ message: string; command_id: string }>(`/commands/${id}/cancel`, {
      method: 'POST',
    }),
  getCommandStats: () => fetchAPI<CommandStatsResponse>('/commands/stats'),
  cleanupCommands: (olderThanDays: number = 7) =>
    fetchAPI<{ cleaned_count: number; message: string }>('/commands/cleanup', {
      method: 'POST',
      body: JSON.stringify({ older_than_days: olderThanDays }),
    }),

  // ========== Decisions API ==========
  listDecisions: (params?: {
    decision_type?: string
    priority?: string
    status?: string
    min_confidence?: number
    start_time?: number
    end_time?: number
    limit?: number
    offset?: number
  }) =>
    fetchAPI<DecisionListResponse>(
      `/decisions${params ? `?${new URLSearchParams(
        Object.entries(params).reduce((acc, [key, value]) => {
          if (value !== undefined) acc[key] = String(value)
          return acc
        }, {} as Record<string, string>)
      )}` : ''}`
    ),
  getDecision: (id: string) => fetchAPI<{ decision: DecisionDto }>(`/decisions/${id}`),
  executeDecision: (id: string) =>
    fetchAPI<{ message: string; decision_id: string }>(`/decisions/${id}/execute`, {
      method: 'POST',
    }),
  approveDecision: (id: string) =>
    fetchAPI<{ message: string; decision_id: string }>(`/decisions/${id}/approve`, {
      method: 'POST',
    }),
  rejectDecision: (id: string) =>
    fetchAPI<{ message: string; decision_id: string }>(`/decisions/${id}/reject`, {
      method: 'POST',
    }),
  deleteDecision: (id: string) =>
    fetchAPI<{ message: string; decision_id: string }>(`/decisions/${id}`, {
      method: 'DELETE',
    }),
  getDecisionStats: () => fetchAPI<DecisionStatsResponse>('/decisions/stats'),
  cleanupDecisions: (olderThanDays: number = 30) =>
    fetchAPI<{ cleaned_count: number; message: string }>('/decisions/cleanup', {
      method: 'POST',
      body: JSON.stringify({ older_than_days: olderThanDays }),
    }),

  // ========== Stats API ==========
  getRuleStats: () => fetchAPI<{ stats: { total_rules: number; enabled_rules: number; disabled_rules: number; by_type: Record<string, number> } }>('/stats/rules'),

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
    }),
  disableRule: (id: string) =>
    fetchAPI<{ message: string }>(`/rules/${id}/disable`, {
      method: 'POST',
    }),
  testRule: (id: string) =>
    fetchAPI<{ result: unknown; message?: string }>(`/rules/${id}/test`, {
      method: 'POST',
    }),
  validateRuleDSL: (dsl: string) =>
    fetchAPI<{ valid: boolean; errors?: string[]; parsed?: unknown }>('/rules/validate', {
      method: 'POST',
      body: JSON.stringify({ dsl }),
    }),

  // ========== Workflows API ==========
  listWorkflows: (params?: {
    status?: string
    limit?: number
    offset?: number
  }) =>
    fetchAPI<{ workflows: Array<Workflow>; count: number }>(
      `/workflows${params ? `?${new URLSearchParams(
        Object.entries(params).reduce((acc, [key, value]) => {
          if (value !== undefined) acc[key] = String(value)
          return acc
        }, {} as Record<string, string>)
      )}` : ''}`
    ),
  getWorkflow: (id: string) => fetchAPI<{ workflow: Workflow }>(`/workflows/${id}`),
  createWorkflow: (workflow: Omit<Workflow, 'id' | 'created_at' | 'updated_at'>) =>
    fetchAPI<{ workflow: Workflow; message?: string }>('/workflows', {
      method: 'POST',
      body: JSON.stringify(workflow),
    }),
  updateWorkflow: (id: string, workflow: Partial<Workflow>) =>
    fetchAPI<{ workflow: Workflow; message?: string }>(`/workflows/${id}`, {
      method: 'PUT',
      body: JSON.stringify(workflow),
    }),
  deleteWorkflow: (id: string) =>
    fetchAPI<{ message: string }>(`/workflows/${id}`, {
      method: 'DELETE',
    }),
  executeWorkflow: (id: string, input?: Record<string, unknown>) =>
    fetchAPI<{ execution_id: string; message: string }>(`/workflows/${id}/execute`, {
      method: 'POST',
      body: JSON.stringify({ input }),
    }),
  getWorkflowExecutions: (id: string, limit?: number) =>
    fetchAPI<{ executions: Array<WorkflowExecution> }>(
      `/workflows/${id}/executions${limit ? `?limit=${limit}` : ''}`
    ),

  // ========== Scenarios API ==========
  listScenarios: () =>
    fetchAPI<{ scenarios: Array<Scenario>; count: number }>('/scenarios'),
  getScenario: (id: string) => fetchAPI<{ scenario: Scenario }>(`/scenarios/${id}`),
  createScenario: (scenario: Omit<Scenario, 'id' | 'created_at' | 'updated_at'>) =>
    fetchAPI<{ scenario: Scenario; message?: string }>('/scenarios', {
      method: 'POST',
      body: JSON.stringify(scenario),
    }),
  updateScenario: (id: string, scenario: Partial<Scenario>) =>
    fetchAPI<{ scenario: Scenario; message?: string }>(`/scenarios/${id}`, {
      method: 'PUT',
      body: JSON.stringify(scenario),
    }),
  deleteScenario: (id: string) =>
    fetchAPI<{ message: string }>(`/scenarios/${id}`, {
      method: 'DELETE',
    }),
  activateScenario: (id: string) =>
    fetchAPI<{ message: string }>(`/scenarios/${id}/activate`, {
      method: 'POST',
    }),
  deactivateScenario: (id: string) =>
    fetchAPI<{ message: string }>(`/scenarios/${id}/deactivate`, {
      method: 'POST',
    }),
  getScenarioTemplates: () =>
    fetchAPI<{ templates: Array<ScenarioTemplate> }>('/scenarios/templates'),

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

  // ========== Events API ==========
  getEvents: (params?: {
    event_type?: string
    source?: string
    start_time?: number
    end_time?: number
    limit?: number
  }) =>
    fetchAPI<{ events: Array<Event>; count: number }>(
      `/events${params ? `?${new URLSearchParams(
        Object.entries(params).reduce((acc, [key, value]) => {
          if (value !== undefined) acc[key] = String(value)
          return acc
        }, {} as Record<string, string>)
      )}` : ''}`
    ),
  subscribeEvents: () => fetchAPI<{ ws_url: string; subscription_id: string }>('/events/subscribe'),

  // ========== Plugins API ==========
  // Matches backend: crates/api/src/handlers/plugins.rs
  //
  // Response format: { plugins: PluginDto[], count: number } (no success wrapper)

  listPlugins: (params?: {
    type?: string      // Filter by plugin_type (llm_backend, device_adapter, etc.)
    state?: string     // Filter by state (Loaded, Running, Stopped, etc.)
    enabled?: boolean  // Filter by enabled status
  }) =>
    fetchAPI<{ plugins: Plugin[]; count: number }>(
      `/plugins${params ? `?${new URLSearchParams(
        Object.entries(params).reduce((acc, [key, value]) => {
          if (value !== undefined) acc[key] = String(value)
          return acc
        }, {} as Record<string, string>)
      )}` : ''}`
    ),
  getPlugin: (id: string) =>
    fetchAPI<{ plugin: Plugin }>(`/plugins/${id}`),

  registerPlugin: (plugin: {
    id: string
    name: string
    plugin_type: string
    description?: string
    path?: string
    config?: Record<string, unknown>
    auto_start?: boolean
    enabled?: boolean
  }) =>
    fetchAPI<{ message: string; plugin_id: string }>('/plugins', {
      method: 'POST',
      body: JSON.stringify(plugin),
    }),

  unregisterPlugin: (id: string) =>
    fetchAPI<{ message: string }>(`/plugins/${id}`, {
      method: 'DELETE',
    }),

  enablePlugin: (id: string) =>
    fetchAPI<{ message: string }>(`/plugins/${id}/enable`, {
      method: 'POST',
    }),

  disablePlugin: (id: string) =>
    fetchAPI<{ message: string }>(`/plugins/${id}/disable`, {
      method: 'POST',
    }),

  startPlugin: (id: string) =>
    fetchAPI<{ message: string }>(`/plugins/${id}/start`, {
      method: 'POST',
    }),

  stopPlugin: (id: string) =>
    fetchAPI<{ message: string }>(`/plugins/${id}/stop`, {
      method: 'POST',
    }),

  getPluginConfig: (id: string) =>
    fetchAPI<{ plugin_id: string; config: Record<string, unknown> }>(`/plugins/${id}/config`),

  updatePluginConfig: (id: string, config: Record<string, unknown>, reload?: boolean) =>
    fetchAPI<{ message: string }>(`/plugins/${id}/config`, {
      method: 'PUT',
      body: JSON.stringify({ config, reload }),
    }),

  executePluginCommand: (id: string, command: string, parameters?: Record<string, unknown>) =>
    fetchAPI<{ result: unknown }>(`/plugins/${id}/command`, {
      method: 'POST',
      body: JSON.stringify({ command, args: parameters }),
    }),

  getPluginStats: (id: string) =>
    fetchAPI<{ plugin_id: string; stats: PluginStatsDto }>(`/plugins/${id}/stats`),

  getPluginHealth: (id: string) =>
    fetchAPI<{ status: string; plugin_id: string; state: string }>(`/plugins/${id}/health`),

  discoverPlugins: () =>
    fetchAPI<{ message: string; count: number }>('/plugins/discover', {
      method: 'POST',
    }),

  listPluginsByType: (type: string) =>
    fetchAPI<{ plugin_type: string; plugins: Plugin[]; count: number }>(`/plugins/type/${type}`),

  getPluginTypesSummary: () =>
    fetchAPI<{ types: Record<string, number>; total: number }>('/plugins/types'),

  // ========== Device Adapter Plugins API ==========
  // Matches backend: crates/api/src/handlers/plugins.rs:632-756

  listDeviceAdapters: () =>
    fetchAPI<{ total_adapters: number; running_adapters: number; total_devices: number; adapters: AdapterPluginDto[] }>('/plugins/device-adapters'),

  registerDeviceAdapter: (adapter: {
    id: string
    name: string
    adapter_type: string
    config?: Record<string, unknown>
    auto_start?: boolean
    enabled?: boolean
  }) =>
    fetchAPI<{ message: string; plugin_id: string }>('/plugins/device-adapters', {
      method: 'POST',
      body: JSON.stringify(adapter),
    }),

  getDeviceAdapterStats: () =>
    fetchAPI<{ total_adapters: number; running_adapters: number; total_devices: number; adapters: AdapterPluginDto[] }>('/plugins/device-adapters/stats'),

  getAdapterDevices: (pluginId: string) =>
    fetchAPI<{ plugin_id: string; devices: AdapterDeviceDto[]; count: number }>(`/plugins/${pluginId}/devices`),

  // ========== Tools API ==========
  listTools: () =>
    fetchAPI<{ tools: Tool[] }>('/tools'),
  getToolSchema: (name: string) =>
    fetchAPI<{ schema: ToolSchema }>(`/tools/${name}/schema`),
  getToolMetrics: () =>
    fetchAPI<{ metrics: ToolMetrics }>('/tools/metrics'),
  executeTool: (name: string, parameters: Record<string, unknown>) =>
    fetchAPI<{ result: ToolExecutionResult }>(`/tools/${name}/execute`, {
      method: 'POST',
      body: JSON.stringify({ parameters }),
    }),
  formatForLLM: () =>
    fetchAPI<{ formatted: string }>('/tools/format-for-llm'),

  // ========== Bulk Operations API ==========
  bulkCreateAlerts: (alerts: Array<{ title: string; message: string; severity?: string }>) =>
    fetchAPI<{ created: number; ids: string[] }>('/bulk/alerts', {
      method: 'POST',
      body: JSON.stringify({ alerts }),
    }),
  bulkResolveAlerts: (ids: string[]) =>
    fetchAPI<{ resolved: number }>('/bulk/alerts/resolve', {
      method: 'POST',
      body: JSON.stringify({ alert_ids: ids }),
    }),
  bulkAcknowledgeAlerts: (ids: string[]) =>
    fetchAPI<{ acknowledged: number }>('/bulk/alerts/acknowledge', {
      method: 'POST',
      body: JSON.stringify({ alert_ids: ids }),
    }),
  bulkDeleteAlerts: (ids: string[]) =>
    fetchAPI<{ deleted: number }>('/bulk/alerts/delete', {
      method: 'POST',
      body: JSON.stringify({ alert_ids: ids }),
    }),
  bulkDeleteSessions: (ids: string[]) =>
    fetchAPI<{ deleted: number }>('/bulk/sessions/delete', {
      method: 'POST',
      body: JSON.stringify({ session_ids: ids }),
    }),
  bulkDeleteDevices: (ids: string[]) =>
    fetchAPI<{ deleted: number }>('/bulk/devices/delete', {
      method: 'POST',
      body: JSON.stringify({ device_ids: ids }),
    }),
  bulkDeleteDeviceTypes: (ids: string[]) =>
    fetchAPI<{ deleted: number }>('/bulk/device-types/delete', {
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

  // ========== Extended Events API ==========
  getEventHistory: (params?: { event_type?: string; source?: string; limit?: number }) =>
    fetchAPI<{ events: Array<NeoTalkEvent>; count: number }>('/events/history' +
      (params ? `?${new URLSearchParams(
        Object.entries(params).reduce((acc, [key, value]) => {
          if (value !== undefined) acc[key] = String(value)
          return acc
        }, {} as Record<string, string>)
      )}` : '')
    ),
  getEventStats: () =>
    fetchAPI<{ stats: EventStats }>('/events/stats'),
  subscribeToEvents: (eventTypes: string[], source?: string) =>
    fetchAPI<{ subscription_id: string; ws_url: string }>('/events/subscribe', {
      method: 'POST',
      body: JSON.stringify({ event_types: eventTypes, source }),
    }),
  unsubscribeFromEvents: (id: string) =>
    fetchAPI<{ message: string }>(`/events/subscribe/${id}`, {
      method: 'DELETE',
    }),
}
