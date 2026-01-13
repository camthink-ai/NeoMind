import { useEffect, useState, useMemo, useRef } from "react"
import { useTranslation } from "react-i18next"
import { useStore } from "@/store"
import { Button } from "@/components/ui/button"
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card"
import { Input } from "@/components/ui/input"
import { Label } from "@/components/ui/label"
import { Textarea } from "@/components/ui/textarea"
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
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select"
import { Badge } from "@/components/ui/badge"
import { AlertCircle, Plus, Check, Trash2, Eye, Bell, RefreshCw } from "lucide-react"
import { LoadingState, EmptyState, Pagination, AlertBadge, BulkActionBar } from "@/components/shared"
import { formatTimestamp } from "@/lib/utils/format"
import { api } from "@/lib/api"
import { useToast } from "@/hooks/use-toast"
import type { Alert } from "@/types"

type AlertFilter = 'all' | 'unacknowledged' | 'info' | 'warning' | 'critical'

export function AlertsTab() {
  const { t } = useTranslation(['common', 'alerts'])
  const { alerts, alertsLoading, fetchAlerts, acknowledgeAlert, createAlert } = useStore()
  const { toast } = useToast()

  // Filter state
  const [filter, setFilter] = useState<AlertFilter>('all')

  // Pagination state
  const [page, setPage] = useState(1)
  const alertsPerPage = 20

  // Bulk selection state
  const [selectedIds, setSelectedIds] = useState<Set<string>>(new Set())
  const [bulkProcessing, setBulkProcessing] = useState(false)

  // Create alert dialog state
  const [createDialogOpen, setCreateDialogOpen] = useState(false)
  const [newAlertTitle, setNewAlertTitle] = useState("")
  const [newAlertMessage, setNewAlertMessage] = useState("")
  const [newAlertSeverity, setNewAlertSeverity] = useState<"info" | "warning" | "critical">("info")
  const [creating, setCreating] = useState(false)
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

  // Reset selection when filter changes
  useEffect(() => {
    setSelectedIds(new Set())
  }, [filter])

  // Filter alerts
  const filteredAlerts = useMemo(() => {
    if (filter === 'all') return alerts
    if (filter === 'unacknowledged') return alerts.filter(a => !a.acknowledged)
    return alerts.filter(a => a.severity === filter)
  }, [alerts, filter])

  // Paginated alerts
  const paginatedAlerts = filteredAlerts.slice(
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

  const handleCreateAlert = async () => {
    if (!newAlertTitle.trim() || !newAlertMessage.trim()) return

    setCreating(true)
    try {
      await createAlert({
        title: newAlertTitle,
        message: newAlertMessage,
        severity: newAlertSeverity,
        source: "manual",
      })
      toast({ title: t('common:success'), description: t('alerts:alertCreated') })
      setCreateDialogOpen(false)
      setNewAlertTitle("")
      setNewAlertMessage("")
      setNewAlertSeverity("info")
    } finally {
      setCreating(false)
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
    if (!confirm(t('alerts:deleteConfirm', { count: selectedIds.size }))) return

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

  const filters = [
    { value: 'all' as AlertFilter, label: t('alerts:all') },
    { value: 'unacknowledged' as AlertFilter, label: t('alerts:unacknowledged') },
    { value: 'info' as AlertFilter, label: t('alerts:info') },
    { value: 'warning' as AlertFilter, label: t('alerts:warning') },
    { value: 'critical' as AlertFilter, label: t('alerts:critical') },
  ]

  return (
    <div className="space-y-4">
      {/* Header */}
      <div className="flex items-center justify-between">
        <div>
          <h2 className="text-xl font-semibold">{t('automation:alerts')}</h2>
          <p className="text-sm text-muted-foreground">
            {t('automation:alertsDesc')}
          </p>
        </div>
        <div className="flex gap-2">
          <Button variant="outline" size="sm" onClick={fetchAlerts}>
            <RefreshCw className="h-4 w-4 mr-2" />
            {t('common:refresh')}
          </Button>
          <Button size="sm" onClick={() => setCreateDialogOpen(true)}>
            <Plus className="h-4 w-4 mr-2" />
            {t('alerts:createAlert')}
          </Button>
        </div>
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

      {/* Bulk Actions Bar */}
      <BulkActionBar
        selectedCount={selectedIds.size}
        actions={[
          { label: t('alerts:acknowledgeSelected'), icon: <Check className="h-4 w-4" />, onClick: handleBulkAcknowledge, disabled: bulkProcessing },
          { label: t('alerts:deleteSelected'), icon: <Trash2 className="h-4 w-4" />, onClick: handleBulkDelete, disabled: bulkProcessing, variant: 'outline' },
        ]}
        onCancel={() => setSelectedIds(new Set())}
      />

      {/* Content */}
      {alertsLoading ? (
        <LoadingState text={t('common:loading')} />
      ) : filteredAlerts.length === 0 ? (
        <EmptyState
          icon={<AlertCircle className="h-12 w-12 text-muted-foreground" />}
          title={t('alerts:noAlerts')}
          description={t('alerts:noAlertsDesc')}
        />
      ) : (
        <div className="space-y-4">
          {/* Header with Select All */}
          <div className="flex items-center gap-2 px-2 py-1 text-sm text-muted-foreground">
            <Checkbox
              checked={allOnPageSelected}
              onCheckedChange={toggleAll}
            />
            <span>{t('common:selectAll')}</span>
          </div>

          {paginatedAlerts.map((alert) => (
            <Card
              key={alert.id}
              className={
                !alert.acknowledged
                  ? alert.severity === 'critical'
                    ? 'border-l-4 border-l-red-500'
                    : alert.severity === 'warning'
                    ? 'border-l-4 border-l-yellow-500'
                    : 'border-l-4 border-l-blue-500'
                  : ''
              }
            >
              <CardHeader className="pb-3">
                <div className="flex items-start justify-between gap-3">
                  <div className="flex items-start gap-3 flex-1">
                    <Checkbox
                      checked={selectedIds.has(alert.id)}
                      onCheckedChange={() => toggleSelection(alert.id)}
                    />
                    <div className="flex-1">
                      <div className="flex items-center gap-2 mb-1 flex-wrap">
                        <CardTitle className="text-base">{alert.title}</CardTitle>
                        <AlertBadge level={alert.severity as "critical" | "warning" | "info" | "emergency"} />
                        {!alert.acknowledged && (
                          <Badge variant="default" className="text-xs">{t('alerts:unacknowledged')}</Badge>
                        )}
                        {alert.source && (
                          <Badge variant="outline" className="text-xs">
                            {alert.source}
                          </Badge>
                        )}
                      </div>
                      <CardDescription className="text-xs">
                        {formatTimestamp(alert.created_at)}
                      </CardDescription>
                    </div>
                  </div>
                  <div className="flex gap-2">
                    <Button
                      onClick={() => setSelectedAlert(alert)}
                      variant="ghost"
                      size="sm"
                    >
                      <Eye className="h-3 w-3" />
                    </Button>
                    {!alert.acknowledged && (
                      <Button
                        variant="outline"
                        size="sm"
                        onClick={() => handleAcknowledge(alert.id)}
                        disabled={acknowledgingId === alert.id}
                      >
                        <Check className="mr-1 h-3 w-3" />
                        {acknowledgingId === alert.id ? t('alerts:acknowledging') : t('alerts:acknowledge')}
                      </Button>
                    )}
                  </div>
                </div>
              </CardHeader>
              <CardContent className="text-sm">
                <p className="text-muted-foreground">{alert.message}</p>
              </CardContent>
            </Card>
          ))}

          {/* Pagination */}
          {filteredAlerts.length > alertsPerPage && (
            <div className="pt-4">
              <Pagination
                total={filteredAlerts.length}
                pageSize={alertsPerPage}
                currentPage={page}
                onPageChange={setPage}
              />
            </div>
          )}
        </div>
      )}

      {/* Create Alert Dialog */}
      <Dialog open={createDialogOpen} onOpenChange={setCreateDialogOpen}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>{t('alerts:createAlert')}</DialogTitle>
            <DialogDescription>
              {t('alerts:manualCreateDesc')}
            </DialogDescription>
          </DialogHeader>
          <div className="space-y-4">
            <div>
              <Label htmlFor="alert-title">{t('alerts:alertTitle')}</Label>
              <Input
                id="alert-title"
                value={newAlertTitle}
                onChange={(e) => setNewAlertTitle(e.target.value)}
                placeholder={t('alerts:titlePlaceholder')}
              />
            </div>
            <div>
              <Label htmlFor="alert-severity">{t('alerts:severity')}</Label>
              <Select value={newAlertSeverity} onValueChange={(v: any) => setNewAlertSeverity(v)}>
                <SelectTrigger id="alert-severity">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value="info">{t('alerts:info')}</SelectItem>
                  <SelectItem value="warning">{t('alerts:warning')}</SelectItem>
                  <SelectItem value="critical">{t('alerts:critical')}</SelectItem>
                </SelectContent>
              </Select>
            </div>
            <div>
              <Label htmlFor="alert-message">{t('alerts:detailDescription')}</Label>
              <Textarea
                id="alert-message"
                value={newAlertMessage}
                onChange={(e) => setNewAlertMessage(e.target.value)}
                placeholder={t('alerts:descriptionPlaceholder')}
                className="min-h-[80px]"
              />
            </div>
          </div>
          <DialogFooter>
            <Button variant="outline" onClick={() => setCreateDialogOpen(false)}>
              {t('common:cancel')}
            </Button>
            <Button
              onClick={handleCreateAlert}
              disabled={!newAlertTitle.trim() || !newAlertMessage.trim() || creating}
            >
              {creating ? t('common:creating') : t('common:add')}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

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
