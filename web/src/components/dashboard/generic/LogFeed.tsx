/**
 * Log Feed Component
 *
 * Real-time scrolling log display with severity levels.
 * Supports filtering, pausing, and auto-scroll.
 */

import { useState, useMemo, useEffect, useRef } from 'react'
import { Scroll, Pause, Play, Trash2, Search, Filter } from 'lucide-react'
import { cn } from '@/lib/utils'
import { useDataSource } from '@/hooks/useDataSource'
import { DashboardComponentWrapper } from '../DashboardComponentWrapper'
import type { DataSourceOrList } from '@/types/dashboard'

export type LogLevel = 'debug' | 'info' | 'warn' | 'error' | 'fatal'

export interface LogEntry {
  id: string
  timestamp?: string | Date
  level: LogLevel
  message: string
  source?: string
  details?: string
}

export interface LogFeedProps {
  dataSource?: DataSourceOrList
  data?: LogEntry[]
  title?: string
  maxEntries?: number
  showTimestamp?: boolean
  showSource?: boolean
  showDetails?: boolean
  filter?: LogLevel[]
  searchable?: boolean
  pauseable?: boolean
  clearable?: boolean
  autoScroll?: boolean
  size?: 'sm' | 'md' | 'lg'
  showCard?: boolean
  className?: string
}

const logLevelConfig: Record<LogLevel, { label: string; color: string; bg: string; border: string; icon: string }> = {
  debug: { label: 'DEBUG', color: 'text-gray-500', bg: 'bg-gray-500/10', border: 'border-gray-500/20', icon: '🔍' },
  info: { label: 'INFO', color: 'text-blue-500', bg: 'bg-blue-500/10', border: 'border-blue-500/20', icon: 'ℹ️' },
  warn: { label: 'WARN', color: 'text-yellow-500', bg: 'bg-yellow-500/10', border: 'border-yellow-500/20', icon: '⚠️' },
  error: { label: 'ERROR', color: 'text-red-500', bg: 'bg-red-500/10', border: 'border-red-500/20', icon: '❌' },
  fatal: { label: 'FATAL', color: 'text-red-600', bg: 'bg-red-600/10', border: 'border-red-600/20', icon: '💀' },
}

const sizeClasses = {
  sm: {
    entry: 'py-1 px-2 text-xs',
    badge: 'px-1.5 py-0.5 text-xs',
    timestamp: 'text-xs',
    message: 'text-xs',
  },
  md: {
    entry: 'py-2 px-3 text-sm',
    badge: 'px-2 py-0.5 text-xs',
    timestamp: 'text-xs',
    message: 'text-sm',
  },
  lg: {
    entry: 'py-2.5 px-4 text-base',
    badge: 'px-2.5 py-0.5 text-sm',
    timestamp: 'text-sm',
    message: 'text-base',
  },
}

