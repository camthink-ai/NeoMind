import { useTranslation } from "react-i18next"
import { Card } from "@/components/ui/card"
import { Button } from "@/components/ui/button"
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table"
import { EmptyStateInline, Pagination, StatusBadge } from "@/components/shared"
import { Badge } from "@/components/ui/badge"
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu"
import { Eye, MoreVertical, Trash2, Cpu, Database, Waves } from "lucide-react"
import { cn } from "@/lib/utils"
import type { Device } from "@/types"
import { TransformsBadge } from "@/components/automation"
import { useDeviceEvents } from "@/hooks/useEvents"
import { useStore } from "@/store"

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
  const updateDeviceStatus = useStore((state) => state.updateDeviceStatus)

  // Listen to device status change events
  useDeviceEvents({
    enabled: true,
    eventTypes: ['DeviceOnline', 'DeviceOffline'],
    onEvent: (event) => {
      if (event.type === 'DeviceOnline' || event.type === 'DeviceOffline') {
        const data = event.data as { device_id: string }
        if (data.device_id) {
          updateDeviceStatus(data.device_id, event.type === 'DeviceOnline' ? 'online' : 'offline')
        }
      }
    },
  })

  // Get adapter icon
  const getAdapterIcon = (adapter: string) => {
    const lower = adapter?.toLowerCase() || ''
    if (lower.includes('mqtt') || lower === 'mqtt') return Database
    if (lower.includes('modbus')) return Cpu
    return Waves
  }

  return (
    <>
      {/* Dialogs (由上层 TAB 操作按钮控制 open 状态) */}
      {addDeviceDialog}
      {discoveryDialog}

      <Card className="overflow-hidden">
        <Table>
          <TableHeader>
            <TableRow className="hover:bg-transparent border-b bg-muted/30">
              <TableHead className="w-10 text-center">#</TableHead>
              <TableHead>
                <div className="flex items-center gap-2 text-xs font-semibold uppercase tracking-wide text-muted-foreground">
                  <Cpu className="h-4 w-4" />
                  {t('devices:headers.name')}
                </div>
              </TableHead>
              <TableHead>
                <div className="flex items-center gap-2 text-xs font-semibold uppercase tracking-wide text-muted-foreground">
                  <Database className="h-4 w-4" />
                  {t('devices:headers.type')}
                </div>
              </TableHead>
              <TableHead>
                <div className="flex items-center gap-2 text-xs font-semibold uppercase tracking-wide text-muted-foreground">
                  <Waves className="h-4 w-4" />
                  {t('devices:headers.adapter')}
                </div>
              </TableHead>
              <TableHead align="center">
                <div className="flex items-center justify-center gap-2 text-xs font-semibold uppercase tracking-wide text-muted-foreground">
                  {t('automation:transforms', { defaultValue: 'Transforms' })}
                </div>
              </TableHead>
              <TableHead align="center">
                <div className="flex items-center justify-center gap-2 text-xs font-semibold uppercase tracking-wide text-muted-foreground">
                  {t('devices:headers.status')}
                </div>
              </TableHead>
              <TableHead align="center">
                <div className="flex items-center justify-center gap-2 text-xs font-semibold uppercase tracking-wide text-muted-foreground">
                  {t('devices:headers.lastOnline')}
                </div>
              </TableHead>
              <TableHead className="w-12"></TableHead>
            </TableRow>
          </TableHeader>
          <TableBody>
            {loading ? (
              <EmptyStateInline title={t('common:loading')} colSpan={8} />
            ) : devices.length === 0 ? (
              <EmptyStateInline title={t('devices:noDevices')} colSpan={8} />
            ) : (
              paginatedDevices.map((device, index) => {
                const AdapterIcon = getAdapterIcon(device.adapter_type)
                return (
                  <TableRow key={device.id} className="group transition-colors hover:bg-muted/50">
                    <TableCell className="text-center">
                      <span className="text-xs text-muted-foreground font-medium">{index + 1}</span>
                    </TableCell>
                    <TableCell>
                      <div className="flex items-center gap-3">
                        <div className={cn(
                          "w-9 h-9 rounded-lg flex items-center justify-center transition-colors",
                          device.status === 'online'
                            ? "bg-green-500/10 text-green-600"
                            : "bg-muted text-muted-foreground"
                        )}>
                          <Cpu className="h-4 w-4" />
                        </div>
                        <div>
                          <div className="font-medium text-sm">{device.name || "-"}</div>
                          <code className="text-xs text-muted-foreground font-mono">{device.id}</code>
                        </div>
                      </div>
                    </TableCell>
                    <TableCell>
                      <Badge variant="outline" className="text-xs">
                        {device.device_type}
                      </Badge>
                    </TableCell>
                    <TableCell>
                      <div className="flex items-center gap-2">
                        <AdapterIcon className="h-3.5 w-3.5 text-muted-foreground" />
                        <Badge variant="outline" className="text-xs">
                          {device.adapter_type || 'mqtt'}
                        </Badge>
                      </div>
                    </TableCell>
                    <TableCell align="center">
                      <TransformsBadge deviceId={device.id} onRefresh={onRefresh} />
                    </TableCell>
                    <TableCell align="center">
                      <StatusBadge status={device.status} />
                    </TableCell>
                    <TableCell align="center">
                      <span className="text-xs text-muted-foreground">
                        {new Date(device.last_seen).toLocaleString()}
                      </span>
                    </TableCell>
                    <TableCell>
                      <DropdownMenu>
                        <DropdownMenuTrigger asChild>
                          <Button variant="ghost" size="icon" className="h-8 w-8 opacity-0 group-hover:opacity-100 transition-opacity">
                            <MoreVertical className="h-4 w-4" />
                          </Button>
                        </DropdownMenuTrigger>
                        <DropdownMenuContent align="end" className="w-40">
                          <DropdownMenuItem onClick={() => onViewDetails(device)}>
                            <Eye className="mr-2 h-4 w-4" />
                            {t('devices:actions.viewDetails')}
                          </DropdownMenuItem>
                          <DropdownMenuSeparator />
                          <DropdownMenuItem
                            onClick={() => onDelete(device.id)}
                            className="text-destructive"
                          >
                            <Trash2 className="mr-2 h-4 w-4" />
                            {t('common:delete')}
                          </DropdownMenuItem>
                        </DropdownMenuContent>
                      </DropdownMenu>
                    </TableCell>
                  </TableRow>
                )
              })
            )}
          </TableBody>
        </Table>
      </Card>

      {devices.length > devicesPerPage && (
        <div className="sticky bottom-0 bg-background/95 backdrop-blur supports-[backdrop-filter]:bg-background/60 pt-4 pb-2 border-t mt-4">
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
