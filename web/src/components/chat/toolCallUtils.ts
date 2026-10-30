// Chat tool-call helpers — split from ToolCallVisualization.tsx
// (react-refresh: component files export only components).

import type { ToolCall } from "@/types"

function resultObject(value: unknown): Record<string, unknown> | undefined {
  if (typeof value === "string") {
    try { value = JSON.parse(value) } catch { return undefined }
  }
  return value && typeof value === "object" && !Array.isArray(value)
    ? value as Record<string, unknown>
    : undefined
}

/** Use execution facts for live and restored cards; business fields are not status. */
export function isToolError(toolCall: ToolCall): boolean {
  if (toolCall.success === false) return true
  const result = resultObject(toolCall.result)
  if (!result) return false
  const data = resultObject(result.data) ?? result
  const isShell = toolCall.name === "shell" || typeof toolCall.arguments?.command === "string"
  if (isShell) {
    if (data.timed_out === true) return true
    if (typeof data.exit_code === "number") return data.exit_code !== 0
    if ("exit_code" in data && data.exit_code === null) return true
  }
  if (toolCall.success === true) return false
  // The failed ToolOutput envelope preserves partial data with its status.
  if (result.success === false && "data" in result && "error" in result) return true
  // Older persisted executor errors used a single error field. Avoid treating
  // arbitrary business payloads containing error:null, error:0 or device faults as failures.
  return Object.keys(result).length === 1
    && typeof result.error === "string"
    && result.error.trim().length > 0
}

/** "device list --yes" → "device list" */
export function getToolDisplayName(name: string, args?: unknown): string {
  if (!args || typeof args !== 'object') return name
  const a = args as Record<string, unknown>

  // Shell tool: parse "neomind device list" → "device list"
  if (typeof a.command === 'string' && a.command.trim()) {
    const cmd = a.command.trim()
    if (cmd.startsWith('neomind ')) {
      let rest = cmd.slice(8).trim()
      // Remove cosmetic flags
      rest = rest.replace(/\s*--(?:json|yes|force)\b/g, '')
      return rest || name
    }
    // Non-neomind shell commands: show as-is (truncated)
    return cmd.length > 40 ? cmd.slice(0, 37) + '...' : cmd
  }

  // Legacy aggregated tools: device(action)
  const action = typeof a.action === 'string' ? a.action : ''
  if (!action) return name
  const extras: string[] = []
  if (typeof a.device_id === 'string') extras.push(a.device_id)
  if (typeof a.metric === 'string') extras.push(a.metric)
  const suffix = extras.length > 0 ? ` ${extras.join(' · ')}` : ''
  return `${name} ${action}${suffix}`
}

/** Check if thinking duplicates the content (Phase 2 LLM may echo response as "thinking") */
export function isThinkingDuplicate(thinking: string | undefined, content: string | undefined): boolean {
  if (!thinking || !content) return false
  const tPreview = thinking.slice(0, 200)
  const cPreview = content.slice(0, 200)
  if (!tPreview || !cPreview) return false
  return cPreview.includes(tPreview) || tPreview.includes(cPreview)
}
