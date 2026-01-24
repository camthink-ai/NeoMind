/**
 * Rules List - Unified card-based table design
 */

import { useState } from "react"
import { Switch } from "@/components/ui/switch"
import { Button } from "@/components/ui/button"
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu"
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table"
import { Card } from "@/components/ui/card"
import { Badge } from "@/components/ui/badge"
import { EmptyStateInline, Pagination } from "@/components/shared"
import { Zap, Edit, Play, Trash2, MoreVertical, Bell, FileText, FlaskConical, AlertTriangle, Sparkles, Clock, CheckCircle2, Timer } from "lucide-react"
import { useTranslation } from "react-i18next"
import type { Rule, RuleAction } from "@/types"
import { cn } from "@/lib/utils"

interface RulesListProps {
  rules: Rule[]
  loading: boolean
  onEdit: (rule: Rule) => void
  onDelete: (rule: Rule) => void
  onToggleStatus: (rule: Rule) => void
  onExecute: (rule: Rule) => void
}

// Action configuration for display
const ACTION_CONFIG: Record<string, { icon: typeof Zap; label: string; color: string }> = {
  Execute: { icon: Zap, label: 'automation:ruleBuilder.actionType.execute', color: 'text-yellow-700 bg-yellow-50 border-yellow-200 dark:text-yellow-400 dark:bg-yellow-950/30 dark:border-yellow-800' },
  Notify: { icon: Bell, label: 'automation:ruleBuilder.actionType.notify', color: 'text-blue-700 bg-blue-50 border-blue-200 dark:text-blue-400 dark:bg-blue-950/30 dark:border-blue-800' },
  Log: { icon: FileText, label: 'automation:ruleBuilder.actionType.log', color: 'text-gray-700 bg-gray-50 border-gray-200 dark:text-gray-400 dark:bg-gray-800 dark:border-gray-700' },
  Set: { icon: FlaskConical, label: 'automation:ruleBuilder.actionType.set', color: 'text-purple-700 bg-purple-50 border-purple-200 dark:text-purple-400 dark:bg-purple-950/30 dark:border-purple-800' },
  Delay: { icon: Timer, label: 'automation:ruleBuilder.actionType.delay', color: 'text-orange-700 bg-orange-50 border-orange-200 dark:text-orange-400 dark:bg-orange-950/30 dark:border-orange-800' },
  CreateAlert: { icon: AlertTriangle, label: 'automation:ruleBuilder.actionType.createAlert', color: 'text-red-700 bg-red-50 border-red-200 dark:text-red-400 dark:bg-red-950/30 dark:border-red-800' },
  HttpRequest: { icon: FlaskConical, label: 'HTTP', color: 'text-green-700 bg-green-50 border-green-200 dark:text-green-400 dark:bg-green-950/30 dark:border-green-800' },
}

const ITEMS_PER_PAGE = 10

// Format condition for display
function formatConditionDisplay(rule: Rule): string {
  if (!rule.dsl) return '-'
  const whenMatch = rule.dsl.match(/WHEN\s+(.+?)(?:\nFOR|\nDO|$)/s)
  if (whenMatch) {
    let condition = whenMatch[1].trim()
    if (condition.length > 50) {
      condition = condition.substring(0, 47) + '...'
    }
    return condition
  }
  return '-'
}

// Check if rule has FOR clause
function hasForClause(rule: Rule): boolean {
  return rule.dsl?.includes('\nFOR ') || false
}

