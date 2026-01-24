/**
 * Status List Component
 *
 * Displays status items with icons, labels, and descriptions.
 * Supports grouping and filtering by status.
 */

import { useMemo } from 'react'
import { Circle, AlertCircle, CheckCircle, XCircle, Clock, HelpCircle } from 'lucide-react'
import { cn } from '@/lib/utils'
import { useDataSource } from '@/hooks/useDataSource'
import { DashboardComponentWrapper } from '../DashboardComponentWrapper'
import type { DataSourceOrList } from '@/types/dashboard'

export type StatusType = 'online' | 'offline' | 'warning' | 'error' | 'pending' | 'unknown' | 'success' | 'info'

export interface StatusItem {
  id: string
  label: string
  status: StatusType
  description?: string
  timestamp?: string | Date
  value?: string | number
  group?: string
  clickable?: boolean
  onClick?: () => void
}

export interface StatusListProps {
  dataSource?: DataSourceOrList
  data?: StatusItem[]
  title?: string
  groupBy?: 'status' | 'group' | 'none'
  filter?: StatusType[]
  showTimestamp?: boolean
  showDescription?: boolean
  compact?: boolean
  size?: 'sm' | 'md' | 'lg'
  showCard?: boolean
  className?: string
}

const statusConfig: Record<StatusType, { icon: typeof Circle; color: string; bg: string; label: string }> = {
  online: { icon: CheckCircle, color: 'text-green-500', bg: 'bg-green-500/10', label: 'Online' },
  offline: { icon: Circle, color: 'text-gray-400', bg: 'bg-gray-400/10', label: 'Offline' },
  warning: { icon: AlertCircle, color: 'text-yellow-500', bg: 'bg-yellow-500/10', label: 'Warning' },
  error: { icon: XCircle, color: 'text-red-500', bg: 'bg-red-500/10', label: 'Error' },
  pending: { icon: Clock, color: 'text-blue-500', bg: 'bg-blue-500/10', label: 'Pending' },
  unknown: { icon: HelpCircle, color: 'text-gray-400', bg: 'bg-gray-400/10', label: 'Unknown' },
  success: { icon: CheckCircle, color: 'text-green-500', bg: 'bg-green-500/10', label: 'Success' },
  info: { icon: Circle, color: 'text-blue-500', bg: 'bg-blue-500/10', label: 'Info' },
}

const sizeClasses = {
  sm: {
    item: 'py-1.5 px-2',
    icon: 'h-3.5 w-3.5',
    label: 'text-xs',
    description: 'text-xs',
    value: 'text-xs',
  },
  md: {
    item: 'py-2 px-3',
    icon: 'h-4 w-4',
    label: 'text-sm',
    description: 'text-xs',
    value: 'text-sm',
  },
  lg: {
    item: 'py-3 px-4',
    icon: 'h-5 w-5',
    label: 'text-base',
    description: 'text-sm',
    value: 'text-base',
  },
}

export function StatusFeed({

  dataSource,
  data: propData,
  title,
  groupBy = 'none',
  filter,
  showTimestamp = false,
  showDescription = true,
  compact = false,
  size = 'md',
  showCard = true,
  className,
}: StatusListProps) {
  // Get data from data source or use prop
  const { data } = useDataSource<StatusItem[]>(dataSource, { fallback: propData || [] })
  const statusList = data || []

  // Apply filter
  const filteredList = filter && filter.length > 0
    ? statusList.filter(item => filter.includes(item.status))
    : statusList

  // Group items
  const groups = useMemo(() => {
    if (groupBy === 'none') {
      return { '': filteredList }
    }

    return filteredList.reduce((acc, item) => {
      const key = groupBy === 'status' ? item.status : (item.group || 'Ungrouped')
      if (!acc[key]) acc[key] = []
      acc[key].push(item)
      return acc
    }, {} as Record<string, StatusItem[]>)
  }, [filteredList, groupBy])

  const formatTimestamp = (timestamp: string | Date) => {
    const date = typeof timestamp === 'string' ? new Date(timestamp) : timestamp
    const now = new Date()
    const diff = now.getTime() - date.getTime()

    if (diff < 60000) return 'Just now'
    if (diff < 3600000) return `${Math.floor(diff / 60000)}m ago`
    if (diff < 86400000) return `${Math.floor(diff / 3600000)}h ago`
    return date.toLocaleDateString()
  }

  const sizes = sizeClasses[size]

  const renderItem = (item: StatusItem) => {
    const config = statusConfig[item.status] || statusConfig.unknown
    const Icon = config.icon

    return (
      <div
        key={item.id}
        className={cn(
          'flex items-center gap-3 border-b border-border last:border-b-0 transition-colors',
          sizes.item,
          item.clickable && 'cursor-pointer hover:bg-muted/50',
          compact && 'gap-2'
        )}
        onClick={item.clickable ? item.onClick : undefined}
      >
        {/* Status icon */}
        <div className={cn('shrink-0', config.color)}>
          <Icon className={sizes.icon} />
        </div>

        {/* Label */}
        <div className="flex-1 min-w-0">
          <div className={cn('font-medium truncate', sizes.label)}>
            {item.label}
          </div>
          {showDescription && item.description && (
            <div className={cn('text-muted-foreground truncate', sizes.description)}>
              {item.description}
            </div>
          )}
        </div>

        {/* Value */}
        {item.value !== undefined && (
          <div className={cn('font-medium tabular-nums shrink-0', sizes.value)}>
            {item.value}
          </div>
        )}

        {/* Timestamp */}
        {showTimestamp && item.timestamp && (
          <div className={cn('text-muted-foreground shrink-0', sizes.description)}>
            {formatTimestamp(item.timestamp)}
          </div>
        )}

        {/* Status badge (compact mode) */}
        {compact && (
          <span className={cn('px-2 py-0.5 rounded text-xs font-medium shrink-0', config.bg, config.color)}>
            {config.label}
          </span>
        )}
      </div>
    )
  }

  const renderGroup = (groupKey: string, items: StatusItem[]) => {
    const groupLabel = groupBy === 'status'
      ? (statusConfig[groupKey as StatusType]?.label || groupKey)
      : groupKey || 'All Items'

    return (
      <div key={groupKey}>
        {groupKey && groupBy !== 'none' && (
          <div className="sticky top-0 bg-background/95 backdrop-blur z-10 px-3 py-1.5 text-xs font-medium text-muted-foreground border-b border-border">
            {groupLabel} ({items.length})
          </div>
        )}
        {items.map(renderItem)}
      </div>
    )
  }

  const content = (
    <div className={cn('overflow-auto', className)}>
      {statusList.length === 0 ? (
        <div className="py-8 text-center text-muted-foreground">
          No status items
        </div>
      ) : (
        Object.entries(groups).map(([key, items]) => renderGroup(key, items))
      )}
    </div>
  )

  if (showCard && title) {
    return (
      <DashboardComponentWrapper
        title={title}
        showCard={true}
        padding="none"
      >
        {content}
      </DashboardComponentWrapper>
    )
  }

  return content
}

// StatusList is an alias for StatusFeed
export const StatusList = StatusFeed
