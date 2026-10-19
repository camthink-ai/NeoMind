// Chat tool-call helpers — split from ToolCallVisualization.tsx
// (react-refresh: component files export only components).

import type { ToolCall } from "@/types"

/** A failed tool is either flagged by the live event or carries an error result. */
export function isToolError(toolCall: ToolCall): boolean {
  if (toolCall.success === false) return true
  const result = toolCall.result
  return !!result && typeof result === "object" && "error" in (result as Record<string, unknown>)
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
