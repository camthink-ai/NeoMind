/**
 * Transform List Component
 *
 * Displays data transformation pipelines and their status.
 * Shows input/output schemas and execution status.
 */

import { ArrowRight, Play, Pause, Settings, FileInput, FileOutput } from 'lucide-react'
import { cn } from '@/lib/utils'
import { useDataSource } from '@/hooks/useDataSource'
import { DashboardComponentWrapper } from '../DashboardComponentWrapper'
import type { DataSource } from '@/types/dashboard'

export type TransformStatus = 'idle' | 'running' | 'success' | 'error' | 'disabled'

export interface Transform {
  id: string
  name: string
  status: TransformStatus
  inputType?: string
  outputType?: string
  lastRun?: number | string
  runCount?: number
  errorRate?: number
  description?: string
}

export interface TransformListProps {
  dataSource?: DataSource
  transforms?: Transform[]
  title?: string
  showSchema?: boolean
  showStats?: boolean
  compact?: boolean
  size?: 'sm' | 'md' | 'lg'
  showCard?: boolean
  className?: string
}

const statusConfig: Record<TransformStatus, { icon: typeof Play; color: string; bg: string; label: string }> = {
  idle: { icon: Pause, color: 'text-gray-400', bg: 'bg-gray-400/10', label: 'Idle' },
  running: { icon: Play, color: 'text-blue-500', bg: 'bg-blue-500/10', label: 'Running' },
  success: { icon: Play, color: 'text-green-500', bg: 'bg-green-500/10', label: 'Success' },
  error: { icon: Settings, color: 'text-red-500', bg: 'bg-red-500/10', label: 'Error' },
  disabled: { icon: Pause, color: 'text-gray-300', bg: 'bg-gray-300/10', label: 'Disabled' },
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

export function TransformList({
  dataSource,
  transforms: propTransforms,
  title,
  showSchema = true,
  showStats = true,
  compact = false,
  size = 'md',
  showCard = true,
  className,
}: TransformListProps) {
  // Get data from data source or use prop
  const { data } = useDataSource<Transform[]>(dataSource, { fallback: propTransforms || [] })
  const transforms = data || []

  const sizes = sizeClasses[size]

  const formatTimeAgo = (timestamp: number | string) => {
    const date = typeof timestamp === 'string' ? new Date(timestamp) : new Date(timestamp)
    const now = new Date()
    const diff = now.getTime() - date.getTime()

    if (diff < 60000) return 'Just now'
    if (diff < 3600000) return `${Math.floor(diff / 60000)}m ago`
    if (diff < 86400000) return `${Math.floor(diff / 3600000)}h ago`
    return `${Math.floor(diff / 86400000)}d ago`
  }

  const renderTransform = (transform: Transform) => {
    const config = statusConfig[transform.status]
    const Icon = config.icon

    return (
      <div
        key={transform.id}
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

        {/* Transform info */}
        <div className="flex-1 min-w-0">
          <div className={cn('font-medium truncate', sizes.name)}>
            {transform.name}
          </div>
          {transform.description && !compact && (
            <div className="text-xs text-muted-foreground truncate">
              {transform.description}
            </div>
          )}

          {/* Schema flow */}
          {showSchema && (transform.inputType || transform.outputType) && (
            <div className="flex items-center gap-2 mt-1.5 text-xs text-muted-foreground">
              {transform.inputType && (
                <span className="flex items-center gap-1">
                  <FileInput className="h-3 w-3" />
                  {transform.inputType}
                </span>
              )}
              <ArrowRight className="h-3 w-3" />
              {transform.outputType && (
                <span className="flex items-center gap-1">
                  <FileOutput className="h-3 w-3" />
                  {transform.outputType}
                </span>
              )}
            </div>
          )}
        </div>

        {/* Stats */}
        {showStats && (
          <div className="flex items-center gap-3 shrink-0">
            {transform.runCount !== undefined && (
              <div className="text-center">
                <div className={cn('font-bold tabular-nums', sizes.value)}>{transform.runCount}</div>
                <div className="text-xs text-muted-foreground">Runs</div>
              </div>
            )}
            {transform.errorRate !== undefined && transform.errorRate > 0 && (
              <div className="text-center">
                <div className={cn('font-bold tabular-nums text-red-500', sizes.value)}>
                  {transform.errorRate.toFixed(1)}%
                </div>
                <div className="text-xs text-muted-foreground">Errors</div>
              </div>
            )}
          </div>
        )}

        {/* Status badge and last run */}
        <div className="text-right shrink-0">
          <span className={cn('px-2 py-1 rounded text-xs font-medium', config.bg, config.color)}>
            {config.label}
          </span>
          {transform.lastRun && (
            <div className="text-xs text-muted-foreground mt-1">
              {formatTimeAgo(transform.lastRun)}
            </div>
          )}
        </div>
      </div>
    )
  }

  // Calculate summary stats
  const stats = {
    total: transforms.length,
    running: transforms.filter(t => t.status === 'running').length,
    success: transforms.filter(t => t.status === 'success').length,
    error: transforms.filter(t => t.status === 'error').length,
    totalRuns: transforms.reduce((sum, t) => sum + (t.runCount || 0), 0),
  }

  const content = (
    <div className={cn('space-y-3', className)}>
      {/* Summary stats */}
      {showStats && (
        <div className="grid grid-cols-5 gap-2">
          <div className="text-center p-2 bg-muted/30 rounded-lg">
            <div className={cn('font-bold tabular-nums', sizes.value)}>{stats.total}</div>
            <div className="text-xs text-muted-foreground">Total</div>
          </div>
          <div className="text-center p-2 bg-blue-500/10 rounded-lg">
            <div className={cn('font-bold tabular-nums text-blue-500', sizes.value)}>{stats.running}</div>
            <div className="text-xs text-muted-foreground">Running</div>
          </div>
          <div className="text-center p-2 bg-green-500/10 rounded-lg">
            <div className={cn('font-bold tabular-nums text-green-500', sizes.value)}>{stats.success}</div>
            <div className="text-xs text-muted-foreground">Success</div>
          </div>
          {stats.error > 0 && (
            <div className="text-center p-2 bg-red-500/10 rounded-lg">
              <div className={cn('font-bold tabular-nums text-red-500', sizes.value)}>{stats.error}</div>
              <div className="text-xs text-muted-foreground">Errors</div>
            </div>
          )}
          <div className="text-center p-2 bg-purple-500/10 rounded-lg">
            <div className={cn('font-bold tabular-nums text-purple-500', sizes.value)}>{stats.totalRuns}</div>
            <div className="text-xs text-muted-foreground">Total Runs</div>
          </div>
        </div>
      )}

      {/* Transforms list */}
      <div className="space-y-2">
        {transforms.length === 0 ? (
          <div className="py-8 text-center text-muted-foreground text-sm">
            No transforms configured
          </div>
        ) : (
          transforms.map(renderTransform)
        )}
      </div>
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

