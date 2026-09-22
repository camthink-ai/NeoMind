/** agents api — split from api.ts 2026-09. */
import type { AiAgentDetail, AgentMemory, AgentToolCatalogItem, AgentStats, AgentExecutionDetail, CreateAgentRequest, UpdateAgentRequest, ExecuteAgentRequest, ValidateLlmRequest, ValidateLlmResponse, AgentListResponse, AgentExecutionsResponse, ParsedIntent, AgentAvailableResources, UserMessage, DashboardResponse, CreateDashboardRequest, UpdateDashboardRequest, DashboardTemplateResponse, MemorySystemConfig } from '@/types'
import type { SkillSummary, SkillDetail } from '@/types/skill'
import { fetchAPI } from "./client"

export const agentsApi = {
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
  // options lets polling callers (dashboard widgets) suppress the global
  // error toast — they render their own not-found state instead.
  getAgent: (id: string, options?: { skipErrorToast?: boolean }) =>
    fetchAPI<AiAgentDetail>(`/agents/${id}`, options),

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
   * Save-before dry run for a structured agent (transient, nothing persisted)
   * POST /api/agents/test-preview
   */
  testAgentPreview: (req: import('@/types').TestPreviewRequest) =>
    fetchAPI<import('@/types').DryRunResult>('/agents/test-preview', {
      method: 'POST',
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
      /** Present when the run exceeds the 60s wait window: execution
       * continues in the background; poll poll_execution for the result. */
      still_executing?: boolean
      message?: string
      poll_execution?: string
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
  getAgentExecutions: (id: string, limit = 50, options?: { skipErrorToast?: boolean }) =>
    fetchAPI<AgentExecutionsResponse>(`/agents/${id}/executions?limit=${limit}`, options),

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
  getAgentUserMessages: (id: string, options?: { skipErrorToast?: boolean }) =>
    fetchAPI<UserMessage[]>(`/agents/${id}/messages`, options),

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

}
