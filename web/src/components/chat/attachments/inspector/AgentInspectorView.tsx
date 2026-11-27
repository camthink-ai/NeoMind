import { useState } from "react"
import { Sparkles, ChevronDown, Clock, History } from "lucide-react"
import type {
  AgentExecution,
  AgentExecutionDetail,
  AgentSpec,
  AiAgentDetail,
} from "@/types"
import { cn } from "@/lib/utils"
import { textMini, textNano } from "@/design-system/tokens/typography"
import { ConclusionContent } from "@/pages/agents-components/ConclusionContent"
import { Badge } from "@/components/ui/badge"
import {
  CopyIdPill,
  InfoCell,
  SegmentedTabs,
  formatDurationMs,
  formatLastSeen,
} from "./shared"

interface AgentInspectorViewProps {
  agent: AiAgentDetail & AgentSpec
  latestExec: AgentExecutionDetail | null
  executions: AgentExecution[]
  fallbackStatusLabel?: string
  entityId?: string
  isZh: boolean
}

function formatAgentSchedule(agent: AiAgentDetail & AgentSpec, isZh: boolean): string {
  const sched = agent.schedule
  if (sched) {
    if (sched.schedule_type === "interval" && sched.interval_seconds) {
      const s = sched.interval_seconds
      if (s % 3600 === 0) return isZh ? `每 ${s / 3600} 小时` : `Every ${s / 3600}h`
      if (s % 60 === 0) return isZh ? `每 ${s / 60} 分钟` : `Every ${s / 60}m`
      return isZh ? `每 ${s} 秒` : `Every ${s}s`
    }
    if (sched.schedule_type === "cron" && sched.cron_expression) {
      return sched.cron_expression
    }
    if (sched.schedule_type === "event") {
      return isZh ? "事件触发" : "Event-driven"
    }
    if (sched.schedule_type === "manual") {
      return isZh ? "手动触发" : "Manual"
    }
  }
  const trig = agent.trigger as unknown as Record<string, unknown> | undefined
  if (trig && typeof trig === "object") {
    const kind = String(trig.kind || trig.type || "")
    if (kind === "interval" && typeof trig.every_secs === "number") {
      const s = trig.every_secs
      if (s % 60 === 0) return isZh ? `每 ${s / 60} 分钟` : `Every ${s / 60}m`
      return isZh ? `每 ${s} 秒` : `Every ${s}s`
    }
    if (kind === "cron" && typeof trig.expr === "string") return trig.expr
    if (kind === "event") return isZh ? "事件触发" : "Event-driven"
    if (kind === "manual") return isZh ? "手动触发" : "Manual"
  }
  return "—"
}

