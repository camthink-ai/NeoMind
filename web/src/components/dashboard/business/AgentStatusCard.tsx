/**
 * Agent Status Card Component (Unified Styles)
 *
 * A business component for displaying AI agent status using unified dashboard styles.
 * Supports data binding and real-time updates.
 * Fully responsive and adaptive.
 */

import { Brain, Play, Pause, RotateCcw, Clock, CheckCircle2, XCircle, Loader2 } from 'lucide-react'
import { Button } from '@/components/ui/button'
import { Badge } from '@/components/ui/badge'
import { Card, CardContent, CardHeader } from '@/components/ui/card'
import { Skeleton } from '@/components/ui/skeleton'
import { cn } from '@/lib/utils'
import { useDataSource } from '@/hooks/useDataSource'
import { dashboardComponentSize, dashboardCardBase } from '@/design-system/tokens/size'
import type { DataSource } from '@/types/dashboard'

export type AgentStatus = 'idle' | 'running' | 'paused' | 'error' | 'completed'

export interface AgentStatusCardProps {
  // Data source configuration
  dataSource?: DataSource

  // Agent info
  agentId?: string
  name?: string
  description?: string
  status?: AgentStatus

  // Stats (can be bound via dataSource)
  executions?: number
  successRate?: number
  avgDuration?: number
  lastRun?: string

  // Actions
  showExecuteButton?: boolean
  showStats?: boolean
  onExecute?: () => void
  onStop?: () => void
  onReset?: () => void
  onClick?: () => void

  // Styling
  size?: 'sm' | 'md' | 'lg'
  className?: string
}

const statusConfig = {
  idle: {
    icon: CheckCircle2,
    color: 'text-muted-foreground',
    bgColor: 'bg-muted',
    label: 'Idle'
  },
  running: {
    icon: Loader2,
    color: 'text-blue-500',
    bgColor: 'bg-blue-500/10',
    label: 'Running'
  },
  paused: {
    icon: Pause,
    color: 'text-yellow-500',
    bgColor: 'bg-yellow-500/10',
    label: 'Paused'
  },
  error: {
    icon: XCircle,
    color: 'text-red-500',
    bgColor: 'bg-red-500/10',
    label: 'Error'
  },
  completed: {
    icon: CheckCircle2,
    color: 'text-green-500',
    bgColor: 'bg-green-500/10',
    label: 'Completed'
  },
}

const getAgentSizeClasses = (size: 'sm' | 'md' | 'lg') => {
  const config = dashboardComponentSize[size]
  return {
    header: config.titleText,
    stats: config.labelText,
    value: config.valueText,
    button: size === 'sm' ? 'h-6 text-xs' : size === 'md' ? 'h-7 text-xs' : 'h-8 text-sm',
    icon: config.iconSize,
    padding: config.padding,
  }
}

export interface AgentData {
  name?: string
  description?: string
  status?: AgentStatus
  executions?: number
  successRate?: number
  avgDuration?: number
  lastRun?: string
}

