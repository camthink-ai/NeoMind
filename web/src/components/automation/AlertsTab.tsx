import { useEffect, useState, useRef } from "react"
import { useTranslation } from "react-i18next"
import { useStore } from "@/store"
import { Button } from "@/components/ui/button"
import { Card } from "@/components/ui/card"
import { Checkbox } from "@/components/ui/checkbox"
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog"
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table"
import { Badge } from "@/components/ui/badge"
import { Check, Trash2, Eye, Bell } from "lucide-react"
import { EmptyStateInline, Pagination, AlertBadge, BulkActionBar } from "@/components/shared"
import { formatTimestamp } from "@/lib/utils/format"
import { api } from "@/lib/api"
import { useToast } from "@/hooks/use-toast"
import { confirm } from "@/hooks/use-confirm"
import type { Alert } from "@/types"

export function AlertsTab() {
  const { t } = useTranslation(['common', 'alerts'])
  const { alerts, alertsLoading, fetchAlerts, acknowledgeAlert } = useStore()
  const { toast } = useToast()

  // Pagination state
  const [page, setPage] = useState(1)
  const alertsPerPage = 20

  // Bulk selection state
  const [selectedIds, setSelectedIds] = useState<Set<string>>(new Set())
  const [bulkProcessing, setBulkProcessing] = useState(false)

  // Acknowledging state
  const [acknowledgingId, setAcknowledgingId] = useState<string | null>(null)

  // Detail dialog state
  const [selectedAlert, setSelectedAlert] = useState<Alert | null>(null)

  // Fetch alerts on mount (once)
  const hasFetchedAlerts = useRef(false)
  useEffect(() => {
    if (!hasFetchedAlerts.current) {
      hasFetchedAlerts.current = true
      fetchAlerts()
    }
  }, [])

  // Reset pagination when alerts change
  useEffect(() => {
    setPage(1)
  }, [alerts.length])

  // Paginated alerts
  const paginatedAlerts = alerts.slice(
    (page - 1) * alertsPerPage,
    page * alertsPerPage
  )

  const handleAcknowledge = async (id: string) => {
    setAcknowledgingId(id)
    try {
      await acknowledgeAlert(id)
      toast({ title: t('common:success'), description: t('alerts:acknowledged') })
    } finally {
      setAcknowledgingId(null)
    }
  }

  // Toggle selection
  const toggleSelection = (id: string) => {
    setSelectedIds((prev) => {
      const next = new Set(prev)
      if (next.has(id)) {
        next.delete(id)
      } else {
        next.add(id)
      }
      return next
    })
  }

  // Toggle all on current page
  const toggleAll = () => {
    const pageIds = new Set(paginatedAlerts.map((a) => a.id))
    if (paginatedAlerts.every((a) => selectedIds.has(a.id))) {
      setSelectedIds((prev) => {
        const next = new Set(prev)
        pageIds.forEach((id) => next.delete(id))
        return next
      })
    } else {
      setSelectedIds((prev) => new Set([...prev, ...pageIds]))
    }
  }

  // Bulk actions
  const handleBulkAcknowledge = async () => {
    if (selectedIds.size === 0) return
    setBulkProcessing(true)
    try {
      const response = await api.bulkAcknowledgeAlerts(Array.from(selectedIds))
      if (response.acknowledged) {
        toast({ title: t('common:success'), description: t('alerts:acknowledgedCount', { count: response.acknowledged }) })
        setSelectedIds(new Set())
        await fetchAlerts()
      }
    } catch (error) {
      toast({ title: t('common:failed'), description: t('alerts:bulkAcknowledgeFailed'), variant: "destructive" })
    } finally {
      setBulkProcessing(false)
    }
  }

  const handleBulkDelete = async () => {
    if (selectedIds.size === 0) return

    const confirmed = await confirm({
      title: t('common:delete'),
      description: t('alerts:deleteConfirm', { count: selectedIds.size }),
      confirmText: t('common:delete'),
      cancelText: t('common:cancel'),
      variant: "destructive"
    })
    if (!confirmed) return

    setBulkProcessing(true)
    try {
      const response = await api.bulkDeleteAlerts(Array.from(selectedIds))
      if (response.deleted) {
        toast({ title: t('common:success'), description: t('alerts:deletedCount', { count: response.deleted }) })
        setSelectedIds(new Set())
        await fetchAlerts()
      }
    } catch (error) {
      toast({ title: t('common:failed'), description: t('alerts:bulkDeleteFailed'), variant: "destructive" })
    } finally {
      setBulkProcessing(false)
    }
  }

  const allOnPageSelected = paginatedAlerts.length > 0 && paginatedAlerts.every((a) => selectedIds.has(a.id))

  return (
    <div className="space-y-4">
      {/* Bulk Actions Bar */}
      <BulkActionBar
        selectedCount={selectedIds.size}
        actions={[
          { label: t('alerts:acknowledgeSelected'), icon: <Check className="h-4 w-4" />, onClick: handleBulkAcknowledge, disabled: bulkProcessing },
          { label: t('alerts:deleteSelected'), icon: <Trash2 className="h-4 w-4" />, onClick: handleBulkDelete, disabled: bulkProcessing, variant: 'outline' },
        ]}
        onCancel={() => setSelectedIds(new Set())}
      />

      {/* Table */}
      <Card>
        <Table>
          <TableHeader>
            <TableRow>
              <TableHead className="w-[40px]">
                <Checkbox
                  checked={allOnPageSelected}
                  onCheckedChange={toggleAll}
                />
              </TableHead>
              <TableHead>{t('alerts:alertTitle')}</TableHead>
              <TableHead>{t('alerts:severity')}</TableHead>
              <TableHead>{t('common:status')}</TableHead>
              <TableHead>{t('alerts:source')}</TableHead>
              <TableHead>{t('alerts:createdAt')}</TableHead>
              <TableHead align="right">{t('automation:actions')}</TableHead>
            </TableRow>
          </TableHeader>
          <TableBody>
            {alertsLoading ? (
              <EmptyStateInline title={t('common:loading')} colSpan={7} />
            ) : alerts.length === 0 ? (
              <EmptyStateInline title={`${t('alerts:noAlerts')} - ${t('alerts:noAlertsDesc')}`} colSpan={7} />
            ) : paginatedAlerts.length === 0 ? (
              <EmptyStateInline title={t('alerts:noAlertsOnPage')} colSpan={7} />
            ) : (
              paginatedAlerts.map((alert) => (
                <TableRow
                  key={alert.id}
                  className={!alert.acknowledged ? (
                    alert.severity === 'critical' ? 'bg-red-500/5' :
                    alert.severity === 'warning' ? 'bg-yellow-500/5' :
                    'bg-blue-500/5'
                  ) : ''}
                >
                  <TableCell>
                    <Checkbox
                      checked={selectedIds.has(alert.id)}
                      onCheckedChange={() => toggleSelection(alert.id)}
                    />
                  </TableCell>
                  <TableCell>
                    <div className="max-w-md">
                      <div className="font-medium truncate">{alert.title}</div>
                      <div className="text-xs text-muted-foreground truncate">{alert.message}</div>
                    </div>
                  </TableCell>
                  <TableCell>
                    <AlertBadge level={alert.severity as "critical" | "warning" | "info" | "emergency"} />
                  </TableCell>
                  <TableCell>
                    {alert.acknowledged ? (
                      <Badge variant="outline">{t('alerts:acknowledged')}</Badge>
                    ) : (
                      <Badge variant="default">{t('alerts:unacknowledged')}</Badge>
                    )}
                  </TableCell>
                  <TableCell>
                    {alert.source && (
                      <Badge variant="outline" className="text-xs">
                        {alert.source}
                      </Badge>
                    )}
                  </TableCell>
                  <TableCell className="text-sm text-muted-foreground">
                    {formatTimestamp(alert.created_at || alert.timestamp)}
                  </TableCell>
                  <TableCell align="right">
                    <div className="flex items-center justify-end gap-1">
                      <Button
                        variant="ghost"
                        size="icon"
                        className="h-8 w-8"
                        onClick={() => setSelectedAlert(alert)}
                      >
                        <Eye className="h-4 w-4" />
                      </Button>
                      {!alert.acknowledged && (
                        <Button
                          variant="ghost"
                          size="icon"
                          className="h-8 w-8"
                          onClick={() => handleAcknowledge(alert.id)}
                          disabled={acknowledgingId === alert.id}
                        >
                          <Check className="h-4 w-4" />
                        </Button>
                      )}
                    </div>
                  </TableCell>
                </TableRow>
              ))
            )}
          </TableBody>
        </Table>

        {/* Pagination */}
        {alerts.length > alertsPerPage && (
          <div className="px-4 pt-4 border-t">
            <Pagination
              total={alerts.length}
              pageSize={alertsPerPage}
              currentPage={page}
              onPageChange={setPage}
            />
          </div>
        )}
      </Card>

      {/* Alert Detail Dialog */}
      <Dialog open={!!selectedAlert} onOpenChange={() => setSelectedAlert(null)}>
        <DialogContent className="max-w-lg">
          <DialogHeader>
            <DialogTitle className="flex items-center gap-2">
              <Bell className="h-5 w-5" />
              {selectedAlert?.title}
            </DialogTitle>
            <DialogDescription>
              {selectedAlert && <AlertBadge level={selectedAlert.severity as "critical" | "warning" | "info" | "emergency"} />}
            </DialogDescription>
          </DialogHeader>
          {selectedAlert && (
            <div className="space-y-4">
              <div className="grid grid-cols-2 gap-4 text-sm">
                <div>
                  <span className="text-muted-foreground">{t('common:status')}:</span>{' '}
                  {selectedAlert.acknowledged ? (
                    <Badge variant="outline">{t('alerts:acknowledged')}</Badge>
                  ) : (
                    <Badge variant="default">{t('alerts:unacknowledged')}</Badge>
                  )}
                </div>
                <div>
                  <span className="text-muted-foreground">{t('alerts:source')}:</span>{' '}
                  <span className="font-medium">{selectedAlert.source || 'N/A'}</span>
                </div>
                <div className="col-span-2">
                  <span className="text-muted-foreground">{t('alerts:createdAt')}:</span>{' '}
                  <span className="font-medium">{formatTimestamp(selectedAlert.created_at)}</span>
                </div>
              </div>

              <div>
                <h4 className="font-medium mb-2">{t('alerts:detailDescription')}</h4>
                <p className="text-sm text-muted-foreground whitespace-pre-wrap">{selectedAlert.message}</p>
              </div>
            </div>
          )}
          <DialogFooter>
            {!selectedAlert?.acknowledged && (
              <Button
                onClick={() => {
                  if (selectedAlert) {
                    handleAcknowledge(selectedAlert.id)
                    setSelectedAlert(null)
                  }
                }}
                disabled={acknowledgingId === selectedAlert?.id}
              >
                {t('alerts:confirmAlert')}
              </Button>
            )}
            <Button variant="outline" onClick={() => setSelectedAlert(null)}>
              {t('common:close')}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </div>
  )
}
