import { useState } from "react"
import { useTranslation } from "react-i18next"
import { ScrollArea } from "@/components/ui/scroll-area"
import { Button } from "@/components/ui/button"
import { Badge } from "@/components/ui/badge"
import { Checkbox } from "@/components/ui/checkbox"
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table"
import { LoadingState, EmptyState, Pagination, BulkActionBar } from "@/components/shared"
import { Eye, Trash2, RefreshCw, Radar, Plus, Home } from "lucide-react"
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
      <div className="flex items-center justify-between mb-4">
        <div className="flex gap-2">
          <Dialog>
            <DialogTrigger asChild>
              <Button size="sm" onClick={onAddDevice}>
                <Plus className="mr-2 h-4 w-4" />
                {t('devices:addDevice')}
              </Button>
            </DialogTrigger>
            {addDeviceDialog}
          </Dialog>
          <Button variant="outline" size="sm" onClick={onRefresh}>
            <RefreshCw className="mr-2 h-4 w-4" />
            {t('common:refresh')}
          </Button>
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
        </div>
      </div>

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
        <EmptyState
          icon={<Radar className="h-12 w-12 text-muted-foreground" />}
          title={t('devices:noDevices')}
          description={t('devices:startUsing')}
        />
      ) : (
        <ScrollArea className="flex-1">
          <div className="px-4">
            <Table>
              <TableHeader>
                <TableRow>
                  <TableHead className="w-[50px]">
                    <Checkbox checked={allOnPageSelected} onCheckedChange={toggleAll} />
                  </TableHead>
                  <TableHead>{t('devices:headers.id')}</TableHead>
                  <TableHead>{t('devices:headers.name')}</TableHead>
                  <TableHead>{t('devices:headers.type')}</TableHead>
                  <TableHead>{t('devices:headers.adapter')}</TableHead>
                  <TableHead>{t('devices:headers.status')}</TableHead>
                  <TableHead>{t('devices:headers.lastOnline')}</TableHead>
                  <TableHead>{t('devices:headers.actions')}</TableHead>
                </TableRow>
              </TableHeader>
              <TableBody>
                {paginatedDevices.map((device) => (
                  <TableRow key={device.id} className={cn(selectedIds.has(device.id) && "bg-muted/50")}>
                    <TableCell>
                      <Checkbox
                        checked={selectedIds.has(device.id)}
                        onCheckedChange={() => toggleSelection(device.id)}
                      />
                    </TableCell>
                    <TableCell className="font-mono text-xs">{device.id}</TableCell>
                    <TableCell>{device.name || "-"}</TableCell>
                    <TableCell className="text-xs">{device.device_type}</TableCell>
                    <TableCell>{device.plugin_name || "-"}</TableCell>
                    <TableCell>
                      {device.status === "online" ? (
                        <Badge className="bg-green-500 text-white">{t('devices:status.online')}</Badge>
                      ) : (
                        <Badge variant="secondary">{t('devices:status.offline')}</Badge>
                      )}
                    </TableCell>
                    <TableCell className="text-xs text-muted-foreground">
                      {new Date(device.last_seen).toLocaleString()}
                    </TableCell>
                    <TableCell>
                      <div className="flex gap-1">
                        <Button variant="ghost" size="icon" onClick={() => onViewDetails(device)}>
                          <Eye className="h-4 w-4" />
                        </Button>
                        <Button variant="ghost" size="icon" onClick={() => onDelete(device.id)}>
                          <Trash2 className="h-4 w-4" />
                        </Button>
                      </div>
                    </TableCell>
                  </TableRow>
                ))}
              </TableBody>
            </Table>
          </div>

          {devices.length > devicesPerPage && (
            <div className="px-4 pt-4">
              <Pagination
                total={devices.length}
                pageSize={devicesPerPage}
                currentPage={devicePage}
                onPageChange={onPageChange}
              />
            </div>
          )}
        </ScrollArea>
      )}
    </>
  )
}
