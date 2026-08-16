import { useTranslation } from "react-i18next"
import { Badge } from "@/components/ui/badge"
import { Card } from "@/components/ui/card"
import { DropdownMenu, DropdownMenuTrigger, DropdownMenuContent, DropdownMenuItem } from "@/components/ui/dropdown-menu"
import { IconButton } from "@/components/ui/button"
import { ResponsiveTable, StatusBadge, EmptyState } from "@/components/shared"
import { DeviceStatusBadge } from "@/components/shared/DeviceStatusBadge"
import { Eye, MoreVertical, Trash2, Cpu, Database, Waves, Pencil } from "lucide-react"
import { cn } from "@/lib/utils"
import { interactiveCardHover } from "@/design-system/tokens/size"
import { textMini } from "@/design-system/tokens/typography"
import { formatTimestamp } from "@/lib/utils/format"
import type { Device } from "@/types"
import { TransformsBadge } from "@/components/automation"
import { useDeviceEvents } from "@/hooks/useEvents"
import { useIsMobile } from "@/hooks/useMobile"
import { useStore } from "@/store"
import { useTransformCounts } from "@/hooks/useTransformCounts"

interface DeviceListProps {
  devices: Device[]
  loading: boolean
  paginatedDevices: Device[]
  devicePage: number
  devicesPerPage: number
  onRefresh: () => void
  onViewDetails: (device: Device) => void
  onEdit: (device: Device) => void
  onDelete: (id: string) => void
  onPageChange: (page: number) => void
  onAddDevice: () => void
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
  onEdit,
  onDelete,
  onPageChange,
  onAddDevice: _onAddDevice,
  addDeviceDialog,
}: DeviceListProps) {
  const { t } = useTranslation(['common', 'devices'])
  const updateDeviceStatus = useStore((state) => state.updateDeviceStatus)
  const touchDeviceActivity = useStore((state) => state.touchDeviceActivity)
  const isMobile = useIsMobile()
  const { deviceCounts, refresh: refreshTransformCounts } = useTransformCounts()

  // Listen to device status change events + telemetry activity.
  // DeviceMetric events update last_seen (throttled) so the list reflects
  // active data flow in real time without a manual refresh.
  useDeviceEvents({
    enabled: true,
    eventTypes: ['DeviceOnline', 'DeviceOffline', 'DeviceMetric'],
    onEvent: (event) => {
      if (event.type === 'DeviceOnline' || event.type === 'DeviceOffline') {
        const data = event.data as { device_id: string }
        if (data.device_id) {
          updateDeviceStatus(data.device_id, event.type === 'DeviceOnline' ? 'online' : 'offline')
        }
      } else if (event.type === 'DeviceMetric') {
        const data = event.data as { device_id: string }
        if (data.device_id) {
          touchDeviceActivity(data.device_id)
        }
      }
    },
  })

  // Get adapter icon
  const getAdapterIcon = (adapter: string) => {
    const lower = adapter?.toLowerCase() || ''
    if (lower.includes('mqtt') || lower === 'mqtt') return Database
    if (lower.includes('http')) return Cpu
    return Waves
  }

  return (
    <>
      {/* Dialogs (由上层 TAB 操作按钮控制 open 状态) */}
      {addDeviceDialog}

      {isMobile ? (
        <div className="space-y-3">
          {paginatedDevices.map((device) => {
            const AdapterIcon = getAdapterIcon(device.adapter_type)
            return (
              <Card
                key={device.id}
                className={cn(
                  "overflow-hidden border-border shadow-sm cursor-pointer active:scale-[0.99]",
                  interactiveCardHover,
                )}
                onClick={() => onViewDetails(device)}
              >
                <div className="px-3 py-2.5">
                  {/* Row 1: icon + name + status + actions */}
                  <div className="flex items-center gap-2.5">
                    <div className={cn(
                      "w-8 h-8 rounded-lg flex items-center justify-center shrink-0",
                      device.status === 'online'
                        ? "bg-success-light text-success"
                        : "bg-muted text-muted-foreground"
                    )}>
                      <Cpu className="h-4 w-4" />
                    </div>
                    <div className="flex-1 min-w-0">
                      <div className="font-medium text-sm truncate">{device.name || "-"}</div>
                    </div>
                    <DeviceStatusBadge device={device} />
                    <DropdownMenu>
                      <DropdownMenuTrigger asChild onClick={(e) => e.stopPropagation()}>
                        <IconButton>
                          <MoreVertical className="h-4 w-4" />
                        </IconButton>
                      </DropdownMenuTrigger>
                      <DropdownMenuContent align="end">
                        <DropdownMenuItem onClick={(e) => { e.stopPropagation(); onViewDetails(device) }}>
                          <Eye className="h-4 w-4 mr-2" />
                          {t('devices:actions.viewDetails')}
                        </DropdownMenuItem>
                        <DropdownMenuItem onClick={(e) => { e.stopPropagation(); onEdit(device) }}>
                          <Pencil className="h-4 w-4 mr-2" />
                          {t('common:edit')}
                        </DropdownMenuItem>
                        <DropdownMenuItem
                          className="text-error"
                          onClick={(e) => { e.stopPropagation(); onDelete(device.id) }}
                        >
                          <Trash2 className="h-4 w-4 mr-2" />
                          {t('common:delete')}
                        </DropdownMenuItem>
                      </DropdownMenuContent>
                    </DropdownMenu>
                  </div>
                  {/* Row 2: type badge + adapter + last seen */}
                  <div className="flex items-center gap-1.5 mt-1.5 ml-[42px]">
                    <Badge variant="outline" className={cn(textMini, "h-5 px-1.5")}>
                      {device.device_type}
                    </Badge>
                    <div className="flex items-center gap-1">
                      <AdapterIcon className="h-3 w-3 text-muted-foreground" />
                      <Badge variant="outline" className={cn(textMini, "h-5 px-1.5")}>
                        {device.adapter_type || 'mqtt'}
                      </Badge>
                    </div>
                    <span className={cn(textMini, "text-muted-foreground ml-auto")}>
                      {formatTimestamp(device.last_seen, false)}
                    </span>
                  </div>
                </div>
              </Card>
            )
          })}
        </div>
      ) : (
        <ResponsiveTable
        onRowClick={(rowData) => {
          onViewDetails(rowData)
        }}
        columns={[
          {
            key: 'name',
            label: t('devices:headers.name'),
            width: '28%',
          },
          {
            key: 'type',
            label: t('devices:headers.type'),
          },
          {
            key: 'adapter',
            label: t('devices:headers.adapter'),
          },
          {
            key: 'transforms',
            label: t('automation:transforms', { defaultValue: 'Transforms' }),
            align: 'center',
          },
          {
            key: 'status',
            label: t('devices:headers.status'),
            align: 'center',
          },
          {
            key: 'lastReport',
            label: t('devices:headers.lastReport'),
            align: 'center',
          },
        ]}
        data={paginatedDevices}
        rowKey={(device: Device) => device.id}
        loading={loading}
        renderCell={(columnKey, rowData) => {
          const device = rowData
          const AdapterIcon = getAdapterIcon(device.adapter_type)

          switch (columnKey) {
            case 'name':
              return (
                <div className="flex items-center gap-3">
                  <div className={cn(
                    "w-9 h-9 rounded-lg flex items-center justify-center transition-colors",
                    device.status === 'online'
                      ? "bg-success-light text-success"
                      : "bg-muted text-muted-foreground"
                  )}>
                    <Cpu className="h-4 w-4" />
                  </div>
                  <div>
                    <div className="font-medium text-sm">{device.name || "-"}</div>
                    <code className="text-xs text-muted-foreground font-mono">{device.id}</code>
                  </div>
                </div>
              )

            case 'type':
              return (
                <Badge variant="outline" className="text-xs">
                  {device.device_type}
                </Badge>
              )

            case 'adapter':
              return (
                <div className="flex items-center gap-2">
                  <AdapterIcon className="h-4 w-4 text-muted-foreground" />
                  <Badge variant="outline" className="text-xs">
                    {device.adapter_type || 'mqtt'}
                  </Badge>
                </div>
              )

            case 'transforms':
              return <TransformsBadge deviceId={device.id} count={deviceCounts[device.id] ?? 0} onRefresh={() => { refreshTransformCounts(); onRefresh() }} />

            case 'status':
              return <DeviceStatusBadge device={device} />

            case 'lastReport':
              return (
                <span className="text-xs text-muted-foreground">
                  {formatTimestamp(device.last_seen, false)}
                </span>
              )

            default:
              return null
          }
        }}
        actions={[
          {
            label: t('devices:actions.viewDetails'),
            icon: <Eye className="h-4 w-4" />,
            onClick: (rowData) => {
              const device = rowData
              onViewDetails(device)
            },
          },
          {
            label: t('common:edit'),
            icon: <Pencil className="h-4 w-4" />,
            onClick: (rowData) => {
              const device = rowData
              onEdit(device)
            },
          },
          {
            label: t('common:delete'),
            icon: <Trash2 className="h-4 w-4" />,
            variant: 'destructive',
            onClick: (rowData) => {
              const device = rowData
              onDelete(device.id)
            },
          },
        ]}
        emptyState={
          <EmptyState
            icon={<Cpu className="h-12 w-12" />}
            title={t('devices:empty.title', 'No devices')}
            description={t('devices:empty.description', 'Add your first device to get started')}
          />
        }
      />
      )}
    </>
  )
}
