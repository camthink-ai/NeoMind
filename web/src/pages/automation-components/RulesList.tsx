/**
 * Rules List - Using ResponsiveTable for consistent styling
 */

import { useState, useEffect } from "react"
import { Switch } from "@/components/ui/switch"
import { Badge } from "@/components/ui/badge"
import { Card } from "@/components/ui/card"
import { IconButton } from "@/components/ui/button"
import { Skeleton } from "@/components/ui/skeleton"
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu"
import {
  FullScreenDialog,
  FullScreenDialogHeader,
  FullScreenDialogContent,
  FullScreenDialogMain,
} from "@/components/automation/dialog"
import { ResponsiveTable, EmptyState, EmptyStateCard, Pagination } from "@/components/shared"
import { Edit, Play, Trash2, Bell, Sparkles, Zap, Timer, History, CheckCircle2, XCircle, Clock, Download, MoreVertical, ArrowRight } from "lucide-react"
import { useTranslation } from "react-i18next"
import type { Rule, RuleAction, RuleExecutionResult } from "@/types"
import { cn } from "@/lib/utils"
import { textMini } from "@/design-system/tokens/typography"
import { formatTimestamp } from "@/lib/utils/format"
import { useIsMobile } from "@/hooks/useMobile"
import { useToast } from "@/hooks/use-toast"
import { api } from "@/lib/api"

interface RulesListProps {
  rules: Rule[]
  loading: boolean
  paginatedRules?: Rule[]
  page?: number
  onPageChange?: (page: number) => void
  onView: (rule: Rule) => void
  onEdit: (rule: Rule) => void
  onDelete: (rule: Rule) => void
  onToggleStatus: (rule: Rule) => void
  onExecute: (rule: Rule) => void
}

// Action configuration for display
const ACTION_CONFIG: Record<string, { icon: typeof Zap; label: string }> = {
  execute: { icon: Zap, label: 'automation:ruleBuilder.actionType.execute' },
  notify: { icon: Bell, label: 'automation:ruleBuilder.actionType.notify' },
  trigger_agent: { icon: Sparkles, label: 'automation:ruleBuilder.actionType.triggerAgent' },
  run_operator: { icon: Sparkles, label: 'automation:ruleBuilder.actionType.runOperator' },
}

function listSubtitle(value?: string | null): string | null {
  const text = value?.trim()
  if (!text || text === '-') return null
  return text
}

export const ITEMS_PER_PAGE = 10

// ============================================================================
// Rule History Dialog (matches DeviceDetail metric history pattern)
// ============================================================================

