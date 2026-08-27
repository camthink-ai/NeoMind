/**
 * DeliveryHistoryPanel - delivery logs for a push target in FullScreenDialog with ResponsiveTable.
 */

import { useEffect, useState, useCallback } from 'react'
import { useStore } from '@/store'
import { useTranslation } from 'react-i18next'
import { FileText, CheckCircle2, XCircle, Clock, AlertTriangle, Copy, Eye } from 'lucide-react'
import { ResponsiveTable, type TableColumn, EmptyState, Pagination } from '@/components/shared'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { UnifiedFormDialog } from '@/components/dialog/UnifiedFormDialog'
import { cn } from '@/lib/utils'
import { notifySuccess } from '@/lib/notify'
import { textMini, textNano } from '@/design-system/tokens/typography'
import {
  FullScreenDialog,
  FullScreenDialogHeader,
  FullScreenDialogContent,
  FullScreenDialogMain,
} from '@/components/automation/dialog'
import type { DeliveryLog, DeliveryStatus } from '@/types'
import { copyToClipboard } from '@/lib/clipboard'

const PAGE_SIZE = 10

interface DeliveryHistoryPanelProps {
  targetId: string
  open: boolean
  onOpenChange: (open: boolean) => void
}

const STATUS_CONFIG: Record<DeliveryStatus, { color: string; bg: string; icon: typeof CheckCircle2; label: string }> = {
  success: { color: 'text-success', bg: 'bg-success-light', icon: CheckCircle2, label: 'SUCCESS' },
  failed: { color: 'text-error', bg: 'bg-error-light', icon: XCircle, label: 'FAILED' },
  pending: { color: 'text-warning', bg: 'bg-warning-light', icon: Clock, label: 'PENDING' },
  retrying: { color: 'text-warning', bg: 'bg-warning-light', icon: AlertTriangle, label: 'RETRYING' },
}

