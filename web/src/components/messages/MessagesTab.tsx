// MessagesTab Component
// Displays a list of messages/notifications with filtering and actions

import { useMemo } from 'react'
import { useTranslation } from 'react-i18next'
import { Card, CardContent } from '@/components/ui/card'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Checkbox } from '@/components/ui/checkbox'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'
import { EmptyState } from '@/components/shared/EmptyState'
import { LoadingState } from '@/components/shared/LoadingState'
import {
  Check,
  AlertCircle,
  Info,
  AlertTriangle,
  ShieldAlert,
  Trash2,
} from 'lucide-react'
import type { NotificationMessage, MessageStats, MessageSeverity, MessageStatus, MessageCategory } from '@/types'

interface MessagesTabProps {
  messages: NotificationMessage[]
  stats: MessageStats | null
  isLoading: boolean
  error: Error | null
  severityFilter: MessageSeverity | 'all'
  onSeverityFilterChange: (value: MessageSeverity | 'all') => void
  statusFilter: MessageStatus | 'all'
  onStatusFilterChange: (value: MessageStatus | 'all') => void
  categoryFilter: MessageCategory | 'all'
  onCategoryFilterChange: (value: MessageCategory | 'all') => void
  selectedIds: string[]
  onSelectionChange: (ids: string[]) => void
  onAcknowledge: (id: string) => Promise<void>
  onResolve: (id: string) => Promise<void>
  onArchive: (id: string) => Promise<void>
  onDelete: (id: string) => Promise<void>
  onRefresh: () => Promise<void>
}

// Severity icon and color mapping
const SEVERITY_CONFIG: Record<
  MessageSeverity,
  { icon: typeof Info; color: string; variant: 'default' | 'destructive' | 'warning' }
> = {
  info: { icon: Info, color: 'text-info', variant: 'default' as const },
  warning: { icon: AlertTriangle, color: 'text-warning', variant: 'warning' as const },
  critical: { icon: AlertCircle, color: 'text-error', variant: 'destructive' as const },
  emergency: { icon: ShieldAlert, color: 'text-error', variant: 'destructive' as const },
}

// Status badge mapping
const STATUS_CONFIG: Record<MessageStatus, { label: string; variant: 'default' | 'secondary' | 'outline' }> = {
  active: { label: 'messages.status.active', variant: 'default' as const },
  acknowledged: { label: 'messages.status.acknowledged', variant: 'secondary' as const },
  resolved: { label: 'messages.status.resolved', variant: 'outline' as const },
  archived: { label: 'messages.status.archived', variant: 'outline' as const },
  false_positive: { label: 'messages.status.false_positive', variant: 'outline' as const },
}

// Category badge mapping
const CATEGORY_CONFIG: Record<MessageCategory, { label: string }> = {
  alert: { label: 'messages.category.alert' },
  notification: { label: 'messages.category.notification' },
  system: { label: 'messages.category.system' },
  business: { label: 'messages.category.business' },
}

