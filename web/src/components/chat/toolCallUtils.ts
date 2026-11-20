// Chat tool-call helpers — split from ToolCallVisualization.tsx
// (react-refresh: component files export only components).

import type { ToolCall } from "@/types"
import type { SendCommandDeliveryResult } from "@/lib/commandDeliveryLadder"

/** Parse a tool result into an object regardless of transport shape (live
 * events deliver objects; some persisted/eval records store JSON strings). */
function asResultObject(result: unknown): Record<string, unknown> | null {
  if (!result) return null
  if (typeof result === "string") {
    const s = result.trim()
    if (!s.startsWith("{") && !s.startsWith("[")) return null
    try {
      const parsed: unknown = JSON.parse(s)
      return parsed && typeof parsed === "object" && !Array.isArray(parsed)
        ? (parsed as Record<string, unknown>)
        : null
    } catch {
      return null
    }
  }
  return typeof result === "object" && !Array.isArray(result)
    ? (result as Record<string, unknown>)
    : null
}

/** A failed tool is flagged by the live event, carries a shell/CLI failure
 * envelope, or carries a non-empty domain error envelope. The shell branch
 * mirrors the backend's `effective_success` (stream_core judges retries and
 * progress with it — the UI must not disagree): exit_code != 0, timed_out,
 * or pipeline-masked "command not found" are failures even when the
 * transport-level `success` flag is true (2026-11-10: exit_code=2 commands
 * were rendered as successful). Persisted history has no `success` flag at
 * all, so the envelope checks are the only signal after a reload. */
export function isToolError(toolCall: ToolCall): boolean {
  if (toolCall.success === false) return true
  const r = asResultObject(toolCall.result)
  if (!r) return false
  const isShell = toolCall.name === "shell" || "exit_code" in r || "timed_out" in r
  if (isShell) {
    if (r.timed_out === true) return true
    if (typeof r.exit_code === "number" && r.exit_code !== 0) return true
    if (["stdout", "stderr"].some(
      (k) => typeof r[k] === "string" && (r[k] as string).toLowerCase().includes("command not found"),
    )) {
      return true
    }
    // No shell-envelope failure — FALL THROUGH to the error-envelope check:
    // legacy records carry {"error": "Tool execution failed"} with no exit
    // code, and must not render as successful (2026-11-10 review regression).
  }
  // Domain tools (and shell fall-through): a non-empty error envelope is a
  // failure; payloads that merely CONTAIN an `error` field (e.g. {"error":
  // null} business data) are not.
  const err: unknown = r.error
  return err != null && err !== ""
}

/** Prefer the LLM's brief audit `description` when present; otherwise derive
 * a scannable label from the shell command / action args. */
export function getToolDisplayName(name: string, args?: unknown): string {
  if (args && typeof args === "object") {
    const a = args as Record<string, unknown>
    if (typeof a.description === "string" && a.description.trim()) {
      return a.description.trim()
    }

    // Shell tool: parse "neomind device list" → "device list"
    if (typeof a.command === "string" && a.command.trim()) {
      const cmd = a.command.trim()
      if (cmd.startsWith("neomind ")) {
        let rest = cmd.slice(8).trim()
        // Remove cosmetic flags
        rest = rest.replace(/\s*--(?:json|yes|force)\b/g, "")
        return rest || name
      }
      // Non-neomind shell commands: show as-is (truncated)
      return cmd.length > 40 ? cmd.slice(0, 37) + "..." : cmd
    }

    // Legacy aggregated tools: device(action)
    const action = typeof a.action === "string" ? a.action : ""
    if (action) {
      const extras: string[] = []
      if (typeof a.device_id === "string") extras.push(a.device_id)
      if (typeof a.metric === "string") extras.push(a.metric)
      const suffix = extras.length > 0 ? ` ${extras.join(" · ")}` : ""
      return `${name} ${action}${suffix}`
    }
  }
  return name
}

/** Short failure reason for the tool row (right side). Prefer envelope
 * error / stderr first line; fall back to null so the caller can show the
 * generic "failed" label. */
export function getToolFailureReason(toolCall: ToolCall): string | null {
  const r = asResultObject(toolCall.result)
  if (!r) return null

  const pick = (raw: unknown): string | null => {
    if (typeof raw !== "string") return null
    const line = raw.trim().split("\n").find((l) => l.trim())?.trim()
    if (!line) return null
    return line.length > 48 ? `${line.slice(0, 45)}…` : line
  }

  return (
    pick(r.error) ||
    pick(r.stderr) ||
    pick(r.message) ||
    (r.timed_out === true ? "timeout" : null)
  )
}

/** Walk a tool result for structured device-command delivery (`sent` +
 * `delivery`). Only returns a payload when the wire shape is present —
 * never invents a receipt from success alone. */
export function extractToolDelivery(
  toolCall: ToolCall,
): SendCommandDeliveryResult | null {
  const candidates: unknown[] = [toolCall.result]

  const root = asResultObject(toolCall.result)
  if (root) {
    if (root.data != null) candidates.push(root.data)
    if (root.stdout != null) candidates.push(root.stdout)
    const stdoutObj = asResultObject(root.stdout)
    if (stdoutObj?.data != null) candidates.push(stdoutObj.data)
    if (stdoutObj) candidates.push(stdoutObj)
  }

  for (const c of candidates) {
    const obj = asResultObject(c)
    if (!obj) continue
    if (typeof obj.sent !== "boolean") continue
    // Only surface when structured delivery evidence exists — bare
    // `{sent:true}` without delivery fields is not a receipt.
    if (!("delivery" in obj) && !("delivery_note" in obj)) continue

    return {
      sent: obj.sent,
      delivery: (obj.delivery as SendCommandDeliveryResult["delivery"]) ?? null,
      delivery_note:
        typeof obj.delivery_note === "string" ? obj.delivery_note : null,
    }
  }
  return null
}

/** Check if thinking duplicates the content (Phase 2 LLM may echo response as "thinking") */
export function isThinkingDuplicate(thinking: string | undefined, content: string | undefined): boolean {
  if (!thinking || !content) return false
  const tPreview = thinking.slice(0, 200)
  const cPreview = content.slice(0, 200)
  if (!tPreview || !cPreview) return false
  return cPreview.includes(tPreview) || tPreview.includes(cPreview)
}
