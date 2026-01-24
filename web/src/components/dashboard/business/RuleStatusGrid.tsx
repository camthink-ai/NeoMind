/**
 * Rule Status Grid Component
 *
 * Displays status overview of automation rules.
 * Shows enabled/disabled state, trigger counts, and health.
 */

import { useMemo } from 'react'
import { Power, Zap, AlertCircle, CheckCircle, Pause, TrendingUp } from 'lucide-react'
import { cn } from '@/lib/utils'
import { useDataSource } from '@/hooks/useDataSource'
import { DashboardComponentWrapper } from '../DashboardComponentWrapper'
import type { DataSource } from '@/types/dashboard'

export type RuleStatusType = 'enabled' | 'disabled' | 'error' | 'paused'

export interface RuleStatus {
  id: string
  name: string
  status: RuleStatusType
  triggerCount?: number
  lastTriggered?: number | string
  errorCount?: number
  category?: string
}

export interface RuleStatusGridProps {
  dataSource?: DataSource
  rules?: RuleStatus[]
  title?: string
  groupBy?: 'status' | 'category' | 'none'
  showTriggers?: boolean
  showErrors?: boolean
  compact?: boolean
  size?: 'sm' | 'md' | 'lg'
  showCard?: boolean
  className?: string
}

const statusConfig: Record<RuleStatusType, { icon: typeof CheckCircle; color: string; bg: string; label: string }> = {
  enabled: { icon: CheckCircle, color: 'text-green-500', bg: 'bg-green-500/10', label: 'Enabled' },
  disabled: { icon: Pause, color: 'text-gray-400', bg: 'bg-gray-400/10', label: 'Disabled' },
  error: { icon: AlertCircle, color: 'text-red-500', bg: 'bg-red-500/10', label: 'Error' },
  paused: { icon: Pause, color: 'text-yellow-500', bg: 'bg-yellow-500/10', label: 'Paused' },
}

const sizeClasses = {
  sm: {
    card: 'p-2',
    name: 'text-xs',
    value: 'text-sm',
  },
  md: {
    card: 'p-3',
    name: 'text-sm',
    value: 'text-base',
  },
  lg: {
    card: 'p-4',
    name: 'text-base',
    value: 'text-lg',
  },
}