export function MessagesTab({
  messages,
  stats,
  isLoading,
  error,
  severityFilter,
  onSeverityFilterChange,
  statusFilter,
  onStatusFilterChange,
  categoryFilter,
  onCategoryFilterChange,
  selectedIds,
  onSelectionChange,
  onAcknowledge,
  onResolve,
  onArchive,
  onDelete,
  onRefresh,
}: MessagesTabProps) {
  const { t } = useTranslation()

  // Handle select all
  const allSelected = messages.length > 0 && selectedIds.length === messages.length
  const someSelected = selectedIds.length > 0 && selectedIds.length < messages.length

  const handleSelectAll = () => {
    if (allSelected || someSelected) {
      onSelectionChange([])
    } else {
      onSelectionChange(messages.map(m => m.id))
    }
  }

  const handleSelectOne = (id: string) => {
    if (selectedIds.includes(id)) {
      onSelectionChange(selectedIds.filter(sid => sid !== id))
    } else {
      onSelectionChange([...selectedIds, id])
    }
  }

  // Stats cards
  const statsCards = useMemo(() => {
    if (!stats) return []

    return [
      {
        label: t('messages.stats.total'),
        value: stats.total || 0,
        color: 'bg-info',
      },
      {
        label: t('messages.stats.active'),
        value: stats.active || 0,
        color: 'bg-error',
      },
      {
        label: t('messages.stats.byCategory.alert'),
        value: stats.by_category?.alert || 0,
        color: 'bg-error',
      },
      {
        label: t('messages.stats.bySeverity.critical'),
        value: stats.by_severity?.critical || 0 + (stats.by_severity?.emergency || 0),
        color: 'bg-error',
      },
    ]
  }, [stats, t])

  if (error) {
    return (
      <div className="p-6">
        <EmptyState
          icon={<AlertCircle className="h-12 w-12" />}
          title={t('messages.error')}
          description={error.message}
          action={{ label: t('retry'), onClick: onRefresh }}
        />
      </div>
    )
  }

  return (
    <div className="space-y-4">
      {/* Stats Cards */}
      {stats && (
        <div className="grid grid-cols-[minmax(0,1fr)_minmax(0,1fr)] md:grid-cols-4 gap-4">
          {statsCards.map((card, i) => (
            <Card key={i}>
              <CardContent className="p-4">
                <div className="flex items-center gap-3">
                  <div className={`w-2 h-8 rounded-full ${card.color}`} />
                  <div>
                    <p className="text-sm text-muted-foreground">{card.label}</p>
                    <p className="text-2xl font-semibold tabular-nums">{card.value}</p>
                  </div>
                </div>
              </CardContent>
            </Card>
          ))}
        </div>
      )}

      {/* Filters */}
      <div className="flex flex-wrap items-center gap-3">
        <Select
          value={severityFilter}
          onValueChange={(v) => onSeverityFilterChange(v as MessageSeverity | 'all')}
        >
          <SelectTrigger className="w-[140px]">
            <SelectValue placeholder={t('messages.filter.severity')} />
          </SelectTrigger>
          <SelectContent>
            <SelectItem value="all">{t('messages.filter.all')}</SelectItem>
            <SelectItem value="info">{t('messages.severity.info')}</SelectItem>
            <SelectItem value="warning">{t('messages.severity.warning')}</SelectItem>
            <SelectItem value="critical">{t('messages.severity.critical')}</SelectItem>
            <SelectItem value="emergency">{t('messages.severity.emergency')}</SelectItem>
          </SelectContent>
        </Select>

        <Select
          value={statusFilter}
          onValueChange={(v) => onStatusFilterChange(v as MessageStatus | 'all')}
        >
          <SelectTrigger className="w-[140px]">
            <SelectValue placeholder={t('messages.filter.status')} />
          </SelectTrigger>
          <SelectContent>
            <SelectItem value="all">{t('messages.filter.all')}</SelectItem>
            <SelectItem value="active">{t('messages.status.active')}</SelectItem>
            <SelectItem value="acknowledged">{t('messages.status.acknowledged')}</SelectItem>
            <SelectItem value="resolved">{t('messages.status.resolved')}</SelectItem>
            <SelectItem value="archived">{t('messages.status.archived')}</SelectItem>
          </SelectContent>
        </Select>

        <Select
          value={categoryFilter}
          onValueChange={(v) => onCategoryFilterChange(v as MessageCategory | 'all')}
        >
          <SelectTrigger className="w-[140px]">
            <SelectValue placeholder={t('messages.filter.category')} />
          </SelectTrigger>
          <SelectContent>
            <SelectItem value="all">{t('messages.filter.all')}</SelectItem>
            <SelectItem value="alert">{t('messages.category.alert')}</SelectItem>
            <SelectItem value="notification">{t('messages.category.notification')}</SelectItem>
            <SelectItem value="system">{t('messages.category.system')}</SelectItem>
            <SelectItem value="business">{t('messages.category.business')}</SelectItem>
          </SelectContent>
        </Select>

        {(severityFilter !== 'all' || statusFilter !== 'all' || categoryFilter !== 'all') && (
          <Button
            variant="ghost"
            size="sm"
            onClick={() => {
              onSeverityFilterChange('all')
              onStatusFilterChange('all')
              onCategoryFilterChange('all')
            }}
          >
            {t('messages.filter.clear')}
          </Button>
        )}
      </div>

      {/* Messages List */}
      {isLoading ? (
        <LoadingState variant="page" />
      ) : messages.length === 0 ? (
        <EmptyState
          icon={<Check className="h-12 w-12" />}
          title={t('messages.empty.title')}
          description={t('messages.empty.description')}
        />
      ) : (
        <div className="space-y-2">
          {/* Header with select all */}
          <div className="flex items-center gap-2 px-4 py-2 text-sm text-muted-foreground">
            <Checkbox
              checked={allSelected}
              onCheckedChange={handleSelectAll}
            />
            <span className="ml-2">
              {selectedIds.length > 0
                ? t('messages.selected', { count: selectedIds.length })
                : t('messages.count', { count: messages.length })
              }
            </span>
          </div>

          {/* Messages */}
          {messages.map((message) => {
            const severityConfig = SEVERITY_CONFIG[message.severity as MessageSeverity]
            const statusConfig = STATUS_CONFIG[message.status as MessageStatus]
            const categoryConfig = CATEGORY_CONFIG[message.category as MessageCategory]
            const SeverityIcon = severityConfig.icon

            const isSelected = selectedIds.includes(message.id)

            return (
              <Card
                key={message.id}
                className={`transition-colors ${isSelected ? 'bg-accent' : ''} ${
                  message.status === 'resolved' ? 'opacity-60' : ''
                }`}
              >
                <CardContent className="p-4">
                  <div className="flex items-start gap-3">
                    {/* Checkbox */}
                    <Checkbox
                      checked={isSelected}
                      onCheckedChange={() => handleSelectOne(message.id)}
                      className="mt-1"
                    />

                    {/* Severity Icon */}
                    <div className={`mt-1 ${severityConfig.color}`}>
                      <SeverityIcon className="h-5 w-5" />
                    </div>

                    {/* Content */}
                    <div className="flex-1 min-w-0">
                      <div className="flex items-center gap-2 flex-wrap">
                        <h3 className="font-semibold truncate">{message.title}</h3>
                        <Badge variant="outline">{t(categoryConfig.label)}</Badge>
                        <Badge variant={statusConfig.variant}>{t(statusConfig.label)}</Badge>
                      </div>
                      <p className="text-sm text-muted-foreground mt-1 line-clamp-2">
                        {message.message}
                      </p>
                      <div className="flex items-center gap-4 mt-2 text-xs text-muted-foreground">
                        <span>{t('messages.source')}: {message.source}</span>
                        <span>
                          {new Date(message.timestamp).toLocaleString()}
                        </span>
                        {message.tags.length > 0 && (
                          <div className="flex items-center gap-1">
                            {message.tags.map((tag, i) => (
                              <Badge key={i} variant="secondary" className="text-xs">
                                {tag}
                              </Badge>
                            ))}
                          </div>
                        )}
                      </div>
                    </div>

                    {/* Actions */}
                    <div className="flex items-center gap-1">
                      {message.status === 'active' && (
                        <>
                          <Button
                            variant="ghost"
                            size="sm"
                            onClick={() => onAcknowledge(message.id)}
                          >
                            <Check className="h-4 w-4" />
                          </Button>
                          <Button
                            variant="ghost"
                            size="sm"
                            onClick={() => onResolve(message.id)}
                          >
                            {t('messages.resolve')}
                          </Button>
                        </>
                      )}
                      {message.status === 'acknowledged' && (
                        <Button
                          variant="ghost"
                          size="sm"
                          onClick={() => onResolve(message.id)}
                        >
                          {t('messages.resolve')}
                        </Button>
                      )}
                      {message.status !== 'archived' && (
                        <Button
                          variant="ghost"
                          size="sm"
                          onClick={() => onArchive(message.id)}
                        >
                          {t('messages.archive')}
                        </Button>
                      )}
                      <Button
                        variant="ghost"
                        size="sm"
                        onClick={() => onDelete(message.id)}
                      >
                        <Trash2 className="h-4 w-4" />
                      </Button>
                    </div>
                  </div>
                </CardContent>
              </Card>
            )
          })}
        </div>
      )}
    </div>
  )
}
