import { useState } from "react"
import {
  Activity,
  Bell,
  Bot,
  ChevronDown,
  Clock,
  Code2,
  Hand,
  History,
  Tag,
  Zap,
} from "lucide-react"
import type { Rule, RuleCondition, RuleExecutionResult, RuleExplanation } from "@/types"
import { cn } from "@/lib/utils"
import { textMini, textNano } from "@/design-system/tokens/typography"
import {
  RuleWhyNotTriggeredPanel,
  type ExplainLoadStatus,
} from "@/components/automation/RuleWhyNotTriggeredPanel"
import { Badge } from "@/components/ui/badge"
import {
  CopyIdPill,
  InfoCell,
  SegmentedTabs,
  formatDurationMs,
  formatLastSeen,
} from "./shared"

interface RuleInspectorViewProps {
  rule: Rule
  ruleExplain: RuleExplanation | null
  explainStatus: ExplainLoadStatus
  history: RuleExecutionResult[]
  entityId?: string
  isZh: boolean
  onRefresh: () => void
}

function formatRuleTrigger(trigger: Rule["trigger"] | null | undefined, isZh: boolean) {
  if (!trigger || !trigger.trigger_type) {
    return {
      Icon: Zap,
      badge: isZh ? "未知" : "Unknown",
      label: isZh ? "未知触发" : "Unknown trigger",
      detail: "—",
      sources: [] as string[],
    }
  }
  switch (trigger.trigger_type) {
    case "data_change":
      return {
        Icon: Activity,
        badge: isZh ? "数据变化" : "Data Change",
        label: trigger.sources?.length
          ? isZh
            ? `监听 ${trigger.sources.length} 个数据源更新`
            : `Watches ${trigger.sources.length} data source(s)`
          : isZh
            ? "数据更新时触发"
            : "Triggers on data updates",
        detail: "",
        sources: trigger.sources ?? [],
      }
    case "schedule":
      return {
        Icon: Clock,
        badge: isZh ? "定时" : "Schedule",
        label: isZh ? "按计划定时触发" : "Scheduled trigger",
        detail: trigger.cron || "—",
        sources: [] as string[],
      }
    case "manual":
      return {
        Icon: Hand,
        badge: isZh ? "手动" : "Manual",
        label: isZh ? "手动按需执行" : "Execute on demand",
        detail: "",
        sources: [] as string[],
      }
    default: {
      const unknownType = (trigger as { trigger_type?: unknown }).trigger_type
      return {
        Icon: Zap,
        badge: unknownType ? String(unknownType) : "—",
        label: unknownType ? String(unknownType) : isZh ? "未知触发" : "Unknown",
        detail: "",
        sources: [] as string[],
      }
    }
  }
}

function formatRuleAction(action: Rule["actions"][number], isZh: boolean) {
  const a = action as unknown as Record<string, unknown>
  const type = String(a.action_type || a.type || "action")
  switch (type) {
    case "notify":
    case "alert":
      return {
        Icon: Bell,
        label: isZh ? "发送通知" : "Notify",
        detail: String(a.message || a.title || a.channel_id || "—"),
      }
    case "device_command":
    case "command":
    case "execute":
      return {
        Icon: Zap,
        label: isZh ? "设备指令" : "Device Command",
        detail:
          [a.target || a.device_id, a.command].filter(Boolean).join(" · ") || "—",
      }
    case "agent":
    case "invoke_agent":
    case "trigger_agent":
    case "run_operator":
      return {
        Icon: Bot,
        label: isZh ? "调用代理" : "Invoke Agent",
        detail: String(a.agent_id || a.input || a.prompt || "—"),
      }
    default:
      return {
        Icon: Code2,
        label: type,
        detail: JSON.stringify(a).slice(0, 100),
      }
  }
}

function ConditionTreeNode({
  condition,
  isZh,
  depth = 0,
}: {
  condition: RuleCondition
  isZh: boolean
  depth?: number
}) {
  const kind = condition.condition_type as string

  if (kind === "none") {
    return (
      <div className="text-xs text-muted-foreground">
        {isZh ? "无附加条件（满足触发时机即执行）" : "No additional condition"}
      </div>
    )
  }

  if (kind === "comparison") {
    const rhs = condition.threshold ?? condition.threshold_value ?? "?"
    return (
      <div className="rounded-lg border border-border bg-muted-20 px-3 py-2 font-mono text-xs break-all text-foreground">
        {condition.source || "?"} {condition.operator || "?"} {String(rhs)}
      </div>
    )
  }

  if (kind === "range") {
    return (
      <div className="rounded-lg border border-border bg-muted-20 px-3 py-2 font-mono text-xs break-all text-foreground">
        {condition.source || "?"} ∈ [{condition.min ?? "?"}, {condition.max ?? "?"}]
      </div>
    )
  }

  if (kind === "logical") {
    const op = (condition.operator || "and").toUpperCase()
    const opLabel =
      op === "AND"
        ? isZh
          ? "AND 全部满足"
          : "AND (all)"
        : op === "OR"
          ? isZh
            ? "OR 满足任一"
            : "OR (any)"
          : op
    const children = condition.conditions ?? []

    return (
      <div className={cn("space-y-2", depth > 0 && "rounded-lg border border-border bg-muted-20 p-2.5")}>
        <div className="flex items-center gap-1.5">
          <span className={cn(textNano, "rounded border border-border bg-card px-1.5 py-0.5 font-mono font-medium text-foreground")}>
            {opLabel}
          </span>
        </div>
        {children.length > 0 && (
          <div className="space-y-2 border-l-2 border-border pl-2.5">
            {children.map((child, idx) => (
              <ConditionTreeNode key={idx} condition={child} isZh={isZh} depth={depth + 1} />
            ))}
          </div>
        )}
      </div>
    )
  }

  return (
    <div className="rounded-lg border border-border bg-muted-20 px-3 py-2 font-mono text-xs break-all text-foreground">
      {JSON.stringify(condition)}
    </div>
  )
}

