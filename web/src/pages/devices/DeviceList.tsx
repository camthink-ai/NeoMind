import { useState } from "react"
import { useTranslation } from "react-i18next"
import { Card } from "@/components/ui/card"
import { Button } from "@/components/ui/button"
import { Checkbox } from "@/components/ui/checkbox"
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table"
import { EmptyStateInline, Pagination, BulkActionBar, StatusBadge } from "@/components/shared"
import { Badge } from "@/components/ui/badge"
import { Eye, Trash2 } from "lucide-react"
import { cn } from "@/lib/utils"
import type { Device } from "@/types"
import { api } from "@/lib/api"
import { useToast } from "@/hooks/use-toast"
import { TransformsBadge } from "@/components/automation"

interface DeviceListProps {
  devices: Device[]
  loading: boolean
  paginatedDevices: Device[]
  devicePage: number
  devicesPerPage: number
  onRefresh: () => void
  onViewDetails: (device: Device) => void
  onDelete: (id: string) => void
  onPageChange: (page: number) => void
  onAddDevice: () => void
  discoveryDialogOpen: boolean
  onDiscoveryOpenChange: (open: boolean) => void
  discoveryDialog: React.ReactNode
  addDeviceDialog: React.ReactNode
}

export function DeviceList({
  devices,
  loading,
  paginatedDevices,
  devicePage,
  devicesPerPage,
  onRefresh,
  onViewDetails,
  onDelete,
  onPageChange,
  onAddDevice: _onAddDevice,
  discoveryDialogOpen: _discoveryDialogOpen,
  onDiscoveryOpenChange: _onDiscoveryOpenChange,
  discoveryDialog,
  addDeviceDialog,
}: DeviceListProps) {
  const { t } = useTranslation(['common', 'devices'])
  const { toast } = useToast()
  const [selectedIds, setSelectedIds] = useState<Set<string>>(new Set())
  const [bulkProcessing, setBulkProcessing] = useState(false)

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

  const toggleAll = () => {
    const pageIds = new Set(paginatedDevices.map((d) => d.id))
    if (paginatedDevices.every((d) => selectedIds.has(d.id))) {
      setSelectedIds((prev) => {
        const next = new Set(prev)
        pageIds.forEach((id) => next.delete(id))
        return next
      })
    } else {
      setSelectedIds((prev) => new Set([...prev, ...pageIds]))
    }
  }

  const handleBulkDelete = async () => {
    if (selectedIds.size === 0) return
    if (!confirm(t('devices:confirmDeleteSelected', { count: selectedIds.size }))) return

    setBulkProcessing(true)
    try {
      const response = await api.bulkDeleteDevices(Array.from(selectedIds))
      if (response.deleted) {
        toast({ title: t('common:success'), description: t('devices:deletedCount', { count: response.deleted }) })
        setSelectedIds(new Set())
        onRefresh()
      }
    } catch (error) {
      toast({ title: t('common:failed'), description: t('devices:bulkDeleteFailed'), variant: "destructive" })
    } finally {
      setBulkProcessing(false)
    }
  }

  const allOnPageSelected = paginatedDevices.length > 0 && paginatedDevices.every((d) => selectedIds.has(d.id))

  return (
    <>
      {/* Bulk Actions Bar */}
      <BulkActionBar
        selectedCount={selectedIds.size}
        actions={[
          {
            label: t('common:delete'),
            icon: <Trash2 className="h-4 w-4" />,
            onClick: handleBulkDelete,
            disabled: bulkProcessing,
            variant: "outline",
          },
        ]}
        onCancel={() => setSelectedIds(new Set())}
      />

      {/* Dialogs (由上层 TAB 操作按钮控制 open 状态) */}
      {addDeviceDialog}
      {discoveryDialog}

      <Card>
        <Table>
          <TableHeader>
            <TableRow>
              <TableHead align="center" className="w-[50px]">
                <Checkbox checked={allOnPageSelected} onCheckedChange={toggleAll} />
              </TableHead>
              <TableHead>{t('devices:headers.id')}</TableHead>
              <TableHead>{t('devices:headers.name')}</TableHead>
              <TableHead>{t('devices:headers.type')}</TableHead>
              <TableHead>{t('devices:headers.adapter')}</TableHead>
              <TableHead align="center">{t('automation:transforms', { defaultValue: 'Transforms' })}</TableHead>
              <TableHead align="center">{t('devices:headers.status')}</TableHead>
              <TableHead>{t('devices:headers.lastOnline')}</TableHead>
              <TableHead align="right">{t('devices:headers.actions')}</TableHead>
            </TableRow>
          </TableHeader>
          <TableBody>
            {loading ? (
              <EmptyStateInline title={t('common:loading')} colSpan={9} />
            ) : devices.length === 0 ? (
              <EmptyStateInline title={t('devices:noDevices')} colSpan={9} />
            ) : (
                paginatedDevices.map((device) => (
                  <TableRow key={device.id} className={cn(selectedIds.has(device.id) && "bg-muted/50")}>
                  <TableCell align="center">
                    <Checkbox
                      checked={selectedIds.has(device.id)}
                      onCheckedChange={() => toggleSelection(device.id)}
                    />
                  </TableCell>
                  <TableCell className="font-mono text-xs">{device.id}</TableCell>
                  <TableCell>{device.name || "-"}</TableCell>
                  <TableCell className="text-xs">{device.device_type}</TableCell>
                  <TableCell>
                    <Badge variant="outline" className="text-xs">
                      {device.adapter_type || 'mqtt'}
                    </Badge>
                  </TableCell>
                  <TableCell align="center">
                    <TransformsBadge deviceId={device.id} onRefresh={onRefresh} />
                  </TableCell>
                  <TableCell align="center">
                    <StatusBadge status={device.status} />
                  </TableCell>
                  <TableCell className="text-xs text-muted-foreground">
                    {new Date(device.last_seen).toLocaleString()}
                  </TableCell>
                  <TableCell align="right">
                    <div className="flex justify-end gap-1">
                      <Button variant="ghost" size="icon" className="h-8 w-8" onClick={() => onViewDetails(device)}>
                        <Eye className="h-4 w-4" />
                      </Button>
                      <Button variant="ghost" size="icon" className="h-8 w-8" onClick={() => onDelete(device.id)}>
                        <Trash2 className="h-4 w-4" />
                      </Button>
                    </div>
                  </TableCell>
                </TableRow>
                ))
            )}
          </TableBody>
        </Table>
      </Card>

      {devices.length > devicesPerPage && (
        <div className="pt-4">
          <Pagination
            total={devices.length}
            pageSize={devicesPerPage}
            currentPage={devicePage}
            onPageChange={onPageChange}
          />
        </div>
      )}
    </>
  )
}
