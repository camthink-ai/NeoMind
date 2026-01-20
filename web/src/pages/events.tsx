import { useState, useMemo } from 'react'
import { useTranslation } from 'react-i18next'
import {
  Activity,
  Cpu,
  Sparkles,
  Workflow,
  Bell,
  Brain,
  RefreshCw,
  ChevronDown,
  ChevronRight,
} from 'lucide-react'
import { PageLayout } from '@/components/layout/PageLayout'
import { api } from '@/lib/api'
import { Card } from '@/components/ui/card'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from '@/components/ui/table'
import { EmptyStateInline } from '@/components/shared'
import { useApiData } from '@/hooks/useApiData'
import { formatTimestamp } from '@/lib/utils/format'
import type { Event as SystemEvent } from '@/types'
import { cn } from '@/lib/utils'

type EventFilter = 'all' | 'device' | 'rule' | 'workflow' | 'alert' | 'llm'

const fetchEvents = async (): Promise<SystemEvent[]> => {
  const response = await api.getEvents({ limit: 200 }) as unknown as { events: SystemEvent[]; count: number }
  return response.events || []
}

// Event type configuration with icons
const EVENT_FILTERS_CONFIG = [
  { value: 'all' as EventFilter, labelKey: 'all', icon: Activity },
  { value: 'device' as EventFilter, labelKey: 'device', icon: Cpu },
  { value: 'rule' as EventFilter, labelKey: 'rule', icon: Sparkles },
  { value: 'workflow' as EventFilter, labelKey: 'workflow', icon: Workflow },
  { value: 'alert' as EventFilter, labelKey: 'alert', icon: Bell },
  { value: 'llm' as EventFilter, labelKey: 'llm', icon: Brain },
]

// Determine event category from event_type string
function getEventCategory(eventType: string): EventFilter {
  const type = eventType.toLowerCase()

  if (type.includes('device') || type.includes('metric') || type.includes('command')) {
    return 'device'
  }
  if (type.includes('rule') || type.includes('trigger') || type.includes('evaluated')) {
    return 'rule'
  }
  if (type.includes('workflow') || type.includes('step')) {
    return 'workflow'
  }
  if (type.includes('alert')) {
    return 'alert'
  }
  if (type.includes('llm') || type.includes('decision') || type.includes('periodic') || type.includes('tool')) {
    return 'llm'
  }
  return 'all'
}

// Get event display info
function getEventDisplayInfo(eventType: string) {
  const category = getEventCategory(eventType)

  // Base styles by category
  const baseStyles = {
    device: {
      color: 'text-blue-600 dark:text-blue-400',
      bgColor: 'bg-blue-50 dark:bg-blue-950/30',
      borderColor: 'border-blue-200 dark:border-blue-800',
      badgeVariant: 'outline' as const,
    },
    rule: {
      color: 'text-yellow-600 dark:text-yellow-400',
      bgColor: 'bg-yellow-50 dark:bg-yellow-950/30',
      borderColor: 'border-yellow-200 dark:border-yellow-800',
      badgeVariant: 'outline' as const,
    },
    workflow: {
      color: 'text-purple-600 dark:text-purple-400',
      bgColor: 'bg-purple-50 dark:bg-purple-950/30',
      borderColor: 'border-purple-200 dark:border-purple-800',
      badgeVariant: 'outline' as const,
    },
    alert: {
      color: 'text-red-600 dark:text-red-400',
      bgColor: 'bg-red-50 dark:bg-red-950/30',
      borderColor: 'border-red-200 dark:border-red-800',
      badgeVariant: 'destructive' as const,
    },
    llm: {
      color: 'text-green-600 dark:text-green-400',
      bgColor: 'bg-green-50 dark:bg-green-950/30',
      borderColor: 'border-green-200 dark:border-green-800',
      badgeVariant: 'outline' as const,
    },
    all: {
      color: 'text-gray-600 dark:text-gray-400',
      bgColor: 'bg-gray-50 dark:bg-gray-950/30',
      borderColor: 'border-gray-200 dark:border-gray-800',
      badgeVariant: 'secondary' as const,
    },
  }

  const styles = baseStyles[category]
  const IconComponent = EVENT_FILTERS_CONFIG.find(f => f.value === category)?.icon || Activity

  return { styles, IconComponent }
}

