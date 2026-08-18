/**
 * ToolCallVisualization - Displays AI tool/function calls as a "task process"
 *
 * Mental model: tool calls are a process the AI goes through to complete a task.
 * The final answer (message.content) is always shown separately below this block.
 *
 * Structure:
 *   ┌─ 🔧 N tool calls · M steps  ▾ ─────────┐
 *   │ Step 1: 🧠 Thinking + 🔧 Tools          │  ← collapsible
 *   │ Step 2: 🧠 Thinking + 🔧 Tools          │
 *   └──────────────────────────────────────────┘
 *   Markdown content (the final answer)        ← rendered separately in chat.tsx
 */

import { useState, useEffect } from "react"
import { useTranslation } from "react-i18next"
import { Wrench, ChevronDown, CheckCircle2, Loader2, Code, FileText } from "lucide-react"
import { cn } from "@/lib/utils"
import { textMini, textCode } from "@/design-system/tokens/typography"
import type { ToolCall } from "@/types"
import { BuildCard, parseBuildResponse } from "@/components/chat/BuildCard"

// ─── Helpers ───

type ToolStatus = "pending" | "running" | "completed"

function getToolStatus(hasResult: boolean, isStreaming: boolean): ToolStatus {
  if (hasResult) return "completed"
  if (isStreaming) return "running"
  return "pending"
}

function formatDuration(ms?: number): string {
  if (!ms) return ""
  if (ms < 1000) return `${ms}ms`
  return `${(ms / 1000).toFixed(1)}s`
}