// Parse actions from DSL
function parseActionsFromDSL(dsl?: string): RuleAction[] {
  if (!dsl) return []
  const actions: RuleAction[] = []
  const doMatch = dsl.match(/\nDO\n(.*?)\nEND/s)
  if (!doMatch) return actions

  const actionLines = doMatch[1].trim().split('\n').map(l => l.trim().replace(/^    /, ''))

  for (const line of actionLines) {
    if (!line) continue

    const notifyMatch = line.match(/^NOTIFY\s+"(.+)"$/)
    if (notifyMatch) {
      actions.push({ type: 'Notify', message: notifyMatch[1] })
      continue
    }

    const execMatch = line.match(/^EXECUTE\s+([^.]+)\.(\w+)(?:\((.*)\))?$/)
    if (execMatch) {
      const [, deviceId, command, paramsStr] = execMatch
      const params: Record<string, string> = {}
      if (paramsStr) {
        paramsStr.split(', ').forEach(p => {
          const [k, v] = p.split('=')
          if (k && v) params[k] = v
        })
      }
      actions.push({ type: 'Execute', device_id: deviceId, command, params })
      continue
    }

    const logMatch = line.match(/^LOG\s+(\w+),\s+"(.+)"$/)
    if (logMatch) {
      actions.push({ type: 'Log', level: logMatch[1], message: logMatch[2] })
      continue
    }

    const setMatch = line.match(/^SET\s+([^.]+)\.([^=]+)\s*=\s*(.+)$/)
    if (setMatch) {
      actions.push({
        type: 'Set',
        device_id: setMatch[1],
        property: setMatch[2].trim(),
        value: setMatch[3].trim().replace(/^"|"$/g, '')
      })
      continue
    }

    const delayMatch = line.match(/^DELAY\s+(\d+)ms$/)
    if (delayMatch) {
      actions.push({ type: 'Delay', duration: parseInt(delayMatch[1], 10) })
      continue
    }

    const alertMatch = line.match(/^ALERT\s+"(.+)"\s+"(.+)"\s+(\w+)$/)
    if (alertMatch) {
      actions.push({
        type: 'CreateAlert',
        title: alertMatch[1],
        message: alertMatch[2],
        severity: alertMatch[3] as 'info' | 'warning' | 'error' | 'critical'
      })
      continue
    }

    const httpMatch = line.match(/^HTTP\s+(GET|POST|PUT|DELETE|PATCH)\s+(.+)$/)
    if (httpMatch) {
      actions.push({
        type: 'HttpRequest',
        method: httpMatch[1] as 'GET' | 'POST' | 'PUT' | 'DELETE' | 'PATCH',
        url: httpMatch[2]
      })
      continue
    }
  }

  return actions
}