export function EventsPage() {
  const { t } = useTranslation(['common', 'events'])
  const [activeFilter, setActiveFilter] = useState<EventFilter>('all')
  const [expandedDataCells, setExpandedDataCells] = useState<Set<string>>(new Set())

  const { data: events, loading, refetch } = useApiData(
    () => fetchEvents(),
    { deps: [] }
  )

  const displayEvents = events || []

  // Toggle data cell expansion
  const toggleDataCell = (eventId: string) => {
    setExpandedDataCells(prev => {
      const next = new Set(prev)
      if (next.has(eventId)) {
        next.delete(eventId)
      } else {
        next.add(eventId)
      }
      return next
    })
  }

  // Filter events by category
  const filteredEvents = useMemo(() => {
    if (!displayEvents) return []
    if (activeFilter === 'all') return displayEvents
    return displayEvents.filter(event => getEventCategory(event.event_type) === activeFilter)
  }, [displayEvents, activeFilter])

  // Get count for each filter
  const filterCounts = useMemo(() => {
    if (!displayEvents) return {}
    const counts: Record<string, number> = { all: displayEvents.length }
    for (const event of displayEvents) {
      const category = getEventCategory(event.event_type)
      counts[category] = (counts[category] || 0) + 1
    }
    return counts
  }, [displayEvents])

  return (
    <PageLayout
      title={t('events:title')}
      subtitle={t('events:description')}
    >
      {/* Filter Bar */}
      <div className="flex items-center gap-2 flex-wrap mb-6">
        {EVENT_FILTERS_CONFIG.map((filter) => {
          const Icon = filter.icon
          const count = filterCounts[filter.value] || 0
          const isActive = activeFilter === filter.value
          const label = t(`events:${filter.labelKey}`)

          return (
            <Button
              key={filter.value}
              variant={isActive ? 'default' : 'outline'}
              size="sm"
              onClick={() => setActiveFilter(filter.value)}
              className={cn(
                "gap-2 rounded-lg",
                !isActive && "bg-background hover:bg-muted"
              )}
            >
              <Icon className="h-4 w-4" />
              <span>{label}</span>
              <Badge
                variant={isActive ? 'secondary' : 'outline'}
                className="h-5 px-1.5 text-xs"
              >
                {count}
              </Badge>
            </Button>
          )
        })}

        <div className="flex-1" />

        <Button
          variant="outline"
          size="sm"
          onClick={refetch}
          className="gap-2"
        >
          <RefreshCw className="h-4 w-4" />
          {t('common:refresh')}
        </Button>
      </div>

      {/* Events Table */}
      <Card>
        <Table>
          <TableHeader>
            <TableRow>
              <TableHead className="w-[100px]">ID</TableHead>
              <TableHead className="w-[100px]">{t('events:type')}</TableHead>
              <TableHead className="w-[150px]">{t('events:source')}</TableHead>
              <TableHead className="w-[300px]">{t('events:data')}</TableHead>
              <TableHead className="w-[100px] text-center">{t('events:status')}</TableHead>
              <TableHead className="w-[150px]">{t('events:timestamp')}</TableHead>
            </TableRow>
          </TableHeader>
          <TableBody>
            {loading ? (
              <EmptyStateInline
                title={t('common:loading')}
                colSpan={6}
              />
            ) : !filteredEvents || filteredEvents.length === 0 ? (
              <EmptyStateInline
                title={activeFilter === 'all'
                  ? t('events:noEvents')
                  : t('events:noFilteredEvents')
                }
                colSpan={6}
              />
            ) : (
              filteredEvents.map((event) => {
                const { styles } = getEventDisplayInfo(event.event_type)

                return (
                  <TableRow key={event.id} className="group">
                    <TableCell>
                      <span className="font-mono text-xs text-muted-foreground truncate block" title={event.id}>
                        {event.id.slice(0, 8)}
                      </span>
                    </TableCell>
                    <TableCell>
                      <Badge variant="outline" className={cn(
                        "text-xs font-medium truncate max-w-full border-0",
                        styles.bgColor
                      )}>
                        <span className={cn(styles.color)} title={event.event_type}>
                          {event.event_type}
                        </span>
                      </Badge>
                    </TableCell>
                    <TableCell>
                      {event.source ? (
                        <span className="font-mono text-xs text-muted-foreground truncate block" title={event.source}>
                          {event.source.length > 18 ? event.source.substring(0, 18) + '...' : event.source}
                        </span>
                      ) : (
                        <span className="text-xs text-muted-foreground/50">-</span>
                      )}
                    </TableCell>
                    <TableCell>
                      <button
                        onClick={() => toggleDataCell(event.id)}
                        className="text-left group/cell w-full block"
                        title={expandedDataCells.has(event.id) ? '收起' : '展开查看完整数据'}
                      >
                        <div className="flex items-start gap-1 min-w-0">
                          {expandedDataCells.has(event.id) ? (
                            <ChevronDown className="h-3 w-3 mt-0.5 shrink-0 text-muted-foreground" />
                          ) : (
                            <ChevronRight className="h-3 w-3 mt-0.5 shrink-0 text-muted-foreground" />
                          )}
                          <div className="min-w-0 text-xs text-muted-foreground font-sans">
                            <pre className={cn(
                              "whitespace-pre-wrap",
                              expandedDataCells.has(event.id) ? "break-all" : "line-clamp-3 break-all"
                            )}>
                              {JSON.stringify(event.data, null, 2)}
                            </pre>
                          </div>
                        </div>
                      </button>
                    </TableCell>
                    <TableCell className="text-center">
                      <Badge variant={event.processed ? 'secondary' : 'outline'} className="text-xs">
                        {event.processed ? t('events:processed') : t('events:pending')}
                      </Badge>
                    </TableCell>
                    <TableCell className="text-sm text-muted-foreground">
                      <span className="truncate block" title={formatTimestamp(event.timestamp)}>
                        {formatTimestamp(event.timestamp)}
                      </span>
                    </TableCell>
                  </TableRow>
                )
              })
            )}
          </TableBody>
        </Table>
      </Card>
    </PageLayout>
  )
}