export function AgentInspectorView({
  agent,
  latestExec,
  executions,
  fallbackStatusLabel,
  entityId,
  isZh,
}: AgentInspectorViewProps) {
  const [tab, setTab] = useState<"latest" | "config">("latest")
  const [expandedIdx, setExpandedIdx] = useState<number | null>(0)

  const latestExecObj = latestExec as unknown as {
    status?: string
    timestamp?: string | number
    duration_ms?: number
    conclusion?: string | null
    brief_text?: string | null
    result?: { conclusion?: string; summary?: string } | null
  } | null

  const conclusionText =
    latestExecObj?.conclusion ||
    latestExecObj?.brief_text ||
    latestExecObj?.result?.conclusion ||
    latestExecObj?.result?.summary ||
    null

  const statusLower = String(agent.status || "").toLowerCase()
  const scheduleLabel = formatAgentSchedule(agent, isZh)
  const successRate =
    agent.execution_count > 0 && typeof agent.success_count === "number"
      ? `${Math.round((agent.success_count / agent.execution_count) * 100)}%`
      : null
  const execSummary =
    agent.execution_count != null
      ? isZh
        ? `${agent.execution_count} 次${successRate ? ` (${successRate})` : ""}`
        : `${agent.execution_count} runs${successRate ? ` (${successRate})` : ""}`
      : "—"

  const resources = agent.resources ?? []

  return (
    <div className="space-y-4">
      {/* Hero Summary */}
      <div className="overflow-hidden rounded-xl border border-border bg-card">
        <div className="flex items-center justify-between gap-3 border-b border-border px-4 py-2.5">
          <span className="inline-flex items-center gap-1.5 rounded-md bg-muted-50 px-2 py-0.5 text-xs font-medium text-foreground">
            <span
              className={cn(
                "h-1.5 w-1.5 rounded-full",
                statusLower === "executing"
                  ? "bg-info motion-safe:animate-pulse"
                  : statusLower === "error"
                    ? "bg-error"
                    : statusLower === "paused" || statusLower === "stopped"
                      ? "bg-muted-foreground"
                      : "bg-success"
              )}
            />
            {String(agent.status || fallbackStatusLabel || "—")}
          </span>
          {entityId && <CopyIdPill id={entityId} isZh={isZh} />}
        </div>
        <div className="grid grid-cols-2 gap-x-4 gap-y-2.5 px-4 py-3">
          <InfoCell label={isZh ? "调度计划" : "Schedule"} value={scheduleLabel} />
          <InfoCell label={isZh ? "累计执行" : "Executions"} value={execSummary} />
        </div>
      </div>

      <SegmentedTabs
        value={tab}
        onChange={setTab}
        items={[
          { id: "latest", label: isZh ? "最新结论" : "Latest Result" },
          {
            id: "config",
            label: isZh ? "配置与历史" : "Config & History",
            count: executions.length > 0 ? executions.length : undefined,
          },
        ]}
      />

      {tab === "latest" ? (
        conclusionText ? (
          <div className="overflow-hidden rounded-xl border border-border bg-card">
            <div className="flex items-center justify-between gap-2 border-b border-border bg-muted-20 px-3.5 py-2 text-xs">
              <div className="flex items-center gap-2 min-w-0">
                <span className="font-medium text-foreground">
                  {isZh ? "最近一次执行结论" : "Latest Conclusion"}
                </span>
                {latestExecObj?.status && (
                  <Badge variant="secondary" className={textNano}>
                    {latestExecObj.status}
                  </Badge>
                )}
              </div>
              <div className="flex shrink-0 items-center gap-1.5 tabular-nums text-muted-foreground">
                {latestExecObj?.timestamp && (
                  <span>{formatLastSeen(latestExecObj.timestamp) || ""}</span>
                )}
                {latestExecObj?.duration_ms != null && (
                  <span>· {formatDurationMs(latestExecObj.duration_ms)}</span>
                )}
              </div>
            </div>
            <div className="max-h-96 overflow-y-auto p-3.5">
              <ConclusionContent content={conclusionText} />
            </div>
          </div>
        ) : (
          <div className="flex flex-col items-center justify-center gap-1.5 rounded-xl border border-border bg-muted-20 px-4 py-6 text-center">
            <Sparkles className="h-5 w-5 text-muted-foreground opacity-60" strokeWidth={1.75} aria-hidden="true" />
            <p className="text-sm font-medium text-foreground">
              {isZh ? "暂无执行结论" : "No executions yet"}
            </p>
            <p className={cn(textMini, "text-muted-foreground")}>
              {isZh
                ? "代理完成首次执行后，结构化结果与巡检报告将在此展示"
                : "Structured output and reports will appear here after the first run"}
            </p>
          </div>
        )
      ) : (
        <div className="space-y-4">
          {/* Prompt & Bound Resources */}
          {(agent.user_prompt || agent.description || resources.length > 0) && (
            <div className="space-y-2.5 rounded-xl border border-border bg-card p-3.5 text-xs">
              <div className="font-medium text-foreground">
                {isZh ? "任务目标" : "Prompt & Goal"}
              </div>
              <p className="whitespace-pre-wrap leading-relaxed text-muted-foreground">
                {agent.user_prompt || agent.description}
              </p>
              {resources.length > 0 && (
                <div className="flex flex-wrap items-center gap-1.5 pt-1">
                  <span className="text-muted-foreground">
                    {isZh ? "关联资源：" : "Resources:"}
                  </span>
                  {resources.map((res, idx) => (
                    <span
                      key={`${res.resource_id}-${idx}`}
                      className="rounded border border-border bg-muted-20 px-2 py-0.5 text-foreground"
                    >
                      {res.name || res.resource_id}
                    </span>
                  ))}
                </div>
              )}
            </div>
          )}

          {/* Execution History Timeline */}
          <div className="space-y-2">
            <span className={cn(textMini, "block font-medium uppercase tracking-wider text-muted-foreground")}>
              {isZh ? "最近执行记录" : "Recent Executions"}
            </span>
            {executions.length === 0 ? (
              <div className="flex flex-col items-center justify-center gap-1.5 rounded-xl border border-border bg-muted-20 px-4 py-5 text-center">
                <History className="h-4 w-4 text-muted-foreground opacity-60" aria-hidden="true" />
                <p className={cn(textMini, "text-muted-foreground")}>
                  {isZh ? "暂无历史执行记录" : "No execution history"}
                </p>
              </div>
            ) : (
              <div className="space-y-2">
                {executions.map((exec, idx) => {
                  const open = expandedIdx === idx
                  const ok = exec.status === "Completed"
                  const failed = exec.status === "Failed"
                  const timeStr = formatLastSeen(exec.timestamp) || exec.timestamp
                  const brief = exec.error || exec.conclusion || exec.status

                  return (
                    <div
                      key={exec.id || idx}
                      className="overflow-hidden rounded-xl border border-border bg-card"
                    >
                      <button
                        type="button"
                        onClick={() => setExpandedIdx(open ? null : idx)}
                        className="flex w-full min-h-0 items-center justify-between gap-2 px-3.5 py-2.5 text-left transition-colors hover:bg-muted-20"
                      >
                        <div className="flex min-w-0 items-center gap-2">
                          <span
                            className={cn(
                              "h-2 w-2 shrink-0 rounded-full",
                              ok ? "bg-success" : failed ? "bg-error" : "bg-warning"
                            )}
                          />
                          <div className="min-w-0">
                            <div className="truncate text-xs font-medium text-foreground">
                              {brief}
                            </div>
                            <div className={cn(textMini, "inline-flex items-center gap-1 tabular-nums text-muted-foreground")}>
                              <Clock className="h-3 w-3" aria-hidden="true" />
                              {timeStr} · {formatDurationMs(exec.duration_ms)}
                            </div>
                          </div>
                        </div>
                        <ChevronDown
                          className={cn(
                            "h-3.5 w-3.5 shrink-0 text-muted-foreground transition-transform",
                            open && "rotate-180"
                          )}
                          aria-hidden="true"
                        />
                      </button>
                      {open && (exec.conclusion || exec.error) && (
                        <div className="border-t border-border bg-muted-20 px-3.5 py-2.5 text-xs">
                          {exec.error ? (
                            <p className="break-words text-error">{exec.error}</p>
                          ) : (
                            <ConclusionContent content={exec.conclusion || ""} />
                          )}
                        </div>
                      )}
                    </div>
                  )
                })}
              </div>
            )}
          </div>
        </div>
      )}
    </div>
  )
}