export function DeliveryHistoryPanel({ targetId, open, onOpenChange }: DeliveryHistoryPanelProps) {
  const { t } = useTranslation()
  const { deliveryLogs, deliveryLogsTotal, deliveryLogsLoading, fetchDeliveryLogs } = useStore()
  const [page, setPage] = useState(1)
  const [previewPayload, setPreviewPayload] = useState<string | null>(null)

  const copyPayload = useCallback(
    async (text: string) => {
      try {
        await copyToClipboard(text)
        notifySuccess(t('common:dataPush.payloadCopied', 'Payload copied to clipboard'))
      } catch {
        /* clipboard unavailable (non-secure context) — ignore */
      }
    },
    [t],
  )

  const loadPage = useCallback((p: number) => {
    setPage(p)
    if (targetId) {
      fetchDeliveryLogs(targetId, PAGE_SIZE, (p - 1) * PAGE_SIZE)
    }
  }, [targetId, fetchDeliveryLogs])

  useEffect(() => {
    if (open && targetId) {
      setPage(1)
      fetchDeliveryLogs(targetId, PAGE_SIZE, 0)
    }
  }, [open, targetId, fetchDeliveryLogs])

  const columns: TableColumn[] = [
    { key: 'status', label: t('common:dataPush.status', 'Status'), width: '12%' },
    { key: 'source', label: 'Source', width: '30%' },
    { key: 'payload', label: 'Payload', width: '35%' },
    { key: 'attempts', label: t('common:dataPush.attemptsLabel', 'Attempts'), width: '10%' },
    { key: 'time', label: t('common:dataPush.updated', 'Time'), width: '13%' },
  ]

  const renderCell = (columnKey: string, rowData: Record<string, unknown>) => {
    const log = rowData as unknown as DeliveryLog
    switch (columnKey) {
      case 'status': {
        const cfg = STATUS_CONFIG[log.status] || STATUS_CONFIG.pending
        const Icon = cfg.icon
        return (
          <Badge variant="outline" className={cn(textNano, "h-5 gap-1 px-1.5 border-0", cfg.bg, cfg.color)}>
            <Icon className="h-3 w-3" />
            {cfg.label}
          </Badge>
        )
      }
      case 'source':
        return (
          <span className={cn(textMini, "truncate block max-w-[250px]")}>{log.data_source_id}</span>
        )
      case 'payload': {
        if (!log.payload_sent) return <span className={cn(textMini, "text-muted-foreground")}>-</span>
        const raw = log.payload_sent
        let preview = raw
        try {
          preview = JSON.stringify(JSON.parse(raw))
        } catch { /* use raw */ }
        return (
          <div className="flex items-center gap-1 min-w-0">
            <code className={cn(textNano, "text-muted-foreground font-mono truncate block flex-1 min-w-0 max-w-[200px]")}>
              {preview.length > 120 ? preview.slice(0, 117) + '...' : preview}
            </code>
            <button
              type="button"
              onClick={() => copyPayload(raw)}
              title={t('common:copy', 'Copy')}
              className="text-muted-foreground hover:text-foreground shrink-0 p-1 rounded"
            >
              <Copy className="h-3.5 w-3.5" />
            </button>
            <button
              type="button"
              onClick={() => setPreviewPayload(raw)}
              title={t('common:dataPush.preview', 'Preview')}
              className="text-muted-foreground hover:text-foreground shrink-0 p-1 rounded"
            >
              <Eye className="h-3.5 w-3.5" />
            </button>
          </div>
        )
      }
      case 'attempts':
        return <span className={cn(textMini)}>{log.attempts}</span>
      case 'time':
        return (
          <span className={cn(textMini, "text-muted-foreground")}>
            {log.created_at ? new Date(log.created_at * 1000).toLocaleString() : '-'}
          </span>
        )
      default:
        return null
    }
  }

  return (
    <FullScreenDialog open={open} onOpenChange={onOpenChange}>
      <FullScreenDialogHeader
        icon={<FileText className="h-5 w-5" />}
        iconBg="bg-info-light"
        iconColor="text-info"
        title={t('common:dataPush.deliveryHistory', 'Delivery History')}
        onClose={() => onOpenChange(false)}
      />
      <FullScreenDialogContent>
        <FullScreenDialogMain className="overflow-hidden">
          <div className="h-full flex flex-col">
            <div className="flex-1 overflow-y-auto px-4 py-4">
              <ResponsiveTable
                columns={columns}
                data={deliveryLogs as unknown as Record<string, unknown>[]}
                renderCell={renderCell}
                rowKey={(row) => (row as unknown as DeliveryLog).id}
                loading={deliveryLogsLoading}
                flexHeight={false}
                emptyState={
                  <EmptyState
                    icon={<FileText className="h-12 w-12" />}
                    title={t('common:dataPush.noLogs', 'No delivery logs')}
                    description={t('common:dataPush.noLogsHint', 'Logs will appear here after data is pushed')}
                  />
                }
              />
            </div>
            {deliveryLogsTotal > PAGE_SIZE && (
              <div className="border-t px-4 py-3">
                <Pagination
                  total={deliveryLogsTotal}
                  pageSize={PAGE_SIZE}
                  currentPage={page}
                  onPageChange={loadPage}
                  hideOnMobile={false}
                />
              </div>
            )}
          </div>
        </FullScreenDialogMain>
      </FullScreenDialogContent>
      <UnifiedFormDialog
        open={previewPayload !== null}
        onOpenChange={(o) => {
          if (!o) setPreviewPayload(null)
        }}
        title={t('common:dataPush.payloadPreview', 'Payload Preview')}
        icon={<Eye className="h-5 w-5" />}
        width="xl"
        className="z-[110]"
        contentClassName="max-h-[70vh]"
        footer={
          <div className="flex w-full justify-end gap-2">
            <Button
              variant="outline"
              size="sm"
              onClick={() => previewPayload && copyPayload(previewPayload)}
            >
              <Copy className="mr-2 h-4 w-4" /> {t('common:copy', 'Copy')}
            </Button>
            <Button variant="secondary" size="sm" onClick={() => setPreviewPayload(null)}>
              {t('common:close', 'Close')}
            </Button>
          </div>
        }
      >
        {previewPayload !== null &&
          (() => {
            let pretty = previewPayload
            try {
              pretty = JSON.stringify(JSON.parse(previewPayload), null, 2)
            } catch {
              /* raw payload is not JSON — show as-is */
            }
            const bytes = new Blob([previewPayload]).size
            return (
              <div className="space-y-2">
                <div className={cn(textNano, 'text-muted-foreground')}>
                  {new Intl.NumberFormat().format(bytes)} {t('common:dataPush.bytes', 'bytes')}
                </div>
                <pre className="text-xs font-mono whitespace-pre-wrap break-all bg-muted-30 rounded p-3 max-h-[55vh] overflow-auto">
                  {pretty}
                </pre>
              </div>
            )
          })()}
      </UnifiedFormDialog>
    </FullScreenDialog>
  )
}