export function RuleInspectorView({
  rule,
  ruleExplain,
  explainStatus,
  history,
  entityId,
  isZh,
  onRefresh,
}: RuleInspectorViewProps) {
  const [tab, setTab] = useState<"logic" | "history">("logic")
  const [expandedIdx, setExpandedIdx] = useState<number | null>(0)

  const trig = formatRuleTrigger(rule.trigger, isZh)
  const lastTriggeredLabel = formatLastSeen(rule.last_triggered)
  const cooldownLabel =
    rule.cooldown && rule.cooldown > 0
      ? isZh
        ? `${Math.round(rule.cooldown / 1000)} 秒`
        : `${Math.round(rule.cooldown / 1000)}s`
      : "—"

  return (
    <div className="space-y-4">
      {/* Hero Summary */}
      <div className="overflow-hidden rounded-xl border border-border bg-card">
        <div className="flex items-center justify-between gap-3 border-b border-border px-4 py-2.5">
          <span
            className={cn(
              "inline-flex items-center gap-1.5 rounded-md px-2 py-0.5 text-xs font-medium",
              rule.enabled ? "badge-success" : "bg-muted-50 text-muted-foreground"
            )}
          >
            <span
              className={cn(
                "h-1.5 w-1.5 rounded-full",
                rule.enabled ? "bg-success" : "bg-muted-foreground"
              )}
            />
            {rule.enabled ? (isZh ? "已启用" : "Enabled") : isZh ? "已停用" : "Disabled"}
          </span>
          {entityId && <CopyIdPill id={entityId} isZh={isZh} />}
        </div>
        <div className="grid grid-cols-3 gap-3 px-4 py-3">
          <InfoCell
            label={isZh ? "累计触发" : "Triggers"}
            value={isZh ? `${rule.trigger_count} 次` : String(rule.trigger_count)}
          />
          <InfoCell label={isZh ? "冷却时间" : "Cooldown"} value={cooldownLabel} />
          <InfoCell
            label={isZh ? "最近触发" : "Last Triggered"}
            value={lastTriggeredLabel || "—"}
          />
        </div>
      </div>

      {rule.tags && rule.tags.length > 0 && (
        <div className="flex flex-wrap items-center gap-1.5">
          <Tag className="h-3 w-3 shrink-0 text-muted-foreground" aria-hidden="true" />
          {rule.tags.map((tag) => (
            <Badge key={tag} variant="outline" className={textNano}>
              {tag}
            </Badge>
          ))}
        </div>
      )}

      <SegmentedTabs
        value={tab}
        onChange={setTab}
        items={[
          { id: "logic", label: isZh ? "规则逻辑" : "Rule Logic" },
          {
            id: "history",
            label: isZh ? "触发记录" : "History",
            count: history.length,
          },
        ]}
      />

      {tab === "logic" ? (
        <div className="space-y-4">
          <RuleWhyNotTriggeredPanel
            explanation={ruleExplain}
            status={explainStatus}
            onRetry={onRefresh}
          />

          {/* When -> If -> Then Vertical Flow */}
          <div className="relative space-y-3 pl-5">
            <div className="absolute bottom-4 left-[6px] top-4 w-px bg-border" aria-hidden="true" />

            {/* WHEN */}
            <div className="relative">
              <span
                className="absolute -left-5 top-3.5 h-3.5 w-3.5 rounded-full border-2 border-accent-orange bg-card"
                aria-hidden="true"
              />
              <div className="space-y-2 rounded-xl border border-border bg-card p-3.5">
                <div className="flex items-center justify-between gap-2">
                  <span className={cn(textMini, "font-semibold uppercase tracking-wider text-accent-orange")}>
                    {isZh ? "WHEN · 触发时机" : "WHEN · Trigger"}
                  </span>
                  <span className={cn(textMini, "text-muted-foreground")}>{trig.badge}</span>
                </div>
                <div className="text-sm font-medium text-foreground">{trig.label}</div>
                {trig.detail && (
                  <div className={cn(textMini, "font-mono text-muted-foreground break-all")}>
                    {trig.detail}
                  </div>
                )}
                {trig.sources.length > 0 && (
                  <div className="flex flex-wrap gap-1.5 pt-0.5">
                    {trig.sources.map((src) => (
                      <code
                        key={src}
                        className="rounded border border-border bg-muted-20 px-2 py-0.5 font-mono text-xs text-muted-foreground break-all"
                      >
                        {src}
                      </code>
                    ))}
                  </div>
                )}
              </div>
            </div>

            {/* IF */}
            {rule.condition && (
              <div className="relative">
                <span
                  className="absolute -left-5 top-3.5 h-3.5 w-3.5 rounded-full border-2 border-accent-cyan bg-card"
                  aria-hidden="true"
                />
                <div className="space-y-2.5 rounded-xl border border-border bg-card p-3.5">
                  <span className={cn(textMini, "block font-semibold uppercase tracking-wider text-accent-cyan")}>
                    {isZh ? "IF · 判断条件" : "IF · Condition"}
                  </span>
                  <ConditionTreeNode condition={rule.condition} isZh={isZh} />
                </div>
              </div>
            )}

            {/* THEN */}
            {rule.actions?.length > 0 && (
              <div className="relative">
                <span
                  className="absolute -left-5 top-3.5 h-3.5 w-3.5 rounded-full border-2 border-accent-emerald bg-card"
                  aria-hidden="true"
                />
                <div className="space-y-2 rounded-xl border border-border bg-card p-3.5">
                  <div className="flex items-center justify-between gap-2">
                    <span className={cn(textMini, "font-semibold uppercase tracking-wider text-accent-emerald")}>
                      {isZh ? "THEN · 执行动作" : "THEN · Actions"}
                    </span>
                    <span className={cn(textMini, "tabular-nums text-muted-foreground")}>
                      {isZh ? `${rule.actions.length} 个动作` : `${rule.actions.length}`}
                    </span>
                  </div>
                  <div className="space-y-2">
                    {rule.actions.map((action, idx) => {
                      const info = formatRuleAction(action, isZh)
                      const ActIcon = info.Icon
                      return (
                        <div
                          key={idx}
                          className="flex items-start gap-2.5 rounded-lg border border-border bg-muted-20 px-3 py-2"
                        >
                          <ActIcon
                            className="mt-0.5 h-3.5 w-3.5 shrink-0 text-muted-foreground"
                            aria-hidden="true"
                          />
                          <div className="min-w-0 flex-1">
                            <div className="text-xs font-medium text-foreground">{info.label}</div>
                            <div
                              className={cn(textMini, "mt-0.5 break-words text-muted-foreground")}
                              title={info.detail}
                            >
                              {info.detail}
                            </div>
                          </div>
                        </div>
                      )
                    })}
                  </div>
                </div>
              </div>
            )}
          </div>
        </div>
      ) : history.length === 0 ? (
        <div className="flex flex-col items-center justify-center gap-1.5 rounded-xl border border-border bg-muted-20 px-4 py-6 text-center">
          <History className="h-5 w-5 text-muted-foreground opacity-60" aria-hidden="true" />
          <p className="text-sm font-medium text-foreground">
            {isZh ? "暂无触发记录" : "No trigger history yet"}
          </p>
          <p className={cn(textMini, "text-muted-foreground")}>
            {isZh ? "规则满足条件并执行后会在此记录明细" : "Execution records appear here after the rule fires"}
          </p>
        </div>
      ) : (
        <div className="space-y-2">
          {history.slice(0, 10).map((item, idx) => {
            const open = expandedIdx === idx
            const timeStr = formatLastSeen(item.triggered_at) || item.triggered_at
            const summary = item.success
              ? isZh
                ? `执行成功 · ${item.actions_executed.length} 个动作`
                : `Succeeded · ${item.actions_executed.length} action(s)`
              : item.error || (isZh ? "执行失败" : "Execution failed")

            return (
              <div
                key={`${item.triggered_at}-${idx}`}
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
                        item.success ? "bg-success" : "bg-error"
                      )}
                    />
                    <div className="min-w-0">
                      <div className="truncate text-xs font-medium text-foreground">{summary}</div>
                      <div className={cn(textMini, "tabular-nums text-muted-foreground")}>
                        {timeStr} · {formatDurationMs(item.duration_ms)}
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
                {open && (
                  <div className="space-y-1.5 border-t border-border bg-muted-20 px-3.5 py-2.5 text-xs">
                    {item.actions_executed.length > 0 && (
                      <div className="flex flex-wrap gap-1.5">
                        {item.actions_executed.map((act, aIdx) => (
                          <span
                            key={aIdx}
                            className="rounded border border-border bg-card px-2 py-0.5 font-mono text-xs text-muted-foreground"
                          >
                            {act}
                          </span>
                        ))}
                      </div>
                    )}
                    {item.error && (
                      <p className="break-words text-error">{item.error}</p>
                    )}
                  </div>
                )}
              </div>
            )
          })}
        </div>
      )}
    </div>
  )
}
