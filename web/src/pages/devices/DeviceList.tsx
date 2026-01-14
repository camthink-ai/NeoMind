import { useState } from "react"
import { useTranslation } from "react-i18next"
import { Button } from "@/components/ui/button"
import { Badge } from "@/components/ui/badge"
import { Card } from "@/components/ui/card"
import { Checkbox } from "@/components/ui/checkbox"
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table"
import { LoadingState, EmptyStateInline, Pagination, BulkActionBar, ActionBar, StatusBadge } from "@/components/shared"
import { Eye, Trash2, Home, Server, Plus, Radar } from "lucide-react"
import { Dialog, DialogTrigger } from "@/components/ui/dialog"
import { cn } from "@/lib/utils"
import type { Device } from "@/types"
import { api } from "@/lib/api"
import { useToast } from "@/hooks/use-toast"

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
  hassDiscoveryDialogOpen?: boolean
  onHassDiscoveryOpenChange?: (open: boolean) => void
  hassDiscoveryDialog?: React.ReactNode
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
  onAddDevice,
  discoveryDialogOpen,
  onDiscoveryOpenChange,
  discoveryDialog,
  addDeviceDialog,
  hassDiscoveryDialogOpen,
  onHassDiscoveryOpenChange,
  hassDiscoveryDialog,
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
      {/* Toolbar */}
      <ActionBar
        title={t('devices:deviceList')}
        titleIcon={<Server className="h-5 w-5" />}
        description={`${devices.length} ${t('devices:totalDevices')}`}
        actions={
          <>
            <Dialog>
              <DialogTrigger asChild>
                <Button size="sm" onClick={onAddDevice}>
                  <Plus className="mr-2 h-4 w-4" />
                  {t('devices:addDevice')}
                </Button>
              </DialogTrigger>
              {addDeviceDialog}
            </Dialog>
            <Dialog open={discoveryDialogOpen} onOpenChange={onDiscoveryOpenChange}>
              <DialogTrigger asChild>
                <Button variant="outline" size="sm">
                  <Radar className="mr-2 h-4 w-4" />
                  {t('devices:localNetworkScan')}
                </Button>
              </DialogTrigger>
              {discoveryDialog}
            </Dialog>
            {onHassDiscoveryOpenChange && hassDiscoveryDialog && (
              <Dialog open={hassDiscoveryDialogOpen} onOpenChange={onHassDiscoveryOpenChange}>
                <DialogTrigger asChild>
                  <Button variant="outline" size="sm">
                    <Home className="mr-2 h-4 w-4" />
                    {t('devices:hassDiscovery')}
                  </Button>
                </DialogTrigger>
                {hassDiscoveryDialog}
              </Dialog>
            )}
          </>
        }
      />

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

      {loading ? (
        <LoadingState text={t('devices:loading')} />
      ) : devices.length === 0 ? (
        <EmptyStateInline title={t('devices:noDevices')} colSpan={8} />
      ) : (
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
                <TableHead align="center">{t('devices:headers.status')}</TableHead>
                <TableHead>{t('devices:headers.lastOnline')}</TableHead>
                <TableHead align="right">{t('devices:headers.actions')}</TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              {paginatedDevices.map((device) => (
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
                  <TableCell>{device.plugin_name || "-"}</TableCell>
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
              ))}
            </TableBody>
          </Table>
        </Card>
      )}

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
