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
import { EmptyStateInline, StatusBadge } from "@/components/shared"
import { Zap, Edit, Play, Trash2, MoreVertical, Bell, FileText, FlaskConical, AlertTriangle, Sparkles, Clock } from "lucide-react"
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

// Get action icon and label for display
function getActionDisplay(action: RuleAction): { icon: React.ReactNode; label: string; color: string } {
  const { t } = useTranslation('automation')

  switch (action.type) {
    case 'Execute':
      return {
        icon: <Zap className="h-3 w-3" />,
        label: t('ruleBuilder.actionType.execute', { defaultValue: '执行' }),
        color: 'text-yellow-700 bg-yellow-50 border-yellow-200 dark:text-yellow-400 dark:bg-yellow-950/30 dark:border-yellow-800'
      }
    case 'Notify':
      return {
        icon: <Bell className="h-3 w-3" />,
        label: t('ruleBuilder.actionType.notify', { defaultValue: '通知' }),
        color: 'text-blue-700 bg-blue-50 border-blue-200 dark:text-blue-400 dark:bg-blue-950/30 dark:border-blue-800'
      }
    case 'Log':
      return {
        icon: <FileText className="h-3 w-3" />,
        label: t('ruleBuilder.actionType.log', { defaultValue: '日志' }),
        color: 'text-gray-700 bg-gray-50 border-gray-200 dark:text-gray-400 dark:bg-gray-800 dark:border-gray-700'
      }
    case 'Set':
      return {
        icon: <FlaskConical className="h-3 w-3" />,
        label: t('ruleBuilder.actionType.set', { defaultValue: '设置' }),
        color: 'text-purple-700 bg-purple-50 border-purple-200 dark:text-purple-400 dark:bg-purple-950/30 dark:border-purple-800'
      }
    case 'Delay':
      return {
        icon: <Play className="h-3 w-3" />,
        label: t('ruleBuilder.actionType.delay', { defaultValue: '延迟' }),
        color: 'text-orange-700 bg-orange-50 border-orange-200 dark:text-orange-400 dark:bg-orange-950/30 dark:border-orange-800'
      }
    case 'CreateAlert':
      return {
        icon: <AlertTriangle className="h-3 w-3" />,
        label: t('ruleBuilder.actionType.createAlert', { defaultValue: '告警' }),
        color: 'text-red-700 bg-red-50 border-red-200 dark:text-red-400 dark:bg-red-950/30 dark:border-red-800'
      }
    case 'HttpRequest':
      return {
        icon: <FlaskConical className="h-3 w-3" />,
        label: 'HTTP',
        color: 'text-green-700 bg-green-50 border-green-200 dark:text-green-400 dark:bg-green-950/30 dark:border-green-800'
      }
    default: {
      const unknownAction = action as { type: string }
      return {
        icon: <Zap className="h-3 w-3" />,
        label: unknownAction.type,
        color: 'text-gray-700 bg-gray-50 border-gray-200 dark:text-gray-400 dark:bg-gray-800 dark:border-gray-700'
      }
    }
  }
}

// Format condition for display
function formatConditionDisplay(rule: Rule): string {
  if (!rule.dsl) return '-'

  // Try to parse the WHEN clause from DSL
  const whenMatch = rule.dsl.match(/WHEN\s+(.+?)(?:\nFOR|\nDO|$)/s)
  if (whenMatch) {
    let condition = whenMatch[1].trim()
    // Limit length for display
    if (condition.length > 60) {
      condition = condition.substring(0, 57) + '...'
    }
    return condition
  }

  // Fallback to first line after RULE
  const lines = rule.dsl.split('\n')
  for (let i = 0; i < lines.length; i++) {
    if (lines[i].startsWith('WHEN ')) {
      let condition = lines[i].substring(5).trim()
      if (condition.length > 60) {
        condition = condition.substring(0, 57) + '...'
      }
      return condition
    }
  }

  return rule.dsl.split('\n')[0]?.replace('RULE "', '').replace('"', '') || '-'
}

// Check if rule has FOR clause
function hasForClause(rule: Rule): boolean {
  return rule.dsl?.includes('\nFOR ') || false
}