export function AgentStatusCard({
  dataSource,
  name: propName = 'Agent',
  description: propDescription,
  status: propStatus = 'idle',
  executions: propExecutions = 0,
  successRate: propSuccessRate = 100,
  avgDuration: propAvgDuration,
  lastRun: propLastRun,
  showExecuteButton = true,
  showStats = true,
  onExecute,
  onStop,
  onReset,
  onClick,
  size = 'md',
  className,
}: AgentStatusCardProps) {
  // Get data from source
  const { data, loading } = useDataSource<AgentData>(dataSource, {
    fallback: {
      name: propName,
      description: propDescription,
      status: propStatus,
      executions: propExecutions,
      successRate: propSuccessRate,
      avgDuration: propAvgDuration,
      lastRun: propLastRun,
    },
  })

  const agentData = data ?? {}

  const name = agentData.name ?? propName
  const description = agentData.description ?? propDescription
  const status = agentData.status ?? propStatus
  const executions = agentData.executions ?? propExecutions
  const successRate = agentData.successRate ?? propSuccessRate
  const avgDuration = agentData.avgDuration ?? propAvgDuration
  const lastRun = agentData.lastRun ?? propLastRun

  const config = statusConfig[status]
  const StatusIcon = config.icon
  const sizeConfig = getAgentSizeClasses(size)

  return (
    <Card
      className={cn(
        dashboardCardBase,
        'transition-all hover:shadow-md cursor-pointer',
        onClick && 'hover:border-primary/50',
        className
      )}
      onClick={onClick}
    >
      {/* Header */}
      <CardHeader className="flex flex-row items-start justify-between space-y-0 pb-3">
        <div className="flex items-center gap-2">
          <div className="rounded-lg bg-primary/10 p-2">
            <Brain className={cn('text-primary', sizeConfig.icon)} />
          </div>
          <div>
            {loading ? (
              <Skeleton className={cn('h-4 w-24', size === 'lg' && 'h-5 w-32')} />
            ) : (
              <h3 className={cn('font-semibold', sizeConfig.header)}>{name}</h3>
            )}
            {description && !loading && (
              <p className="text-xs text-muted-foreground line-clamp-1">{description}</p>
            )}
          </div>
        </div>

        <Badge className={cn(config.color, config.bgColor, 'gap-1', size === 'sm' && 'text-xs')}>
          <StatusIcon className={cn(sizeConfig.icon, status === 'running' && 'animate-spin')} />
          {config.label}
        </Badge>
      </CardHeader>

      {/* Stats */}
      {showStats && (
        <CardContent className="grid grid-cols-3 gap-2 pb-3">
          <div className="text-center">
            <p className={cn('text-muted-foreground', sizeConfig.stats)}>Executions</p>
            {loading ? (
              <Skeleton className="h-4 w-8 mx-auto" />
            ) : (
              <p className={cn('font-semibold', sizeConfig.value)}>{executions}</p>
            )}
          </div>
          <div className="text-center">
            <p className={cn('text-muted-foreground', sizeConfig.stats)}>Success</p>
            {loading ? (
              <Skeleton className="h-4 w-8 mx-auto" />
            ) : (
              <p className={cn('font-semibold', sizeConfig.value)}>{successRate}%</p>
            )}
          </div>
          <div className="text-center">
            <p className={cn('text-muted-foreground', sizeConfig.stats)}>Avg Time</p>
            {loading ? (
              <Skeleton className="h-4 w-8 mx-auto" />
            ) : (
              <p className={cn('font-semibold', sizeConfig.value)}>
                {avgDuration ? `${avgDuration}s` : '-'}
              </p>
            )}
          </div>
        </CardContent>
      )}

      {/* Footer with actions */}
      <CardContent className={cn('flex items-center justify-between pt-0', !showStats && 'pt-4')}>
        {lastRun && !loading && (
          <span className="text-xs text-muted-foreground flex items-center gap-1">
            <Clock className="w-3 h-3" />
            {lastRun}
          </span>
        )}
        {(!lastRun || loading) && (
          loading ? <Skeleton className="h-4 w-20" /> : <div />
        )}

        {showExecuteButton && !loading && (
          <div className="flex items-center gap-1" onClick={(e) => e.stopPropagation()}>
            {status === 'running' ? (
              <Button
                size="sm"
                variant="outline"
                onClick={onStop}
                className={sizeConfig.button}
              >
                <Pause className={cn(sizeConfig.icon, 'mr-1')} />
                Stop
              </Button>
            ) : status === 'error' ? (
              <Button
                size="sm"
                variant="outline"
                onClick={onReset}
                className={sizeConfig.button}
              >
                <RotateCcw className={cn(sizeConfig.icon, 'mr-1')} />
                Reset
              </Button>
            ) : (
              <Button
                size="sm"
                onClick={onExecute}
                className={sizeConfig.button}
              >
                <Play className={cn(sizeConfig.icon, 'mr-1')} />
                Execute
              </Button>
            )}
          </div>
        )}
      </CardContent>
    </Card>
  )
}
