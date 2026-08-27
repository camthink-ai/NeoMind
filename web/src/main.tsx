import { StrictMode } from "react"
import * as React from "react"
import { createRoot } from "react-dom/client"
import { BrowserRouter } from "react-router-dom"
import * as jsxRuntime from "react/jsx-runtime"
import "./index.css"
import "./i18n/config"
import App from "./App"
import { ThemeProvider } from "@/components/ui/theme"
import { initVisualViewport } from "@/hooks/useVisualViewport"

// Expose React and JSX runtime as globals for community component bundles (IIFE)
;(window as any).React = React
;(window as any).jsxRuntime = jsxRuntime

// Expose NeoMind API for community/extension components
;(window as any).neomind = {
  /**
   * Call an extension command from a frontend component.
   * POST /api/extensions/:id/command
   */
  callExtension: async (extensionId: string, command: string, args?: Record<string, unknown>) => {
    const { api } = await import('@/lib/api')
    return api.executeCommand(extensionId, { command, args: args || {} })
  },

  /**
   * List all installed extensions.
   * GET /api/extensions
   */
  listExtensions: async () => {
    const { api } = await import('@/lib/api')
    return api.listExtensions()
  },

  /**
   * Create a transform automation.
   * Supports two formats:
   *   1. js_code format: { js_code, output_prefix, rule } — AI-native JavaScript transform
   *   2. operations format: { extension_id, command, input, output } — legacy extension-based
   * POST /api/automations
   */
  createTransform: async (config: {
    name: string
    description?: string
    scope: string
    extension_id?: string
    command?: string
    input?: Record<string, unknown>
    output?: Record<string, unknown>
    args?: Record<string, unknown>
    js_code?: string
    output_prefix?: string
    rule: { device_id: string; device_type?: string; _fp?: string }
  }) => {
    const { api } = await import('@/lib/api')

    let definition: Record<string, unknown>

    if (config.js_code) {
      // js_code format: pass directly as TransformAutomation fields
      definition = {
        scope: { device: config.scope },
        js_code: config.js_code,
        output_prefix: config.output_prefix || 'transform',
        rule: config.rule,
      }
    } else {
      // Legacy operations format
      const parameters: Record<string, unknown> = { ...(config.input || {}), ...(config.args || {}) }
      const outputMetrics = config.output ? Object.keys(config.output) : []

      definition = {
        scope: { device: config.scope },
        operations: [{
          op_type: 'extension',
          extension_id: config.extension_id || '',
          command: config.command || 'detect',
          parameters,
          output_metrics: outputMetrics,
          output_mapping: config.output || null,
        }],
      }
    }

    const result = await api.createAutomation({
      name: config.name,
      description: config.description || '',
      type: 'transform',
      enabled: true,
      definition,
    })

    // Return shape expected by components: { id, name, scope, extension_id, rule, status, created_at }
    const automation = result.automation
    return {
      id: automation.id,
      name: automation.name,
      scope: config.scope,
      extension_id: config.extension_id,
      rule: config.rule,
      status: automation.enabled ? 'active' : 'paused',
      created_at: automation.created_at,
    }
  },

  /**
   * Delete a transform automation.
   * DELETE /api/automations/:id
   */
  deleteTransform: async (id: string) => {
    const { api } = await import('@/lib/api')
    await api.deleteAutomation(id)
  },

  /**
   * Update a transform automation in place (avoids delete+create race conditions).
   * PUT /api/automations/:id
   */
  updateTransform: async (id: string, config: {
    name?: string
    description?: string
    scope?: string
    js_code?: string
    output_prefix?: string
  }) => {
    const { api } = await import('@/lib/api')
    const definition: Record<string, unknown> = {}
    if (config.scope) definition.scope = { device: config.scope }
    if (config.js_code) definition.js_code = config.js_code
    if (config.output_prefix) definition.output_prefix = config.output_prefix

    // Errors propagate so the caller can handle fallback (e.g., recreate Transform)
    const result = await api.updateAutomation(id, {
      name: config.name,
      description: config.description,
      definition: Object.keys(definition).length > 0 ? definition : undefined,
    })
    const automation = result.automation
    return {
      id: automation.id,
      name: automation.name,
      status: automation.enabled ? 'active' : 'paused',
    }
  },

  /**
   * List transform automations, optionally filtered.
   * GET /api/automations?type=transform
   */
  listTransforms: async (filter?: { scope?: string; extension_id?: string }) => {
    const { api } = await import('@/lib/api')
    const result = await api.listAutomations({ type: 'transform' })
    return result.automations.map((a: any) => ({
      id: a.id,
      name: a.name,
      description: a.description || '',
      scope: a.scope || '',
      extension_id: a.extension_id || '',
      rule: a.rule || {},
      status: a.enabled ? 'active' : 'paused',
      created_at: a.created_at,
    }))
  },

  /**
   * Write a virtual metric value for a device.
   * POST /api/automations/transforms/process
   */
  writeMetric: async (deviceId: string, metric: string, value: unknown) => {
    // Virtual metrics are produced by transforms.
    // For direct writes from components, we use the transform process endpoint
    // with a minimal inline payload that sets the metric directly.
    const { api } = await import('@/lib/api')
    try {
      await api.processTransformData({
        device_id: deviceId,
        data: { [metric]: value },
      })
    } catch {
      // Silent degradation — components should never block on metric writes
    }
  },

  /**
   * Fetch the latest telemetry values for a device.
   * GET /api/devices/:id — returns fresh current_values from the backend.
   * Used by community components to poll for data that may not arrive via WS
   * (e.g., large base64 images that exceed WS message size limits).
   */
  fetchDeviceValues: async (deviceId: string): Promise<Record<string, unknown> | null> => {
    const { api } = await import('@/lib/api')
    try {
      const result = await api.getDeviceCurrent(deviceId)
      // The API returns { device, metrics: { name: { value, is_virtual, ... } }, commands }
      // Extract raw values from the metrics map, handling both current and legacy formats.
      const metrics = result.metrics
      const cv: Record<string, unknown> = {}
      if (metrics && typeof metrics === 'object') {
        for (const [key, entry] of Object.entries(metrics)) {
          // Each metric is { name, value, is_virtual, ... } — extract the raw value.
          // Skip metrics with null value (no data) to avoid returning the wrapper object.
          const val = entry && typeof entry === 'object' ? entry.value : entry
          if (val != null) cv[key] = val
        }
      }
      // Also check legacy flat formats
      const legacy = (result as Partial<Record<'current_values' | 'values', Record<string, unknown>>>).current_values
        ?? (result as Partial<Record<'current_values' | 'values', Record<string, unknown>>>).values
      if (legacy && typeof legacy === 'object') {
        Object.assign(cv, legacy)
      }
      if (Object.keys(cv).length === 0) return null
      // Convert flat dot-separated keys to nested structure (matching store behavior)
      const nested: Record<string, unknown> = {}
      for (const [key, value] of Object.entries(cv)) {
        const parts = key.split('.')
        let target = nested
        for (let i = 0; i < parts.length - 1; i++) {
          if (!target[parts[i]] || typeof target[parts[i]] !== 'object') {
            target[parts[i]] = {}
          }
          target = target[parts[i]] as Record<string, unknown>
        }
        target[parts[parts.length - 1]] = value
      }
      return nested
    } catch {
      return null
    }
  },

  /**
   * List all devices known to the platform.
   * GET /api/devices -> { devices: Device[] }
   * Used by community components that need to enumerate devices (e.g. the 3D
   * viewer's per-pin device picker). Returns a minimal shape.
   */
  listDevices: async (): Promise<Array<{ id: string; name: string; device_type: string }>> => {
    const { api } = await import('@/lib/api')
    try {
      const result = await api.getDevices()
      return (result.devices || []).map((d: any) => ({
        id: d.id,
        name: d.name || d.id,
        device_type: d.device_type || '',
      }))
    } catch {
      return []
    }
  },

  /**
   * Fetch a device-type definition (metrics + commands schema).
   * GET /api/device-types/:id -> DeviceType
   * Used by community components to populate per-pin metric/command pickers
   * from the stable schema rather than only live-reported values.
   */
  getDeviceType: async (deviceType: string): Promise<{
    metrics: Array<{ name: string; display_name: string; data_type?: string; unit?: string }>
    commands: Array<{ name: string; display_name: string }>
  } | null> => {
    const { api } = await import('@/lib/api')
    try {
      const t: any = await api.getDeviceType(deviceType)
      return {
        metrics: Array.isArray(t?.metrics) ? t.metrics : [],
        commands: Array.isArray(t?.commands) ? t.commands : [],
      }
    } catch {
      return null
    }
  },
}

// Initialize global VisualViewport tracking for mobile keyboard handling
initVisualViewport()

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    {/* react-router v7: v7_relativeSplatPath / v7_startTransition are now the
        only behaviours — the old `future` prop no longer exists. */}
    <BrowserRouter>
      <ThemeProvider>
        <App />
      </ThemeProvider>
    </BrowserRouter>
  </StrictMode>,
)
