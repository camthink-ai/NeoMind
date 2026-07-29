/**
 * PushTargetsTab - Data Push targets displayed with ResponsiveTable.
 * Follows the project design system patterns.
 */

import { useEffect, useState, useCallback } from 'react'
import { useStore } from '@/store'
import { useTranslation } from 'react-i18next'
import { useIsMobile } from '@/hooks/useMobile'
import { ResponsiveTable, type TableColumn, EmptyState, Pagination } from '@/components/shared'
import { Send, Play, Square, FlaskConical, FileText, Pencil, Trash2, Loader2, Globe, Radio } from 'lucide-react'
import { Badge } from '@/components/ui/badge'
import { textMini, textNano } from '@/design-system/tokens/typography'
import { cn } from '@/lib/utils'
import { notifySuccess, notifyError } from '@/lib/notify'
import { confirm } from '@/components/ui/use-confirm'
import type { PushTarget } from '@/types'
import { DeliveryHistoryPanel } from './DeliveryHistoryPanel'
import { PushTargetDialog } from './PushTargetDialog'

const PAGE_SIZE = 10

export function PushTargetsTab() {
  const { t } = useTranslation()
  const isMobile = useIsMobile()
  const {
    pushTargets,
    pushTargetsLoading,
    fetchPushTargets,
    deletePushTarget,
    startPushTarget,
    stopPushTarget,
    testPushTarget,
    setEditingPushTarget,
    setPushTargetDialogOpen,
  } = useStore()

  const [logsTargetId, setLogsTargetId] = useState<string | null>(null)
  const [testingId, setTestingId] = useState<string | null>(null)
  const [page, setPage] = useState(1)

  useEffect(() => {
    fetchPushTargets()
  }, [fetchPushTargets])

  const handleToggle = useCallback(async (target: PushTarget) => {
    if (target.enabled) {
      // Stop interrupts the downstream data stream — confirm before executing.
      // Start is harmless and runs immediately (no confirm, asymmetric by design).
      const ok = await confirm({
        title: t('common:dataPush.confirmStop', 'Stop this push target?'),
        description: target.name,
        confirmText: t('common:dataPush.stop', 'Stop'),
        cancelText: t('common:cancel', 'Cancel'),
        variant: 'default',
      })
      if (!ok) return
      const success = await stopPushTarget(target.id)
      if (success) {
        notifySuccess(t('common:dataPush.stopped', 'Stopped'), target.name)
      } else {
        notifyError(t('common:dataPush.stopFailed', 'Stop failed'), target.name)
      }
    } else {
      const success = await startPushTarget(target.id)
      if (success) {
        notifySuccess(t('common:dataPush.started', 'Started'), target.name)
      } else {
        notifyError(t('common:dataPush.startFailed', 'Start failed'), target.name)
      }
    }
  }, [startPushTarget, stopPushTarget, t])

  const handleTest = useCallback(async (id: string, name: string) => {
    setTestingId(id)
    try {
      const result = await testPushTarget(id)
      if (result) {
        notifySuccess(
          t('common:dataPush.testSuccess', 'Test Successful'),
          t('common:dataPush.testSuccessDesc', { defaultValue: '{{name}} responded successfully', name })
        )
      } else {
        notifyError(
          t('common:dataPush.testFailed', 'Test Failed'),
          t('common:dataPush.testFailedDesc', { defaultValue: '{{name}} test returned no response', name })
        )
      }
    } catch {
      notifyError(
        t('common:dataPush.testFailed', 'Test Failed'),
        t('common:dataPush.testFailedDesc', { defaultValue: '{{name}} test returned no response', name })
      )
    } finally {
      setTestingId(null)
    }
  }, [testPushTarget, t])

  const handleDelete = useCallback(async (target: PushTarget) => {
    const confirmed = await confirm({
      title: t('common:dataPush.confirmDelete', 'Delete this push target?'),
      description: target.name,
      confirmText: t('common:dataPush.delete', 'Delete'),
      cancelText: t('common:cancel', 'Cancel'),
      variant: 'destructive',
    })
    if (!confirmed) return
    const ok = await deletePushTarget(target.id)
    if (ok) {
      notifySuccess(t('common:dataPush.deleted', 'Deleted'), target.name)
    }
  }, [deletePushTarget, t])

  const columns: TableColumn[] = [
    { key: 'name', label: t('common:dataPush.name', 'Name'), width: '22%' },
    { key: 'target_type', label: t('common:dataPush.targetType', 'Type'), width: '10%' },
    { key: 'status', label: t('common:dataPush.status', 'Status'), width: '8%' },
    { key: 'schedule', label: t('common:dataPush.schedule', 'Schedule'), width: '15%' },
    { key: 'sources', label: t('common:dataPush.sourcePatterns', 'Sources'), width: '25%' },
    { key: 'updated', label: t('common:dataPush.updated', 'Updated'), width: '12%' },
    { key: 'actions', label: '', width: '8%' },
  ]

  const renderCell = (columnKey: string, rowData: Record<string, unknown>) => {
    const target = rowData as unknown as PushTarget
    switch (columnKey) {
      case 'name':
        return (
          <div className="flex items-center gap-2.5">
            <div className={cn(
              "w-8 h-8 rounded-lg flex items-center justify-center shrink-0",
              target.enabled
                ? "bg-success-light text-success"
                : "bg-muted text-muted-foreground"
            )}>
              {target.target_type === 'webhook'
                ? <Globe className="h-4 w-4" />
                : <Radio className="h-4 w-4" />}
            </div>
            <div className="min-w-0">
              <div className="font-medium text-sm truncate">{target.name}</div>
              <code className={cn(textMini, "text-muted-foreground font-mono truncate")}>
                {target.id.slice(0, 8)}
              </code>
            </div>
          </div>
        )
      case 'target_type':
        return (
          <Badge variant="outline" className={cn(textMini, "h-6 uppercase")}>
            {target.target_type}
          </Badge>
        )
      case 'status':
        return (
          <div className="flex items-center gap-1.5">
            <div className={cn(
              "h-2 w-2 rounded-full",
              target.enabled ? "bg-success" : "bg-muted-foreground"
            )} />
            <span className={cn(textMini, target.enabled ? "text-success" : "text-muted-foreground")}>
              {target.enabled ? t('common:dataPush.running', 'Running') : t('common:dataPush.stopped', 'Stopped')}
            </span>
          </div>
        )
      case 'schedule':
        return (
          <span className={cn(textMini, "text-muted-foreground")}>
            {target.schedule.type === 'event_driven'
              ? t('common:dataPush.eventDriven', 'Event-driven')
              : t('common:dataPush.interval', { defaultValue: 'Every {{secs}}s', secs: target.schedule.interval_secs })}
          </span>
        )
      case 'sources':
        return (
          <span className={cn(textMini, "text-muted-foreground truncate block max-w-[250px]")}>
            {target.data_filter.source_patterns.length > 0
              ? target.data_filter.source_patterns.join(', ')
              : t('common:dataPush.allSources', 'All sources')}
          </span>
        )
      case 'updated':
        return (
          <span className={cn(textMini, "text-muted-foreground")}>
            {new Date(target.updated_at * 1000).toLocaleDateString()}
          </span>
        )
      default:
        return null
    }
  }

  const actions = [
    {
      label: t('common:dataPush.toggle', 'Start / Stop'),
      icon: <Play className="h-4 w-4" />,
      onClick: (rowData?: Record<string, unknown>) => handleToggle(rowData as unknown as PushTarget),
    },
    {
      label: t('common:dataPush.test', 'Test'),
      icon: testingId ? <Loader2 className="h-4 w-4 animate-spin" /> : <FlaskConical className="h-4 w-4" />,
      onClick: (rowData?: Record<string, unknown>) => {
        const target = rowData as unknown as PushTarget
        handleTest(target.id, target.name)
      },
    },
    {
      label: t('common:dataPush.logs', 'Logs'),
      icon: <FileText className="h-4 w-4" />,
      onClick: (rowData?: Record<string, unknown>) => setLogsTargetId((rowData as unknown as PushTarget).id),
    },
    {
      label: t('common:dataPush.edit', 'Edit'),
      icon: <Pencil className="h-4 w-4" />,
      onClick: (rowData?: Record<string, unknown>) => setEditingPushTarget(rowData as unknown as PushTarget),
    },
    {
      label: t('common:dataPush.delete', 'Delete'),
      icon: <Trash2 className="h-4 w-4" />,
      variant: 'destructive' as const,
      onClick: (rowData?: Record<string, unknown>) => handleDelete(rowData as unknown as PushTarget),
    },
  ]

  // Pagination
  useEffect(() => {
    const maxPage = Math.ceil(pushTargets.length / PAGE_SIZE) || 1
    if (page > maxPage) setPage(maxPage)
  }, [pushTargets.length])

  // Mobile infinite scroll: cumulative slice (0 to current page) so previous items stay
  // visible as the user scrolls. Desktop: current-page slice only. Matches automation.tsx.
  const paginatedTargets = isMobile
    ? pushTargets.slice(0, page * PAGE_SIZE)
    : pushTargets.slice((page - 1) * PAGE_SIZE, page * PAGE_SIZE)

  return (
    <div className="space-y-4">
      {/* Table */}
      <ResponsiveTable
        columns={columns}
        data={paginatedTargets as unknown as Record<string, unknown>[]}
        renderCell={renderCell}
        rowKey={(row) => (row as unknown as PushTarget).id}
        onRowClick={(row) => setEditingPushTarget(row as unknown as PushTarget)}
        actions={actions}
        loading={pushTargetsLoading}
        flexHeight
        emptyState={
          <EmptyState
            icon={<Send className="h-12 w-12" />}
            title={t('common:dataPush.empty', 'No push targets configured')}
            description={t('common:dataPush.emptyHint', 'Create a push target to forward data to external systems')}
          />
        }
      />

      {/* Pagination */}
      {pushTargets.length > PAGE_SIZE && (
        <Pagination
          total={pushTargets.length}
          pageSize={PAGE_SIZE}
          currentPage={page}
          onPageChange={setPage}
        />
      )}

      {/* Delivery History Dialog */}
      <DeliveryHistoryPanel
        targetId={logsTargetId || ''}
        open={!!logsTargetId}
        onOpenChange={(open) => { if (!open) setLogsTargetId(null) }}
      />

      {/* Create/Edit Dialog */}
      <PushTargetDialog />
    </div>
  )
}
