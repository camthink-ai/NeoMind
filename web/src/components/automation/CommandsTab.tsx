import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { api } from '@/lib/api'
import type { CommandDto } from '@/types'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import { Badge } from '@/components/ui/badge'
import { LoadingState, EmptyState } from '@/components/shared'
import { useApiData } from '@/hooks/useApiData'
import { formatTimestamp } from '@/lib/utils/format'
import { useToast } from '@/hooks/use-toast'
import { RefreshCw, RotateCcw } from 'lucide-react'

type CommandFilter = 'all' | 'pending' | 'completed' | 'failed'

const fetchCommands = async (filter: CommandFilter): Promise<CommandDto[]> => {
  const status = filter === 'all' ? undefined : filter
  const response = await api.listCommands({ status, limit: 100 })
  return response.commands || []
}

export function CommandsTab() {
  const { t } = useTranslation(['common', 'commands'])
  const [filter, setFilter] = useState<CommandFilter>('all')
  const { toast } = useToast()

  const { data: commands, loading, refetch } = useApiData(
    () => fetchCommands(filter),
    { deps: [filter] }
  )

  const handleRetry = async (id: string) => {
    try {
      await api.retryCommand(id)
      toast({ title: t('commands:retrySuccess'), description: t('commands:retryDesc') })
      refetch()
    } catch (error) {
      toast({ title: t('commands:retryFailed'), description: (error as Error).message || t('commands:retryFailedDesc'), variant: 'destructive' })
    }
  }

  const handleCancel = async (id: string) => {
    try {
      await api.cancelCommand(id)
      toast({ title: t('commands:cancelSuccess'), description: t('commands:cancelDesc') })
      refetch()
    } catch (error) {
      toast({ title: t('commands:cancelFailed'), description: (error as Error).message || t('commands:cancelFailedDesc'), variant: 'destructive' })
    }
  }

  const getStatusBadge = (status: string) => {
    const variantMap: Record<string, 'default' | 'secondary' | 'destructive' | 'outline'> = {
      Pending: 'default',
      Queued: 'secondary',
      Sending: 'secondary',
      WaitingAck: 'secondary',
      Completed: 'outline',
      Failed: 'destructive',
      Cancelled: 'outline',
      Timeout: 'destructive',
    }
    const labelMap: Record<string, string> = {
      Pending: t('commands:pending'),
      Queued: t('commands:queued'),
      Sending: t('commands:sending'),
      WaitingAck: t('commands:waitingAck'),
      Completed: t('commands:completed'),
      Failed: t('commands:failed'),
      Cancelled: t('commands:cancelled'),
      Timeout: t('commands:timeout'),
    }
    return (
      <Badge variant={variantMap[status] || 'secondary'}>
        {labelMap[status] || status}
      </Badge>
    )
  }

  const getPriorityBadge = (priority: string) => {
    const colorMap: Record<string, string> = {
      low: 'bg-blue-500',
      normal: 'bg-gray-500',
      high: 'bg-orange-500',
      critical: 'bg-red-500',
      emergency: 'bg-red-600',
    }
    const labelMap: Record<string, string> = {
      low: t('commands:priorityLow'),
      normal: t('commands:priorityNormal'),
      high: t('commands:priorityHigh'),
      critical: t('commands:priorityCritical'),
      emergency: t('commands:priorityEmergency'),
    }
    return (
      <span className={`inline-flex items-center px-2 py-0.5 rounded text-xs font-medium text-white ${colorMap[priority] || 'bg-gray-500'}`}>
        {labelMap[priority] || priority}
      </span>
    )
  }

  const filters = [
    { value: 'all' as CommandFilter, label: t('commands:all') },
    { value: 'pending' as CommandFilter, label: t('commands:pending') },
    { value: 'completed' as CommandFilter, label: t('commands:completed') },
    { value: 'failed' as CommandFilter, label: t('commands:failed') },
  ]

  return (
    <div className="space-y-4">
      {/* Header */}
      <div className="flex items-center justify-between">
        <div>
          <h2 className="text-xl font-semibold">{t('automation:commands')}</h2>
          <p className="text-sm text-muted-foreground">
            {t('automation:commandsDesc')}
          </p>
        </div>
        <Button onClick={refetch} variant="outline" size="sm">
          <RefreshCw className="h-4 w-4 mr-2" />
          {t('common:refresh')}
        </Button>
      </div>

      {/* Filter tabs */}
      <div className="flex gap-2">
        {filters.map((f) => (
          <Button
            key={f.value}
            variant={filter === f.value ? "default" : "outline"}
            size="sm"
            onClick={() => setFilter(f.value)}
          >
            {f.label}
          </Button>
        ))}
      </div>

      {/* Content */}
      {loading ? (
        <LoadingState text={t('commands:loading')} />
      ) : !commands || commands.length === 0 ? (
        <EmptyState
          title={t('commands:noCommands')}
          description={t('commands:noCommandsDesc')}
        />
      ) : (
        <div className="space-y-4">
          {commands.map((command) => (
            <Card
              key={command.id}
              className={
                command.status === 'Pending' || command.status === 'Queued'
                  ? 'border-l-4 border-l-blue-500'
                  : command.status === 'Failed' || command.status === 'Timeout'
                  ? 'border-l-4 border-l-red-500'
                  : ''
              }
            >
              <CardHeader className="pb-3">
                <div className="flex items-start justify-between">
                  <div className="flex-1">
                    <div className="flex items-center gap-2 mb-1 flex-wrap">
                      <CardTitle className="text-base font-mono">{command.command}</CardTitle>
                      {getStatusBadge(command.status)}
                      {getPriorityBadge(command.priority)}
                    </div>
                    <CardDescription className="text-xs">
                      {t('commands:device')}: <span className="font-mono">{command.device_id}</span> • {t('commands:source')}: {command.source_type}
                    </CardDescription>
                  </div>
                  <div className="flex gap-2">
                    {(command.status === 'Failed' || command.status === 'Timeout') && (
                      <Button
                        onClick={() => handleRetry(command.id)}
                        variant="outline"
                        size="sm"
                      >
                        <RotateCcw className="h-3 w-3 mr-1" />
                        {t('commands:retry')}
                      </Button>
                    )}
                    {(command.status === 'Pending' || command.status === 'Queued') && (
                      <Button
                        onClick={() => handleCancel(command.id)}
                        variant="outline"
                        size="sm"
                      >
                        {t('commands:cancel')}
                      </Button>
                    )}
                  </div>
                </div>
              </CardHeader>
              <CardContent className="text-sm">
                <div className="grid grid-cols-2 md:grid-cols-4 gap-4 text-muted-foreground">
                  <div>
                    <span className="font-medium">{t('commands:commandId')}:</span>{' '}
                    <span className="font-mono text-xs">{command.id.slice(0, 8)}...</span>
                  </div>
                  <div>
                    <span className="font-medium">{t('commands:createdAt')}:</span> {formatTimestamp(command.created_at)}
                  </div>
                  {command.executed_at && (
                    <div>
                      <span className="font-medium">{t('commands:executedAt')}:</span> {formatTimestamp(command.executed_at)}
                    </div>
                  )}
                  <div>
                    <span className="font-medium">{t('commands:attempts')}:</span>{' '}
                    <span className={command.attempt > 1 ? 'text-orange-600 font-medium' : ''}>
                      {command.attempt}
                    </span>
                  </div>
                </div>
                {command.params && Object.keys(command.params).length > 0 && (
                  <details className="mt-3">
                    <summary className="cursor-pointer text-muted-foreground hover:text-foreground">
                      {t('commands:parameters')}
                    </summary>
                    <pre className="mt-2 p-2 bg-muted rounded text-xs overflow-x-auto">
                      {JSON.stringify(command.params, null, 2)}
                    </pre>
                  </details>
                )}
                {command.result && (
                  <details className="mt-3" open={command.status === 'Failed' || command.status === 'Timeout'}>
                    <summary className="cursor-pointer text-muted-foreground hover:text-foreground">
                      {t('commands:result')}
                    </summary>
                    <div className="mt-2 p-3 bg-muted rounded text-xs">
                      <div className="text-muted-foreground">{command.result.message}</div>
                      {command.result.response_data && (
                        <pre className="mt-2 p-2 bg-background rounded overflow-x-auto">
                          {JSON.stringify(command.result.response_data, null, 2)}
                        </pre>
                      )}
                    </div>
                  </details>
                )}
              </CardContent>
            </Card>
          ))}
        </div>
      )}
    </div>
  )
}