export function RuleStatusGrid({
  dataSource,
  rules: propRules,
  title,
  groupBy = 'none',
  showTriggers = true,
  showErrors = true,
  compact = false,
  size = 'md',
  showCard = true,
  className,
}: RuleStatusGridProps) {
  // Get data from data source or use prop
  const { data } = useDataSource<RuleStatus[]>(dataSource, { fallback: propRules || [] })
  const rules = data || []

  const sizes = sizeClasses[size]

  // Group rules
  const groupedRules = useMemo(() => {
    if (groupBy === 'none') {
      return { '': rules }
    }

    return rules.reduce((acc, rule) => {
      const key = groupBy === 'status' ? rule.status : (rule.category || 'Uncategorized')
      if (!acc[key]) acc[key] = []
      acc[key].push(rule)
      return acc
    }, {} as Record<string, RuleStatus[]>)
  }, [rules, groupBy])

  // Calculate stats
  const stats = useMemo(() => {
    return {
      total: rules.length,
      enabled: rules.filter(r => r.status === 'enabled').length,
      disabled: rules.filter(r => r.status === 'disabled').length,
      error: rules.filter(r => r.status === 'error').length,
      totalTriggers: rules.reduce((sum, r) => sum + (r.triggerCount || 0), 0),
      totalErrors: rules.reduce((sum, r) => sum + (r.errorCount || 0), 0),
    }
  }, [rules])

  const formatTimeAgo = (timestamp: number | string) => {
    const date = typeof timestamp === 'string' ? new Date(timestamp) : new Date(timestamp)
    const now = new Date()
    const diff = now.getTime() - date.getTime()

    if (diff < 60000) return 'Just now'
    if (diff < 3600000) return `${Math.floor(diff / 60000)}m ago`
    if (diff < 86400000) return `${Math.floor(diff / 3600000)}h ago`
    return `${Math.floor(diff / 86400000)}d ago`
  }

  const renderRule = (rule: RuleStatus) => {
    const config = statusConfig[rule.status]
    const Icon = config.icon

    return (
      <div
        key={rule.id}
        className={cn(
          'flex items-center gap-3 rounded-lg border border-border',
          'hover:bg-muted/50 transition-colors',
          sizes.card,
          compact && 'gap-2'
        )}
      >
        {/* Status icon */}
        <div className={cn('shrink-0 p-2 rounded-lg', config.bg)}>
          <Icon className={cn('h-4 w-4', config.color)} />
        </div>

        {/* Rule info */}
        <div className="flex-1 min-w-0">
          <div className={cn('font-medium truncate', sizes.name)}>
            {rule.name}
          </div>
          {rule.category && groupBy !== 'category' && (
            <div className="text-xs text-muted-foreground">{rule.category}</div>
          )}
        </div>

        {/* Metrics */}
        <div className="flex items-center gap-3 shrink-0">
          {showTriggers && rule.triggerCount !== undefined && (
            <div className="text-center">
              <div className={cn('font-bold tabular-nums', sizes.value)}>{rule.triggerCount}</div>
              <div className="text-xs text-muted-foreground flex items-center gap-1">
                <Zap className="h-2.5 w-2.5" />
                Triggers
              </div>
            </div>
          )}

          {showErrors && rule.errorCount !== undefined && rule.errorCount > 0 && (
            <div className="text-center">
              <div className={cn('font-bold tabular-nums text-red-500', sizes.value)}>
                {rule.errorCount}
              </div>
              <div className="text-xs text-muted-foreground">Errors</div>
            </div>
          )}
        </div>

        {/* Last triggered */}
        {rule.lastTriggered && (
          <div className="text-xs text-muted-foreground shrink-0">
            {formatTimeAgo(rule.lastTriggered)}
          </div>
        )}
      </div>
    )
  }

  const renderGroup = (groupKey: string, groupRules: RuleStatus[]) => {
    const label = groupKey || 'All Rules'

    return (
      <div key={groupKey}>
        {groupKey && groupBy !== 'none' && (
          <div className="sticky top-0 bg-background/95 backdrop-blur z-10 px-2 py-1.5 text-xs font-medium text-muted-foreground border-b border-border mb-2">
            {label} ({groupRules.length})
          </div>
        )}
        <div className={cn(
          'gap-2',
          compact ? 'grid grid-cols-1' : 'grid grid-cols-1 md:grid-cols-2'
        )}>
          {groupRules.map(renderRule)}
        </div>
      </div>
    )
  }

  const content = (
    <div className={cn('space-y-4', className)}>
      {/* Summary stats */}
      <div className="grid grid-cols-5 gap-2">
        <div className="text-center p-2 bg-muted/30 rounded-lg">
          <div className={cn('font-bold tabular-nums', sizes.value)}>{stats.total}</div>
          <div className="text-xs text-muted-foreground">Total</div>
        </div>
        <div className="text-center p-2 bg-green-500/10 rounded-lg">
          <Power className="h-3 w-3 mx-auto mb-1 text-green-500" />
          <div className={cn('font-bold tabular-nums text-green-500', sizes.value)}>{stats.enabled}</div>
          <div className="text-xs text-muted-foreground">Enabled</div>
        </div>
        <div className="text-center p-2 bg-gray-500/10 rounded-lg">
          <Pause className="h-3 w-3 mx-auto mb-1 text-gray-400" />
          <div className={cn('font-bold tabular-nums text-gray-400', sizes.value)}>{stats.disabled}</div>
          <div className="text-xs text-muted-foreground">Disabled</div>
        </div>
        {stats.error > 0 && (
          <div className="text-center p-2 bg-red-500/10 rounded-lg">
            <AlertCircle className="h-3 w-3 mx-auto mb-1 text-red-500" />
            <div className={cn('font-bold tabular-nums text-red-500', sizes.value)}>{stats.error}</div>
            <div className="text-xs text-muted-foreground">Errors</div>
          </div>
        )}
        {showTriggers && (
          <div className="text-center p-2 bg-blue-500/10 rounded-lg">
            <TrendingUp className="h-3 w-3 mx-auto mb-1 text-blue-500" />
            <div className={cn('font-bold tabular-nums text-blue-500', sizes.value)}>
              {stats.totalTriggers}
            </div>
            <div className="text-xs text-muted-foreground">Triggers</div>
          </div>
        )}
      </div>

      {/* Rules list */}
      {rules.length === 0 ? (
        <div className="py-8 text-center text-muted-foreground text-sm">
          No rules configured
        </div>
      ) : (
        <div className="space-y-3">
          {Object.entries(groupedRules).map(([key, rules]) => renderGroup(key, rules))}
        </div>
      )}
    </div>
  )

  if (showCard && title) {
    return (
      <DashboardComponentWrapper
        title={title}
        showCard={true}
        padding="md"
      >
        {content}
      </DashboardComponentWrapper>
    )
  }

  return content
}

