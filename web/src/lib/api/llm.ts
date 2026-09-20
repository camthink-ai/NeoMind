/** llm api — split from api.ts 2026-09. */
import type { LlmBackendInstance, CreateLlmBackendRequest, UpdateLlmBackendRequest, LlmBackendListResponse, BuiltinLlmStatus, BuiltinModelDef, BackendTypeDefinition, BackendTestResult } from '@/types'
import type { ReasoningCapabilities } from '@/types/llm-backend'
import { fetchAPI } from "./client"

export const llmApi = {
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
  /** POST /api/builtin-llm/download/cancel — stop the in-flight model
   * download (partial file is kept; re-download resumes; another model can
   * be downloaded right after). */
  cancelModelDownload: () =>
    fetchAPI<{ cancelled: boolean; active?: boolean }>('/builtin-llm/download/cancel', {
      method: 'POST',
    }),

  /**
   * Start / resume the model download (single-flight on the server).
   * POST /api/builtin-llm/download
   */
  downloadBuiltinLlm: (modelId?: string) =>
    fetchAPI<{ started: boolean; already_running: boolean }>('/builtin-llm/download', {
      method: 'POST',
      body: modelId ? JSON.stringify({ model_id: modelId }) : undefined,
    }),
  /**
   * List installable builtin models with per-entry install state.
   * GET /api/builtin-llm/models
   */
  getBuiltinModels: () =>
    fetchAPI<{ models: BuiltinModelDef[]; default_model_id: string }>('/builtin-llm/models'),

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
  restartBuiltinLlm: (ctx?: number) =>
    fetchAPI<{ restarted: boolean; already_running: boolean; endpoint?: string }>(
      `/builtin-llm/restart${ctx ? `?ctx=${ctx}` : ''}`,
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

}