function RuleHistoryDialog({ rule, open, onOpenChange }: {
  rule: Rule | null
  open: boolean
  onOpenChange: (open: boolean) => void
}) {
  const { t } = useTranslation(['automation', 'common'])
  const isMobile = useIsMobile()
  const [history, setHistory] = useState<RuleExecutionResult[]>([])
  const [loading, setLoading] = useState(false)
  const [page, setPage] = useState(1)
  const pageSize = ITEMS_PER_PAGE

  const loadHistory = async () => {
    if (!rule) return
    setLoading(true)
    try {
      const data = await api.getRuleHistory(rule.id)
      setHistory(data.executions || [])
    } catch {
      setHistory([])
    } finally {
      setLoading(false)
    }
  }

  // Load history whenever the dialog opens or the selected rule changes.
  // FullScreenDialog only fires onOpenChange on close events (escape/backdrop),
  // so relying on handleOpenChange to trigger the load would never fire when
  // the parent flips `open` from false → true — the dialog would render with
  // an empty history list indefinitely.
  useEffect(() => {
    if (open && rule) {
      setPage(1) // Reset to first page on open / rule switch
      loadHistory()
    }
    if (!open) {
      // Reset on close so next open doesn't flash stale data
      setHistory([])
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open, rule?.id])

  // Slice for current page
  const totalPages = Math.ceil(history.length / pageSize) || 1
  const safePage = Math.min(page, totalPages)
  const pagedHistory = history.slice(
    (safePage - 1) * pageSize,
    safePage * pageSize,
  )

  const handleOpenChange = (nextOpen: boolean) => {
    onOpenChange(nextOpen)
  }

  const columns = [
    { key: 'time', label: t('automation:ruleMonitor.time', 'Time'), width: '35%' },
    { key: 'result', label: t('automation:result', 'Result'), width: '20%' },
    { key: 'detail', label: t('automation:detail', 'Detail'), width: '45%' },
  ]

  const renderCell = (columnKey: string, entry: RuleExecutionResult) => {
    switch (columnKey) {
      case 'time':
        return (
          <span className="text-sm text-muted-foreground">
            {formatTimestamp(entry.triggered_at)}
          </span>
        )
      case 'result':
        return (
          <div className="flex items-center gap-1.5">
            {entry.success ? (
              <CheckCircle2 className="h-4 w-4 text-success shrink-0" />
            ) : (
              <XCircle className="h-4 w-4 text-error shrink-0" />
            )}
            <Badge variant="outline" className={cn(textMini, "gap-1")}>
              <Clock className="h-3 w-3" />
              {entry.duration_ms}ms
            </Badge>
          </div>
        )
      case 'detail':
        return (
          <div className="space-y-1">
            {entry.actions_executed.length > 0 ? (
              <div className="flex flex-wrap gap-1">
                {entry.actions_executed.map((action, j) => (
                  <span key={j} className={cn(textMini, "bg-muted px-1.5 py-0.5 rounded")}>
                    {action}
                  </span>
                ))}
              </div>
            ) : (
              <span className={cn(textMini, "text-muted-foreground")}>-</span>
            )}
            {entry.error && (
              <p className={cn(textMini, "text-error")}>{entry.error}</p>
            )}
          </div>
        )
      default:
        return null
    }
  }

  return (
    <FullScreenDialog open={open} onOpenChange={handleOpenChange}>
      <FullScreenDialogHeader
        icon={<History className="h-5 w-5" />}
        iconBg="bg-accent-indigo-light"
        iconColor="text-accent-indigo"
        title={rule ? `${t('automation:executionHistory')} — ${rule.name}` : t('automation:executionHistory')}
        onClose={() => onOpenChange(false)}
      />
      <FullScreenDialogContent>
        <FullScreenDialogMain className="overflow-hidden">
          <div className="h-full flex flex-col">
            <div className={cn("flex-1 overflow-y-auto", isMobile ? "px-3 py-3" : "px-4 py-4")}>
              <ResponsiveTable
                columns={columns}
                data={pagedHistory}
                renderCell={renderCell}
                rowKey={(entry: RuleExecutionResult) => `${entry.triggered_at}-${entry.duration_ms}`}
                loading={loading}
                flexHeight={false}
                emptyState={
                  <EmptyState
                    title={t('automation:noHistory')}
                  />
                }
              />
            </div>
            {history.length > pageSize && (
              <div className={cn("border-t", isMobile ? "px-3 py-2" : "px-4 py-3")}>
                <Pagination
                  total={history.length}
                  pageSize={pageSize}
                  currentPage={safePage}
                  onPageChange={setPage}
                  hideOnMobile={false}
                />
              </div>
            )}
          </div>
        </FullScreenDialogMain>
      </FullScreenDialogContent>
    </FullScreenDialog>
  )
}

// Format condition for display
function formatConditionDisplay(rule: Rule): { text: string; full: string } {
  if (!rule.dsl_preview) return { text: '-', full: '-' }

  const whenMatch = rule.dsl_preview.match(/WHEN\s+(.+?)(?:\nFOR|\nDO|$)/s)
  if (whenMatch) {
    let condition = whenMatch[1].trim()
    // Simplify common patterns for better readability
    condition = condition
      .replace(/\s+/g, ' ')  // Collapse multiple spaces
      .replace(/ > /g, '>')  // Clean up operators
      .replace(/ < /g, '<')
      .replace(/ >= /g, '>=')
      .replace(/ <= /g, '<=')
      .replace(/ == /g, '=')
      .replace(/ != /g, '≠')

    const full = condition
    // Truncate for display
    let text = condition
    if (text.length > 45) {
      text = text.substring(0, 42) + '...'
    }
    return { text, full }
  }
  return { text: '-', full: '-' }
}

// Parse FOR clause to get duration
function parseForClause(rule: Rule): { duration: number; unit: string } | null {
  if (!rule.dsl_preview) return null
  const forMatch = rule.dsl_preview.match(/FOR\s+(\d+)(ms|min|s|m|h)\b/)
  if (forMatch) {
    const duration = parseInt(forMatch[1], 10)
    const unit = forMatch[2]
    return { duration, unit }
  }
  return null
}

// Check if rule has FOR clause
function _hasForClause(rule: Rule): boolean {
  return rule.dsl_preview?.includes('\nFOR ') || false
}

// Parse actions from DSL preview (matches preview.rs render_action format)
function parseActionsFromDSL(dslPreview?: string): RuleAction[] {
  if (!dslPreview) return []
  const actions: RuleAction[] = []
  const doMatch = dslPreview.match(/\nDO\n(.*?)\nEND/s)
  if (!doMatch) return actions

  const actionLines = doMatch[1].trim().split('\n').map(l => l.trim().replace(/^ {4}/, ''))

  for (const line of actionLines) {
    if (!line) continue

    // NOTIFY [SEVERITY] "message" — matches preview.rs render_action
    const notifyMatch = line.match(/^NOTIFY\s+\[(\w+)\]\s+"(.+)"$/)
    if (notifyMatch) {
      const sevMap: Record<string, string> = { INFO: 'info', WARNING: 'warning', CRITICAL: 'critical', EMERGENCY: 'emergency' }
      actions.push({ type: 'notify', message: notifyMatch[2], severity: (sevMap[notifyMatch[1]] || 'info') as any })
      continue
    }

    // EXECUTE prefix.target command(params) — matches preview.rs render_action
    const execMatch = line.match(/^EXECUTE\s+(?:device|extension)\.(\S+)\s+(\w+)(?:\((.+)\))?$/)
    if (execMatch) {
      const [, targetId, command, paramsStr] = execMatch
      const params: Record<string, string> = {}
      if (paramsStr) {
        paramsStr.split(', ').forEach(p => {
          const [k, v] = p.split('=')
          if (k && v) params[k] = v
        })
      }
      actions.push({ type: 'execute', target: targetId, target_type: 'device', command, params })
      continue
    }

    // TRIGGER AGENT agent_id INPUT "text" — matches preview.rs render_action
    const agentMatch = line.match(/^TRIGGER AGENT\s+(\S+)(?:\s+INPUT\s+"([^"]*)")?/)
    if (agentMatch) {
      actions.push({ type: 'trigger_agent', agent_id: agentMatch[1], input: agentMatch[2] })
      continue
    }
  }

  return actions
}

export function RulesList({
  rules,
  loading,
  paginatedRules: propsPaginatedRules,
  page: propsPage,
  onPageChange,
  onView,
  onEdit,
  onDelete,
  onToggleStatus,
  onExecute,
}: RulesListProps) {
  const { t } = useTranslation(['common', 'automation'])
  const isMobile = useIsMobile()
  const { toast } = useToast()
  const [internalPage, setInternalPage] = useState(1)
  const [historyRule, setHistoryRule] = useState<Rule | null>(null)
  const [showHistory, setShowHistory] = useState(false)

  // Export a single rule as JSON. Strips frontend-only bookkeeping fields so
  // the file matches the backend Rule schema and can be re-imported cleanly.
  const handleExportRule = async (rule: Rule) => {
    try {
      const { _source, ...rest } = rule as Rule & { _source?: unknown }
      void _source
      const data = JSON.stringify(rest, null, 2)
      const blob = new Blob([data], { type: 'application/json' })
      const url = URL.createObjectURL(blob)
      const link = document.createElement('a')
      link.href = url
      const safeName = (rule.name || rule.id)
        .replace(/[^a-zA-Z0-9-_]+/g, '-')
        .replace(/-+/g, '-')
        .replace(/^-|-$/g, '')
        .slice(0, 64) || rule.id
      link.download = `rule-${safeName}.json`
      document.body.appendChild(link)
      link.click()
      document.body.removeChild(link)
      URL.revokeObjectURL(url)
      toast({ title: t('common:success'), description: `Exported ${rule.name}` })
    } catch {
      toast({ title: t('common:failed'), description: 'Failed to export rule', variant: 'destructive' })
    }
  }

  // Use props if provided, otherwise use internal state (backward compatibility)
  const page = propsPage ?? internalPage
  const _setPage = onPageChange ?? setInternalPage

  const _totalPages = Math.ceil(rules.length / ITEMS_PER_PAGE) || 1
  const startIndex = (page - 1) * ITEMS_PER_PAGE
  const endIndex = startIndex + ITEMS_PER_PAGE
  // Mobile infinite-scroll parents pass a cumulative window; if props are
  // omitted, mirror that here so footer load-more cannot replace-slice.
  const paginatedRules = propsPaginatedRules ?? (
    isMobile
      ? rules.slice(0, page * ITEMS_PER_PAGE)
      : rules.slice(startIndex, endIndex)
  )

  const content = (
    isMobile ? (
      <div className="space-y-2">
        {/* Mobile has no ResponsiveTable — carry its own loading skeleton. */}
        {loading && paginatedRules.length === 0 && (
          [0, 1, 2].map((i) => (
            <div key={i} className="rounded-2xl border border-border bg-card px-4 py-3.5 space-y-2">
              <div className="flex items-center justify-between gap-3">
                <Skeleton className="h-4 w-36" />
                <Skeleton className="h-5 w-9 rounded-full" />
              </div>
              <Skeleton className="h-3 w-52" />
            </div>
          ))
        )}
        {paginatedRules.length === 0 && !loading ? (
          <EmptyStateCard>
            <EmptyState
              title={t('automation:emptyRules.title', 'No rules')}
              description={t('automation:emptyRules.description', 'Create your first rule to automate actions based on conditions')}
            />
          </EmptyStateCard>
        ) : null}
        {paginatedRules.map((rule) => {
          const condition = formatConditionDisplay(rule)

          return (
            <Card
              key={rule.id}
              className={cn(
                "cursor-pointer overflow-hidden rounded-2xl border border-border bg-card shadow-none transition-colors active:bg-muted",
                !rule.enabled && "opacity-50"
              )}
              onClick={() => onView(rule)}
            >
              <div className="px-4 py-3.5">
                <div className="flex items-center gap-2">
                  <div className="flex-1 min-w-0">
                    <div className="truncate text-heading font-medium">{rule.name}</div>
                    <p className="mt-1 truncate text-xs text-muted-foreground" title={condition.full}>
                      {condition.text}
                    </p>
                  </div>
                  <Switch
                    checked={rule.enabled}
                    onCheckedChange={() => onToggleStatus(rule)}
                    onClick={(e) => e.stopPropagation()}
                  />
                </div>
              </div>
            </Card>
          )
        })}
      </div>
    ) : loading && paginatedRules.length === 0 ? (
      <div className="rounded-xl border border-border bg-card divide-y divide-glass-border overflow-hidden">
        {Array.from({ length: 6 }).map((_, i) => (
          <div key={i} className="flex items-center gap-4 px-5 py-3.5">
            <Skeleton className="h-9 w-9 rounded-lg shrink-0" />
            <div className="flex-1 space-y-2">
              <Skeleton className="h-4 w-48" />
              <div className="flex items-center gap-2">
                <Skeleton className="h-5 w-44 rounded-md" />
                <Skeleton className="h-5 w-20 rounded-md" />
              </div>
            </div>
            <Skeleton className="h-4 w-24 hidden lg:block" />
            <Skeleton className="h-5 w-9 rounded-full" />
          </div>
        ))}
      </div>
    ) : paginatedRules.length === 0 ? (
      <EmptyStateCard>
        <EmptyState
          title={t('automation:emptyRules.title', 'No rules')}
          description={t('automation:emptyRules.description', 'Create your first rule to automate actions based on conditions')}
        />
      </EmptyStateCard>
    ) : (
      <div className="rounded-xl border border-border bg-card divide-y divide-glass-border overflow-hidden">
        {paginatedRules.map((rule) => {
          const subtitle = listSubtitle(rule.description)
          const condition = formatConditionDisplay(rule)
          const forClause = parseForClause(rule)
          const actions = rule.actions && rule.actions.length > 0
            ? rule.actions
            : parseActionsFromDSL(rule.dsl_preview)
          const actionsCount = actions.length
          const firstActions = actions.slice(0, 2)
          const hasTriggered = rule.last_triggered && rule.last_triggered !== '-'
          const triggerCount = rule.trigger_count || 0

          return (
            <div
              key={rule.id}
              role="button"
              tabIndex={0}
              onClick={() => onView(rule)}
              onKeyDown={(e) => { if (e.key === 'Enter' || e.key === ' ') { e.preventDefault(); onView(rule) } }}
              className={cn(
                "group/row flex items-center gap-4 px-5 py-3.5 transition-colors duration-fast hover:bg-muted-20 cursor-pointer",
                !rule.enabled && "opacity-55"
              )}
            >
              {/* Icon */}
              <div className="flex h-9 w-9 shrink-0 items-center justify-center rounded-lg border border-border bg-muted-30 text-foreground">
                <Zap className="h-4 w-4" strokeWidth={1.75} />
              </div>

              {/* Main 2-line content: Name + WHEN -> THEN summary */}
              <div className="min-w-0 flex-1 space-y-1.5">
                <div className="flex items-baseline gap-2 min-w-0">
                  <span className="truncate text-sm font-medium text-foreground">{rule.name}</span>
                  {subtitle && (
                    <span className="truncate text-xs text-muted-foreground">{subtitle}</span>
                  )}
                </div>
                <div className="flex flex-wrap items-center gap-1.5 min-w-0">
                  <code
                    className="text-xs font-mono bg-muted-30 border border-glass-border px-2 py-0.5 rounded-md truncate max-w-[320px] text-foreground"
                    title={condition.full}
                  >
                    {condition.text === '-' ? <span className="text-muted-foreground">-</span> : condition.text}
                  </code>
                  {forClause && (
                    <Badge variant="outline" className="text-xs font-normal gap-1 shrink-0 bg-muted-20 border-glass-border text-muted-foreground">
                      <Timer className="h-3 w-3" />
                      {forClause.duration}{forClause.unit}
                    </Badge>
                  )}
                  {actionsCount > 0 && (
                    <>
                      <ArrowRight className="h-3 w-3 shrink-0 text-muted-foreground" aria-hidden="true" />
                      {firstActions.map((action, i) => {
                        const config = ACTION_CONFIG[action.type] || ACTION_CONFIG.execute
                        const Icon = config.icon
                        return (
                          <Badge
                            key={i}
                            variant="outline"
                            className="gap-1 text-xs font-normal bg-muted-20 border-glass-border text-muted-foreground"
                          >
                            <Icon className="h-3 w-3" />
                            {t(config.label)}
                          </Badge>
                        )
                      })}
                      {actionsCount > 2 && (
                        <Badge variant="outline" className="text-xs font-normal bg-muted-20 border-glass-border text-muted-foreground">
                          +{actionsCount - 2}
                        </Badge>
                      )}
                    </>
                  )}
                </div>
              </div>

              {/* Quiet right meta: last triggered / created with clear label */}
              <div className="hidden lg:flex flex-col items-end gap-0.5 shrink-0 text-xs tabular-nums text-muted-foreground">
                <span>
                  {hasTriggered
                    ? `${t('automation:lastTriggered', 'Triggered')} · ${formatTimestamp(rule.last_triggered)}`
                    : `${t('common:createdAt', 'Created')} · ${formatTimestamp(rule.created_at)}`}
                </span>
                {hasTriggered && triggerCount > 0 && (
                  <span className={cn(textMini, "text-muted-foreground")}>
                    {triggerCount}x
                  </span>
                )}
              </div>

              {/* Switch + hover-reveal actions */}
              <div className="flex items-center gap-2 shrink-0" onClick={(e) => e.stopPropagation()}>
                <Switch
                  checked={rule.enabled}
                  onCheckedChange={() => onToggleStatus(rule)}
                  className="scale-90"
                />
                <DropdownMenu>
                  <DropdownMenuTrigger asChild>
                    <IconButton
                      size="sm"
                      aria-label={t('common:actions', { defaultValue: 'Actions' })}
                      className="opacity-50 transition-opacity group-hover/row:opacity-100 focus-visible:opacity-100 data-[state=open]:opacity-100"
                    >
                      <MoreVertical className="h-4 w-4" />
                    </IconButton>
                  </DropdownMenuTrigger>
                  <DropdownMenuContent align="end" className="w-[160px]">
                    <DropdownMenuItem onClick={() => onEdit(rule)} className="gap-2">
                      <Edit className="h-4 w-4 shrink-0" />
                      <span>{t('common:edit')}</span>
                    </DropdownMenuItem>
                    <DropdownMenuItem onClick={() => onExecute(rule)} className="gap-2">
                      <Play className="h-4 w-4 shrink-0" />
                      <span>{t('automation:execute')}</span>
                    </DropdownMenuItem>
                    <DropdownMenuItem onClick={() => handleExportRule(rule)} className="gap-2">
                      <Download className="h-4 w-4 shrink-0" />
                      <span>{t('common:export')}</span>
                    </DropdownMenuItem>
                    <DropdownMenuItem onClick={() => { setHistoryRule(rule); setShowHistory(true) }} className="gap-2">
                      <History className="h-4 w-4 shrink-0" />
                      <span>{t('automation:executionHistory', 'History')}</span>
                    </DropdownMenuItem>
                    <DropdownMenuItem onClick={() => onDelete(rule)} className="gap-2 text-error focus:text-error">
                      <Trash2 className="h-4 w-4 shrink-0" />
                      <span>{t('common:delete')}</span>
                    </DropdownMenuItem>
                  </DropdownMenuContent>
                </DropdownMenu>
              </div>
            </div>
          )
        })}
      </div>
    )
  )

  return (
    <>
      {content}
      <RuleHistoryDialog
        rule={historyRule}
        open={showHistory}
        onOpenChange={setShowHistory}
      />
    </>
  )
}
