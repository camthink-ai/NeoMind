/**
 * Rules List - Using ResponsiveTable for consistent styling
 */

import { useState, useEffect } from "react"
import { Switch } from "@/components/ui/switch"
import { Badge } from "@/components/ui/badge"
import { Card } from "@/components/ui/card"
import { DropdownMenu, DropdownMenuTrigger, DropdownMenuContent, DropdownMenuItem } from "@/components/ui/dropdown-menu"
import { IconButton } from "@/components/ui/button"
import {
  FullScreenDialog,
  FullScreenDialogHeader,
  FullScreenDialogContent,
  FullScreenDialogMain,
} from "@/components/automation/dialog"
import { ResponsiveTable, EmptyState, Pagination } from "@/components/shared"
import { Edit, Play, Trash2, Bell, Sparkles, Zap, MoreVertical, Timer, History, CheckCircle2, XCircle, Clock, Download } from "lucide-react"
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
const ACTION_CONFIG: Record<string, { icon: typeof Zap; label: string; color: string }> = {
  execute: { icon: Zap, label: 'automation:ruleBuilder.actionType.execute', color: 'text-warning bg-warning-light border-warning' },
  notify: { icon: Bell, label: 'automation:ruleBuilder.actionType.notify', color: 'text-info bg-info-light border-info' },
  trigger_agent: { icon: Sparkles, label: 'automation:ruleBuilder.actionType.triggerAgent', color: 'text-accent-purple bg-accent-purple-light border-accent-purple-light' },
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
    { key: 'time', label: t('automation:time', 'Time'), width: '35%' },
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
                    icon={<History className="h-12 w-12" />}
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
function hasForClause(rule: Rule): boolean {
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
  const setPage = onPageChange ?? setInternalPage

  const totalPages = Math.ceil(rules.length / ITEMS_PER_PAGE) || 1
  const startIndex = (page - 1) * ITEMS_PER_PAGE
  const endIndex = startIndex + ITEMS_PER_PAGE
  const paginatedRules = propsPaginatedRules ?? rules.slice(startIndex, endIndex)

  const content = (
    isMobile ? (
      <div className="space-y-2">
        {paginatedRules.length === 0 ? (
          <EmptyState
            icon={<Sparkles className="h-12 w-12" />}
            title={t('automation:emptyRules.title', 'No rules')}
            description={t('automation:emptyRules.description', 'Create your first rule to automate actions based on conditions')}
          />
        ) : null}
        {paginatedRules.map((rule) => {
          const actions = rule.actions && rule.actions.length > 0
            ? rule.actions
            : parseActionsFromDSL(rule.dsl_preview)
          const condition = formatConditionDisplay(rule)
          const hasTriggered = rule.last_triggered && rule.last_triggered !== '-'
          const forClause = parseForClause(rule)

          return (
            <Card
              key={rule.id}
              className={cn(
                "overflow-hidden border-border shadow-sm cursor-pointer active:scale-[0.99] transition-all",
                !rule.enabled && "opacity-50"
              )}
              onClick={() => onView(rule)}
            >
              <div className="px-3 py-2.5">
                {/* Row 1: icon + name + switch + actions */}
                <div className="flex items-center gap-2.5">
                  <div className={cn(
                    "w-8 h-8 rounded-lg flex items-center justify-center shrink-0",
                    rule.enabled ? "bg-warning-light text-warning" : "bg-muted text-muted-foreground"
                  )}>
                    <Sparkles className="h-4 w-4" />
                  </div>
                  <div className="flex-1 min-w-0">
                    <div className="font-medium text-sm truncate">{rule.name}</div>
                  </div>
                  <Switch
                    checked={rule.enabled}
                    onCheckedChange={() => onToggleStatus(rule)}
                    className="scale-75"
                    onClick={(e) => e.stopPropagation()}
                  />
                  <DropdownMenu>
                    <DropdownMenuTrigger asChild onClick={(e) => e.stopPropagation()}>
                      <IconButton>
                        <MoreVertical className="h-4 w-4" />
                      </IconButton>
                    </DropdownMenuTrigger>
                    <DropdownMenuContent align="end">
                      <DropdownMenuItem onClick={(e) => { e.stopPropagation(); onEdit(rule) }}>
                        <Edit className="h-4 w-4 mr-2" />
                        {t('common:edit')}
                      </DropdownMenuItem>
                      <DropdownMenuItem onClick={(e) => { e.stopPropagation(); onExecute(rule) }}>
                        <Play className="h-4 w-4 mr-2" />
                        {t('automation:execute')}
                      </DropdownMenuItem>
                      <DropdownMenuItem onClick={(e) => { e.stopPropagation(); handleExportRule(rule) }}>
                        <Download className="h-4 w-4 mr-2" />
                        {t('common:export')}
                      </DropdownMenuItem>
                      <DropdownMenuItem onClick={(e) => { e.stopPropagation(); setHistoryRule(rule); setShowHistory(true) }}>
                        <History className="h-4 w-4 mr-2" />
                        {t('automation:executionHistory', 'History')}
                      </DropdownMenuItem>
                      <DropdownMenuItem
                        className="text-error"
                        onClick={(e) => { e.stopPropagation(); onDelete(rule) }}
                      >
                        <Trash2 className="h-4 w-4 mr-2" />
                        {t('common:delete')}
                      </DropdownMenuItem>
                    </DropdownMenuContent>
                  </DropdownMenu>
                </div>
                {/* Row 2: condition + action badges + last triggered */}
                <div className="mt-1.5 ml-[42px]">
                  <div className="flex items-center gap-1.5 flex-wrap">
                    <code className={cn(textMini, "font-mono bg-muted px-1.5 py-0.5 rounded truncate max-w-[180px]")}>
                      {condition.text}
                    </code>
                    {forClause && (
                      <Badge variant="outline" className={cn(textMini, "h-5 px-1.5 gap-0.5 text-info border-info")}>
                        <Timer className="h-3 w-3" />
                        {forClause.duration}{forClause.unit}
                      </Badge>
                    )}
                    {actions.slice(0, 2).map((action, i) => {
                      const config = ACTION_CONFIG[action.type] || ACTION_CONFIG.execute
                      return (
                        <Badge key={i} variant="outline" className={cn(textMini, "h-5 px-1.5 gap-0.5", config.color)}>
                          {t(config.label)}
                        </Badge>
                      )
                    })}
                    {actions.length > 2 && (
                      <span className={cn(textMini, "text-muted-foreground")}>+{actions.length - 2}</span>
                    )}
                    <span className={cn(textMini, "text-muted-foreground ml-auto")}>
                      {hasTriggered ? formatTimestamp(rule.last_triggered) : t('automation:never', 'Never')}
                    </span>
                  </div>
                </div>
              </div>
            </Card>
          )
        })}
      </div>
    ) : (
    <ResponsiveTable
      columns={[
        {
          key: 'name',
          label: t('automation:ruleName'),
          width: '24%',
        },
        {
          key: 'trigger',
          label: t('automation:trigger'),
          width: '22%',
        },
        {
          key: 'actions',
          label: t('automation:ruleBuilder.executeActions'),
          width: '18%',
        },
        {
          key: 'createdAt',
          label: t('common:createdAt', 'Created'),
          width: '14%',
        },
        {
          key: 'lastTriggered',
          label: t('automation:lastTriggered'),
          width: '14%',
        },
        {
          key: 'status',
          label: t('automation:status'),
          width: '8%',
        },
      ]}
      data={paginatedRules}
      rowKey={(rule: Rule) => rule.id}
      loading={loading}
      onRowClick={(rowData: Rule) => onView(rowData)}
      getRowClassName={(rowData) => {
        const rule = rowData
        return cn(!rule.enabled && "opacity-50")
      }}
      renderCell={(columnKey, rowData) => {
        const rule = rowData

        switch (columnKey) {
          case 'name':
            return (
              <div className="flex items-center gap-3">
                <div className={cn(
                  "w-9 h-9 rounded-lg flex items-center justify-center transition-colors shrink-0",
                  rule.enabled ? "bg-warning-light text-warning" : "bg-muted text-muted-foreground"
                )}>
                  <Sparkles className="h-4 w-4" />
                </div>
                <div className="min-w-0">
                  <div className="font-medium text-sm truncate">{rule.name}</div>
                  <div className="text-xs text-muted-foreground line-clamp-1">
                    {rule.description || '-'}
                  </div>
                </div>
              </div>
            )

          case 'trigger': {
            const condition = formatConditionDisplay(rule)
            const forClause = parseForClause(rule)

            return (
              <div className="flex items-center gap-2 min-w-0">
                <code className="text-xs font-mono bg-muted px-2 py-1 rounded-md truncate block" title={condition.full}>
                  {condition.text === '-' ? <span className="text-muted-foreground">-</span> : condition.text}
                </code>
                {forClause && (
                  <Badge variant="outline" className="text-xs gap-1 shrink-0 text-info border-info">
                    <Timer className="h-3 w-3" />
                    {forClause.duration}{forClause.unit}
                  </Badge>
                )}
              </div>
            )
          }

          case 'actions': {
            const actions = rule.actions && rule.actions.length > 0
              ? rule.actions
              : parseActionsFromDSL(rule.dsl_preview)
            const actionsCount = actions.length
            const firstActions = actions.slice(0, 2)

            return actionsCount === 0 ? (
              <span className="text-muted-foreground text-sm">-</span>
            ) : (
              <div className="flex flex-wrap gap-1">
                {firstActions.map((action, i) => {
                  const config = ACTION_CONFIG[action.type] || ACTION_CONFIG.execute
                  const Icon = config.icon
                  return (
                    <Badge
                      key={i}
                      variant="outline"
                      className={cn("text-xs gap-1", config.color)}
                    >
                      <Icon className="h-3 w-3" />
                      {t(config.label)}
                    </Badge>
                  )
                })}
                {actionsCount > 2 && (
                  <Badge variant="outline" className="text-xs bg-muted">
                    +{actionsCount - 2}
                  </Badge>
                )}
              </div>
            )
          }

          case 'createdAt':
            return (
              <span className="text-xs text-muted-foreground">
                {formatTimestamp(rule.created_at)}
              </span>
            )

          case 'lastTriggered': {
            const hasTriggered = rule.last_triggered && rule.last_triggered !== '-'
            const triggerCount = rule.trigger_count || 0

            return !hasTriggered ? (
              <span className="text-xs text-muted-foreground">-</span>
            ) : (
              <div className="flex flex-col gap-0.5">
                <span className="text-xs">{formatTimestamp(rule.last_triggered)}</span>
                {triggerCount > 1 && (
                  <span className="text-xs text-muted-foreground">
                    {triggerCount}x
                  </span>
                )}
              </div>
            )
          }

          case 'status':
            return (
              <Switch
                checked={rule.enabled}
                onCheckedChange={() => onToggleStatus(rule)}
                className="scale-90"
              />
            )

          default:
            return null
        }
      }}
      actions={[
        {
          label: t('common:edit'),
          icon: <Edit className="h-4 w-4" />,
          onClick: (rowData) => {
            const rule = rowData
            onEdit(rule)
          },
        },
        {
          label: t('automation:execute'),
          icon: <Play className="h-4 w-4" />,
          onClick: (rowData) => {
            const rule = rowData
            onExecute(rule)
          },
        },
        {
          label: t('common:export'),
          icon: <Download className="h-4 w-4" />,
          onClick: (rowData) => {
            const rule = rowData
            handleExportRule(rule)
          },
        },
        {
          label: t('automation:executionHistory', 'History'),
          icon: <History className="h-4 w-4" />,
          onClick: (rowData) => {
            const rule = rowData
            setHistoryRule(rule)
            setShowHistory(true)
          },
        },
        {
          label: t('common:delete'),
          icon: <Trash2 className="h-4 w-4" />,
          variant: 'destructive',
          onClick: (rowData) => {
            const rule = rowData
            onDelete(rule)
          },
        },
      ]}
      emptyState={
        <EmptyState
          icon={<Sparkles className="h-12 w-12" />}
          title={t('automation:emptyRules.title', 'No rules')}
          description={t('automation:emptyRules.description', 'Create your first rule to automate actions based on conditions')}
        />
      }
    />
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