export function LogFeed({
  dataSource,
  data: propData,
  title,
  maxEntries = 100,
  showTimestamp = true,
  showSource = false,
  showDetails = false,
  filter,
  searchable = true,
  pauseable = true,
  clearable = true,
  autoScroll = true,
  size = 'md',
  showCard = true,
  className,
}: LogFeedProps) {
  const [isPaused, setIsPaused] = useState(false)
  const [searchQuery, setSearchQuery] = useState('')
  const [levelFilter, setLevelFilter] = useState<Set<LogLevel>>(new Set(filter || ['info', 'warn', 'error', 'fatal']))
  const [expandedEntries, setExpandedEntries] = useState<Set<string>>(new Set())
  const scrollRef = useRef<HTMLDivElement | null>(null)
  const [showFilterPanel, setShowFilterPanel] = useState(false)

  // Get data from data source or use prop
  const { data } = useDataSource<LogEntry[]>(dataSource, { fallback: propData || [] })
  const allLogs = data || []

  // Filter logs
  const filteredLogs = useMemo(() => {
    return allLogs
      .filter(log => {
        if (!levelFilter.has(log.level)) return false
        if (searchQuery && !log.message.toLowerCase().includes(searchQuery.toLowerCase())) {
          return false
        }
        if (searchQuery && log.source && !log.source.toLowerCase().includes(searchQuery.toLowerCase())) {
          return false
        }
        return true
      })
      .slice(-maxEntries)
  }, [allLogs, levelFilter, searchQuery, maxEntries])

  // Auto-scroll when new entries arrive
  useEffect(() => {
    if (autoScroll && !isPaused && scrollRef.current) {
      scrollRef.current.scrollTop = scrollRef.current.scrollHeight
    }
  }, [filteredLogs.length, autoScroll, isPaused, scrollRef])

  const formatTimestamp = (timestamp: string | Date) => {
    const date = typeof timestamp === 'string' ? new Date(timestamp) : timestamp
    return date.toLocaleTimeString('en-US', { hour12: false, hour: '2-digit', minute: '2-digit', second: '2-digit' })
  }

  const toggleLevelFilter = (level: LogLevel) => {
    setLevelFilter(prev => {
      const next = new Set(prev)
      if (next.has(level)) {
        next.delete(level)
      } else {
        next.add(level)
      }
      return next
    })
  }

  const toggleEntry = (id: string) => {
    setExpandedEntries(prev => {
      const next = new Set(prev)
      if (next.has(id)) {
        next.delete(id)
      } else {
        next.add(id)
      }
      return next
    })
  }

  const handleClear = () => {
    // This would need to be connected to a data source clear function
    // For now, this is a no-op since logs come from immutable data source
    console.log('Clear logs - would clear data source')
  }

  const sizes = sizeClasses[size]

  const renderLogEntry = (entry: LogEntry, index: number) => {
    const config = logLevelConfig[entry.level]
    const isExpanded = expandedEntries.has(entry.id)

    return (
      <div
        key={entry.id}
        className={cn(
          'font-mono border-b border-border last:border-b-0 transition-colors cursor-pointer hover:bg-muted/30',
          sizes.entry,
          config.border
        )}
        onClick={() => showDetails && entry.details && toggleEntry(entry.id)}
      >
        <div className="flex items-start gap-2">
          {/* Level badge */}
          <span className={cn('shrink-0 px-1.5 py-0.5 rounded font-bold', config.bg, config.color, sizes.badge)}>
            {config.label}
          </span>

          {/* Icon */}
          <span className="shrink-0">{config.icon}</span>

          {/* Timestamp */}
          {showTimestamp && entry.timestamp && (
            <span className={cn('text-muted-foreground shrink-0', sizes.timestamp)}>
              {formatTimestamp(entry.timestamp)}
            </span>
          )}

          {/* Source */}
          {showSource && entry.source && (
            <span className={cn('text-muted-foreground shrink-0', sizes.timestamp)}>
              [{entry.source}]
            </span>
          )}

          {/* Message */}
          <span className={cn('flex-1 break-all', sizes.message)}>
            {entry.message}
          </span>
        </div>

        {/* Details (expandable) */}
        {isExpanded && entry.details && (
          <div className="mt-2 pl-4 text-muted-foreground text-xs whitespace-pre-wrap border-l-2 border-border">
            {entry.details}
          </div>
        )}
      </div>
    )
  }

  const renderFilterPanel = () => (
    <div className="p-3 border-t border-border bg-muted/30">
      <div className="text-xs font-medium text-muted-foreground mb-2">Filter by level:</div>
      <div className="flex flex-wrap gap-2">
        {(Object.keys(logLevelConfig) as LogLevel[]).map(level => {
          const config = logLevelConfig[level]
          const isSelected = levelFilter.has(level)
          return (
            <button
              key={level}
              onClick={() => toggleLevelFilter(level)}
              className={cn(
                'px-2 py-1 rounded text-xs font-medium transition-colors',
                isSelected ? config.bg + ' ' + config.color : 'bg-muted text-muted-foreground hover:bg-muted/70'
              )}
            >
              {config.label}
            </button>
          )
        })}
      </div>
    </div>
  )

  const content = (
    <div className={cn('flex flex-col', className)}>
      {/* Toolbar */}
      <div className="flex items-center gap-2 p-2 border-b border-border bg-muted/20">
        {/* Search */}
        {searchable && (
          <div className="relative flex-1">
            <Search className="absolute left-2 top-1/2 -translate-y-1/2 h-3.5 w-3.5 text-muted-foreground" />
            <input
              type="text"
              value={searchQuery}
              onChange={e => setSearchQuery(e.target.value)}
              placeholder="Search logs..."
              className="w-full pl-8 pr-3 py-1.5 text-sm border border-input rounded-md bg-background focus:outline-none focus:ring-1 focus:ring-ring"
            />
          </div>
        )}

        {/* Filter button */}
        <button
          onClick={() => setShowFilterPanel(!showFilterPanel)}
          className={cn(
            'p-2 rounded-md transition-colors',
            showFilterPanel ? 'bg-accent' : 'hover:bg-muted'
          )}
        >
          <Filter className="h-4 w-4" />
        </button>

        {/* Pause button */}
        {pauseable && (
          <button
            onClick={() => setIsPaused(!isPaused)}
            className={cn(
              'p-2 rounded-md transition-colors',
              isPaused ? 'bg-accent text-amber-500' : 'hover:bg-muted'
            )}
          >
            {isPaused ? <Pause className="h-4 w-4" /> : <Play className="h-4 w-4" />}
          </button>
        )}

        {/* Clear button */}
        {clearable && (
          <button
            onClick={handleClear}
            className="p-2 rounded-md hover:bg-muted transition-colors text-muted-foreground"
          >
            <Trash2 className="h-4 w-4" />
          </button>
        )}
      </div>

      {/* Filter panel */}
      {showFilterPanel && renderFilterPanel()}

      {/* Log entries */}
      <div
        ref={node => { if (node) scrollRef.current = node }}
        className="flex-1 overflow-auto max-h-96 bg-background/50"
      >
        {filteredLogs.length === 0 ? (
          <div className="py-8 text-center text-muted-foreground text-sm">
            {searchQuery || levelFilter.size < 5 ? 'No matching logs' : 'No logs available'}
          </div>
        ) : (
          filteredLogs.map((entry, index) => renderLogEntry(entry, index))
        )}
      </div>

      {/* Footer */}
      <div className="px-3 py-1.5 border-t border-border bg-muted/20 text-xs text-muted-foreground flex justify-between">
        <span>{filteredLogs.length} entries</span>
        {isPaused && <span className="text-amber-500">Paused</span>}
      </div>
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
