/** automation api — split from api.ts 2026-09. */
import type { Rule } from '@/types'
import { fetchAPI } from "./client"

export const automationApi = {
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

}