/** "device list --yes" → "device list" */
function getToolDisplayName(name: string, args?: unknown): string {
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

function formatJson(data: unknown): string {
  if (typeof data === "string") {
    try {
      const parsed = JSON.parse(data)
      return JSON.stringify(parsed, null, 2)
    } catch {
      return data
    }
  }
  return JSON.stringify(data, null, 2)
}

function groupByRound(toolCalls: ToolCall[]): Map<number, ToolCall[]> {
  const groups = new Map<number, ToolCall[]>()
  for (const tc of toolCalls) {
    const round = tc.round ?? 1
    if (!groups.has(round)) groups.set(round, [])
    groups.get(round)!.push(tc)
  }
  return groups
}

/** Check if thinking duplicates the content (Phase 2 LLM may echo response as "thinking") */
export function isThinkingDuplicate(thinking: string | undefined, content: string | undefined): boolean {
  if (!thinking || !content) return false
  const tPreview = thinking.slice(0, 200)
  const cPreview = content.slice(0, 200)
  if (!tPreview || !cPreview) return false
  return cPreview.includes(tPreview) || tPreview.includes(cPreview)
}

// ─── ToolProcessBlock: the main exported component ───

interface ToolProcessBlockProps {
  toolCalls: ToolCall[]
  /** Per-round intermediate content (shown alongside tools in each round) */
  roundContents?: Record<number, string>
  isStreaming?: boolean
}

/**
 * Renders the tool-calling process as a collapsible card.
 * Shows: summary header + per-round content & tool calls.
 * Does NOT render thinking or final content — those are handled by chat.tsx.
 */
export function ToolProcessBlock({
  toolCalls,
  roundContents = {},
  isStreaming = false,
}: ToolProcessBlockProps) {
  const { t } = useTranslation("chat")
  const [isExpanded, setIsExpanded] = useState(true)

  // Auto-collapse when many tool calls are present and all completed
  useEffect(() => {
    if (toolCalls.length > 4 && !isStreaming) {
      setIsExpanded(false)
    }
  }, [toolCalls.length, isStreaming])

  if (!toolCalls || toolCalls.length === 0) return null

  const toolGroups = groupByRound(toolCalls)
  const steps = Array.from(toolGroups.keys()).sort((a, b) => a - b)
  const completedCount = toolCalls.filter(tc => tc.result != null).length
  const allComplete = completedCount === toolCalls.length && !isStreaming
  const manyCalls = toolCalls.length > 4

  return (
    <div className="mb-4 overflow-hidden rounded-lg bg-muted/40">
      {/* Summary header */}
      <button
        onClick={() => setIsExpanded(!isExpanded)}
        className="w-full flex items-center gap-2.5 px-3 py-2 text-left hover:bg-muted-50 transition-colors"
      >
        <div className={cn(
          "h-4 w-4 rounded flex items-center justify-center shrink-0",
          allComplete && "bg-accent-emerald-light text-accent-emerald",
          isStreaming && "bg-warning-light text-warning",
          !allComplete && !isStreaming && "bg-muted text-muted-foreground"
        )}>
          {isStreaming ? (
            <Loader2 className="h-2.5 w-2.5 animate-spin" />
          ) : allComplete ? (
            <CheckCircle2 className="h-4 w-4" />
          ) : (
            <Wrench className="h-2.5 w-2.5" />
          )}
        </div>
        <span className={cn("text-body sm:text-sm", "font-medium text-foreground")}>
          {isStreaming
            ? `${completedCount}/${toolCalls.length} ${t("toolCall.status.running")}`
            : `${toolCalls.length} ${t("toolCall.title")} · ${steps.length} ${t("toolCall.rounds")}`
          }
        </span>
        <div className="flex-1" />
        <ChevronDown className={cn(
          "h-4 w-4 text-muted-foreground transition-transform duration-200",
          isExpanded && "rotate-180"
        )} />
      </button>

      {/* Steps */}
      {isExpanded && (
        <div>
          {steps.map((step, idx) => {
            const calls = toolGroups.get(step) ?? []
            const isLastStep = idx === steps.length - 1
            const stepStreaming = isStreaming && isLastStep
            const roundContent = roundContents[step]
            return (
              <div key={step} className="mb-1">
                {/* Round content (intermediate results) */}
                {roundContent && (
                  <RoundContent content={roundContent} />
                )}
                {calls.map((tc, i) => (
                  <ToolCallItem
                    key={`${step}-${i}`}
                    toolCall={tc}
                    isStreaming={stepStreaming && tc.result == null}
                    t={t}
                  />
                ))}
              </div>
            )
          })}
        </div>
      )}
    </div>
  )
}

/** Round content block - shows intermediate results from each round */
function RoundContent({ content }: { content: string }) {
  const [isExpanded, setIsExpanded] = useState(false)

  if (!content || content.trim().length === 0) return null

  return (
    <div>
      <button
        onClick={() => setIsExpanded(!isExpanded)}
        className="w-full flex items-center gap-2 px-3 py-1 text-left hover:bg-muted-30 transition-colors"
      >
        <FileText className="h-3.5 w-3.5 text-muted-foreground shrink-0" />
        {!isExpanded && (
          <span className={cn(textMini, "text-muted-foreground truncate")}>
            {content.slice(0, 80)}{content.length > 80 && "..."}
          </span>
        )}
        {isExpanded && (
          <span className={cn(textMini, "text-muted-foreground")}>Details</span>
        )}
        <div className="flex-1" />
        <ChevronDown className={cn(
          "h-4 w-4 text-muted-foreground transition-transform",
          isExpanded && "rotate-180"
        )} />
      </button>
      {isExpanded && (
        <div className="px-3 pb-2">
          <div className={cn("text-body sm:text-sm", "font-mono text-muted-foreground whitespace-pre-wrap break-words leading-relaxed")}>
            {content}
          </div>
        </div>
      )}
    </div>
  )
}

// ─── ToolCallItem: single tool call with expandable args/result ───

function ToolCallItem({
  toolCall,
  isStreaming,
  t
}: {
  toolCall: ToolCall
  isStreaming: boolean
  t: (key: string) => string
}) {
  const [isExpanded, setIsExpanded] = useState(false)
  const status = getToolStatus(
    toolCall.result !== undefined && toolCall.result !== null,
    isStreaming
  )
  const hasArguments = toolCall.arguments !== undefined && toolCall.arguments !== null
  const hasResult = toolCall.result !== undefined && toolCall.result !== null
  const hasDetails = hasArguments || hasResult

  // Check if result contains build_meta for rich card rendering
  const buildResponse = hasResult ? parseBuildResponse(toolCall.result) : null
  const isBuildCard = buildResponse !== null

  const statusLabels = {
    pending: t("toolCall.status.pending"),
    running: t("toolCall.status.running"),
    completed: t("toolCall.status.completed"),
  }

  return (
    <div className="px-3 py-1">
      <button
        onClick={() => { if (hasDetails) setIsExpanded(!isExpanded) }}
        className={cn(
          "w-full flex items-center gap-2 rounded text-left transition-colors",
          hasDetails && "hover:bg-muted-30"
        )}
        aria-label={hasDetails ? t("toolCall.toggleDetails") : undefined}
      >
        <div className={cn(
          "h-3.5 w-3.5 rounded-full flex items-center justify-center shrink-0",
          status === "completed" && "text-accent-emerald",
          status === "running" && "text-warning",
          status === "pending" && "text-muted-foreground"
        )}>
          {status === "running" ? (
            <Loader2 className="h-3 w-3 animate-spin" />
          ) : status === "completed" ? (
            <CheckCircle2 className="h-3.5 w-3.5" />
          ) : (
            <div className="h-1.5 w-1.5 rounded-full bg-muted-foreground" />
          )}
        </div>
        <span className={cn("text-sm", "truncate text-muted-foreground")}>{getToolDisplayName(toolCall.name, toolCall.arguments)}</span>
        {status === "running" && (
          <span className={cn(textMini, "px-1.5 py-0.5 rounded bg-warning-light text-warning shrink-0")}>
            {statusLabels[status]}
          </span>
        )}
        <div className="flex-1" />
        {hasDetails && (
          <ChevronDown className={cn(
            "h-4 w-4 text-muted-foreground transition-transform duration-200 shrink-0",
            isExpanded && "rotate-180"
          )} />
        )}
      </button>
      {isExpanded && hasDetails && (
        <div className="ml-5.5 mt-0.5 mb-1 space-y-1">
          {hasArguments && (
            <div className="rounded bg-muted-30 p-1.5">
              <div className={cn(textMini, "text-muted-foreground mb-0.5 flex items-center gap-1")}>
                <Code className="h-3 w-3" />{t("toolCall.arguments")}
              </div>
              <pre className={cn(textCode, "font-mono text-muted-foreground whitespace-pre-wrap break-words leading-relaxed")}>
                {formatJson(toolCall.arguments)}
              </pre>
            </div>
          )}
          {hasResult && isBuildCard ? (
            <BuildCard response={buildResponse} />
          ) : hasResult && (
            <div className="rounded bg-muted-30 p-1.5">
              <div className={cn(textMini, "text-muted-foreground mb-0.5 flex items-center gap-1")}>
                <CheckCircle2 className="h-3 w-3" />{t("toolCall.result")}
              </div>
              <pre className={cn(textCode, "font-mono text-muted-foreground whitespace-pre-wrap break-words max-h-32 overflow-y-auto leading-relaxed")}>
                {formatJson(toolCall.result)}
              </pre>
            </div>
          )}
        </div>
      )}
    </div>
  )
}

// ─── Legacy exports (keep for backward compat if other files import them) ───

export function PerRoundBlocks(_props: any) {
  return null // Deprecated: use ToolProcessBlock + MarkdownMessage instead
}

export function ToolCallVisualization(_props: any) {
  return null // Deprecated: use ToolProcessBlock instead
}