// Parse actions from DSL
function parseActionsFromDSL(dsl?: string): RuleAction[] {
  if (!dsl) return []

  const actions: RuleAction[] = []

  // Find the DO ... END block
  const doMatch = dsl.match(/\nDO\n(.*?)\nEND/s)
  if (!doMatch) return actions

  const actionLines = doMatch[1].trim().split('\n').map(l => l.trim().replace(/^    /, ''))

  for (const line of actionLines) {
    if (!line) continue

    // Parse NOTIFY "message"
    const notifyMatch = line.match(/^NOTIFY\s+"(.+)"$/)
    if (notifyMatch) {
      actions.push({ type: 'Notify', message: notifyMatch[1] })
      continue
    }

    // Parse EXECUTE device.command(params)
    const execMatch = line.match(/^EXECUTE\s+([^.]+)\.(\w+)(?:\((.+)\))?$/)
    if (execMatch) {
      const [, deviceId, command, paramsStr] = execMatch
      const params: Record<string, string> = {}
      if (paramsStr) {
        paramsStr.split(', ').forEach(p => {
          const [k, v] = p.split('=')
          if (k && v) params[k] = v
        })
      }
      actions.push({
        type: 'Execute',
        device_id: deviceId,
        command,
        params
      })
      continue
    }

    // Parse LOG level, "message"
    const logMatch = line.match(/^LOG\s+(\w+),\s+"(.+)"$/)
    if (logMatch) {
      actions.push({ type: 'Log', level: logMatch[1], message: logMatch[2] })
      continue
    }

    // Parse SET device.property = value
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

    // Parse DELAY duration
    const delayMatch = line.match(/^DELAY\s+(\d+)ms$/)
    if (delayMatch) {
      actions.push({ type: 'Delay', duration: parseInt(delayMatch[1], 10) })
      continue
    }

    // Parse ALERT "title" "message" severity
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

    // Parse HTTP method url
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

  return (
    <Card>
      <Table>
        <TableHeader>
          <TableRow className="hover:bg-transparent border-b">
            <TableHead className="w-12">#</TableHead>
            <TableHead>
              <div className="flex items-center gap-1.5">
                <Sparkles className="h-4 w-4 text-muted-foreground" />
                {t('automation:ruleName')}
              </div>
            </TableHead>
            <TableHead>{t('automation:description')}</TableHead>
            <TableHead>{t('automation:trigger')}</TableHead>
            <TableHead>{t('automation:ruleBuilder.executeActions')}</TableHead>
            <TableHead>{t('automation:status')}</TableHead>
            <TableHead>
              <div className="flex items-center gap-1.5">
                <Zap className="h-4 w-4 text-muted-foreground" />
                {t('automation:triggerCount')}
              </div>
            </TableHead>
            <TableHead>
              <div className="flex items-center gap-1.5">
                <Clock className="h-4 w-4 text-muted-foreground" />
                {t('automation:lastTriggered')}
              </div>
            </TableHead>
            <TableHead className="text-right">{t('common:actions')}</TableHead>
          </TableRow>
        </TableHeader>
        <TableBody>
          {loading ? (
            <EmptyStateInline title={t('common:loading')} colSpan={9} />
          ) : rules.length === 0 ? (
            <EmptyStateInline title={t('automation:noRules')} colSpan={9} />
          ) : (
            rules.map((rule, index) => {
              // Try to get actions from detail response first, fallback to parsing DSL
              const actions = rule.actions && rule.actions.length > 0
                ? rule.actions
                : parseActionsFromDSL(rule.dsl)
              const actionsCount = actions.length
              const firstActions = actions.slice(0, 3)

              return (
                <TableRow
                  key={rule.id}
                  className={cn(
                    "group transition-colors hover:bg-muted/50",
                    !rule.enabled && "opacity-60"
                  )}
                >
                  <TableCell className="text-muted-foreground text-sm font-medium">
                    {index + 1}
                  </TableCell>
                  <TableCell className="font-medium">{rule.name}</TableCell>
                  <TableCell className="text-muted-foreground max-w-xs truncate">
                    {rule.description || '-'}
                  </TableCell>
                  <TableCell>
                    <div className="space-y-1">
                      <code className="text-xs bg-muted px-2 py-1 rounded block max-w-xs truncate font-mono">
                        {formatConditionDisplay(rule)}
                      </code>
                      {hasForClause(rule) && (
                        <Badge variant="outline" className="text-xs text-blue-600 border-blue-200">
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
                          const display = getActionDisplay(action)
                          return (
                            <Badge
                              key={i}
                              variant="outline"
                              className={cn("text-xs gap-1", display.color)}
                            >
                              {display.icon}
                              {display.label}
                            </Badge>
                          )
                        })}
                        {actionsCount > 3 && (
                          <Badge variant="outline" className="text-xs bg-muted">
                            +{actionsCount - 3}
                          </Badge>
                        )}
                      </div>
                    )}
                  </TableCell>
                  <TableCell>
                    <div className="flex items-center gap-2">
                      <Switch
                        checked={rule.enabled}
                        onCheckedChange={() => onToggleStatus(rule)}
                        className="scale-90"
                      />
                      <StatusBadge status={rule.enabled ? 'enabled' : 'disabled'} />
                    </div>
                  </TableCell>
                  <TableCell>
                    <span className="text-sm font-medium">{rule.trigger_count || 0}</span>
                  </TableCell>
                  <TableCell className="text-sm text-muted-foreground">
                    {rule.last_triggered ? new Date(rule.last_triggered).toLocaleString() : '-'}
                  </TableCell>
                  <TableCell className="text-right">
                    <DropdownMenu>
                      <DropdownMenuTrigger asChild>
                        <Button variant="ghost" size="icon" className="h-8 w-8 opacity-0 group-hover:opacity-100 transition-opacity">
                          <MoreVertical className="h-4 w-4" />
                        </Button>
                      </DropdownMenuTrigger>
                      <DropdownMenuContent align="end">
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
  )
}