export function RulesList({
  rules,
  loading,
  onEdit,
  onDelete,
  onToggleStatus,
  onExecute,
}: RulesListProps) {
  const { t } = useTranslation(['common', 'automation'])
  const [page, setPage] = useState(1)

  const totalPages = Math.ceil(rules.length / ITEMS_PER_PAGE) || 1
  const startIndex = (page - 1) * ITEMS_PER_PAGE
  const endIndex = startIndex + ITEMS_PER_PAGE
  const paginatedRules = rules.slice(startIndex, endIndex)

  function formatDateTime(dateStr: string | number | null | undefined): string {
    if (!dateStr) return '-'
    try {
      const date = new Date(dateStr)
      const now = new Date()
      const diffMs = now.getTime() - date.getTime()
      const diffMins = Math.floor(diffMs / 60000)

      if (diffMins < 1) return t('automation:time.justNow')
      if (diffMins < 60) return `${diffMins} ${t('automation:time.minutesAgo')}`
      const diffHours = Math.floor(diffMins / 60)
      if (diffHours < 24) return `${diffHours} ${t('automation:time.hoursAgo')}`
      return date.toLocaleDateString()
    } catch {
      return '-'
    }
  }

  return (
    <>
      <Card className="overflow-hidden">
        <Table>
          <TableHeader>
            <TableRow className="hover:bg-transparent border-b bg-muted/30">
              <TableHead className="w-10 text-center">#</TableHead>
              <TableHead>
                <div className="flex items-center gap-2 text-xs font-semibold uppercase tracking-wide text-muted-foreground">
                  <Sparkles className="h-4 w-4" />
                  {t('automation:ruleName')}
                </div>
              </TableHead>
              <TableHead>
                <div className="flex items-center gap-2 text-xs font-semibold uppercase tracking-wide text-muted-foreground">
                  <Zap className="h-4 w-4" />
                  {t('automation:trigger')}
                </div>
              </TableHead>
              <TableHead>
                <div className="flex items-center gap-2 text-xs font-semibold uppercase tracking-wide text-muted-foreground">
                  <CheckCircle2 className="h-4 w-4" />
                  {t('automation:ruleBuilder.executeActions')}
                </div>
              </TableHead>
              <TableHead>
                <div className="flex items-center gap-2 text-xs font-semibold uppercase tracking-wide text-muted-foreground">
                  <Clock className="h-4 w-4" />
                  {t('automation:lastTriggered')}
                </div>
              </TableHead>
              <TableHead className="text-center">
                <div className="flex items-center justify-center gap-2 text-xs font-semibold uppercase tracking-wide text-muted-foreground">
                  {t('automation:status')}
                </div>
              </TableHead>
              <TableHead className="w-12"></TableHead>
            </TableRow>
          </TableHeader>
          <TableBody>
            {loading ? (
              <EmptyStateInline title={t('common:loading')} colSpan={7} />
            ) : rules.length === 0 ? (
              <EmptyStateInline title={t('automation:noRules')} colSpan={7} />
            ) : (
              paginatedRules.map((rule, index) => {
                const actions = rule.actions && rule.actions.length > 0
                  ? rule.actions
                  : parseActionsFromDSL(rule.dsl)
                const actionsCount = actions.length
                const firstActions = actions.slice(0, 2)

                return (
                  <TableRow
                    key={rule.id}
                    className={cn(
                      "group transition-colors hover:bg-muted/50",
                      !rule.enabled && "opacity-50"
                    )}
                  >
                    <TableCell className="text-center">
                      <span className="text-xs text-muted-foreground font-medium">{startIndex + index + 1}</span>
                    </TableCell>

                    <TableCell>
                      <div className="flex items-center gap-3">
                        <div className={cn(
                          "w-9 h-9 rounded-lg flex items-center justify-center transition-colors",
                          rule.enabled ? "bg-amber-500/10 text-amber-600" : "bg-muted text-muted-foreground"
                        )}>
                          <Sparkles className="h-4 w-4" />
                        </div>
                        <div>
                          <div className="font-medium text-sm">{rule.name}</div>
                          <div className="text-xs text-muted-foreground line-clamp-1 max-w-[180px]">
                            {rule.description || '-'}
                          </div>
                        </div>
                      </div>
                    </TableCell>

                    <TableCell>
                      <div className="space-y-1.5">
                        <code className="text-xs bg-muted px-2 py-1 rounded-md block max-w-[200px] truncate font-mono">
                          {formatConditionDisplay(rule)}
                        </code>
                        {hasForClause(rule) && (
                          <Badge variant="outline" className="text-xs gap-1 text-blue-600 border-blue-200">
                            <Timer className="h-3 w-3" />
                            {t('automation:ruleBuilder.duration')}
                          </Badge>
                        )}
                      </div>
                    </TableCell>

                    <TableCell>
                      {actionsCount === 0 ? (
                        <span className="text-muted-foreground text-sm">-</span>
                      ) : (
                        <div className="flex flex-wrap gap-1">
                          {firstActions.map((action, i) => {
                            const config = ACTION_CONFIG[action.type] || ACTION_CONFIG.Execute
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
                      )}
                    </TableCell>

                    <TableCell>
                      <div className="flex items-center gap-2 text-xs">
                        <span className="text-muted-foreground">{formatDateTime(rule.last_triggered)}</span>
                        <span className="text-muted-foreground">({rule.trigger_count || 0})</span>
                      </div>
                    </TableCell>

                    <TableCell className="text-center">
                      <div className="flex items-center justify-center gap-2">
                        <Switch
                          checked={rule.enabled}
                          onCheckedChange={() => onToggleStatus(rule)}
                          className="scale-90"
                        />
                        <Badge variant="outline" className={cn(
                          "text-xs gap-1 hidden sm:flex",
                          rule.enabled
                            ? "text-green-700 bg-green-50 border-green-200 dark:text-green-400 dark:bg-green-950/30 dark:border-green-800"
                            : "text-gray-700 bg-gray-50 border-gray-200 dark:text-gray-400 dark:bg-gray-800 dark:border-gray-700"
                        )}>
                          <CheckCircle2 className="h-3 w-3" />
                          {rule.enabled ? t('automation:statusEnabled') : t('automation:statusDisabled')}
                        </Badge>
                      </div>
                    </TableCell>

                    <TableCell>
                      <DropdownMenu>
                        <DropdownMenuTrigger asChild>
                          <Button variant="ghost" size="icon" className="h-8 w-8 opacity-0 group-hover:opacity-100 transition-opacity">
                            <MoreVertical className="h-4 w-4" />
                          </Button>
                        </DropdownMenuTrigger>
                        <DropdownMenuContent align="end" className="w-40">
                          <DropdownMenuItem onClick={() => onEdit(rule)}>
                            <Edit className="mr-2 h-4 w-4" />
                            {t('common:edit')}
                          </DropdownMenuItem>
                          <DropdownMenuItem onClick={() => onExecute(rule)}>
                            <Play className="mr-2 h-4 w-4" />
                            {t('automation:execute')}
                          </DropdownMenuItem>
                          <DropdownMenuSeparator />
                          <DropdownMenuItem
                            onClick={() => onDelete(rule)}
                            className="text-destructive"
                          >
                            <Trash2 className="mr-2 h-4 w-4" />
                            {t('common:delete')}
                          </DropdownMenuItem>
                        </DropdownMenuContent>
                      </DropdownMenu>
                    </TableCell>
                  </TableRow>
                )
              })
            )}
          </TableBody>
        </Table>
      </Card>

      {rules.length > ITEMS_PER_PAGE && (
        <div className="sticky bottom-0 bg-background/95 backdrop-blur supports-[backdrop-filter]:bg-background/60 pt-4 pb-2 border-t mt-4">
          <Pagination
            total={rules.length}
            pageSize={ITEMS_PER_PAGE}
            currentPage={page}
            onPageChange={setPage}
          />
        </div>
      )}
    </>
  )
}
