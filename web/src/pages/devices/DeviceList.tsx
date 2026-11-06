import { useTranslation } from "react-i18next"
import { Badge } from "@/components/ui/badge"
import { DropdownMenu, DropdownMenuTrigger, DropdownMenuContent, DropdownMenuItem } from "@/components/ui/dropdown-menu"
import { IconButton } from "@/components/ui/button"
import { ResponsiveTable, EmptyState, EmptyStateCard } from "@/components/shared"
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
  /** True while a text search is active — switches the mobile empty state
      from the "add your first device" pitch to a no-results message. */
  searching?: boolean
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
  devices: _devices,
  loading,
  paginatedDevices,
  searching = false,
  devicePage: _devicePage,
  devicesPerPage: _devicesPerPage,
  onRefresh,
  onViewDetails,
  onEdit,
  onDelete,
  onPageChange: _onPageChange,
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
        <div className="space-y-1">
          {/* Mobile has no ResponsiveTable, so it must carry its own loading
              and empty states — a bare map() renders as a blank void. */}
          {loading && paginatedDevices.length === 0 && (
            [0, 1, 2].map((i) => (
              <div key={i} className="animate-pulse py-4">
                <div className="h-4 w-36 rounded bg-muted" />
                <div className="mt-2 h-3 w-52 rounded bg-muted" />
              </div>
            ))
          )}
          {!loading && paginatedDevices.length === 0 && (
            <EmptyStateCard>
              {searching ? (
                <EmptyState
                  title={t('devices:searchNoResults', 'No devices match your search')}
                  description={t('devices:searchNoResultsHint', 'Try a different name, type or adapter')}
                />
              ) : (
                <EmptyState
                  title={t('devices:empty.title', 'No devices')}
                  description={t('devices:empty.description', 'Add your first device to get started')}
                />
              )}
            </EmptyStateCard>
          )}
          {paginatedDevices.map((device) => {
            return (
              <div
                key={device.id}
                role="button"
                tabIndex={0}
                className="cursor-pointer rounded-xl px-2 py-3.5 transition-[background-color,transform] active:scale-[0.99] active:bg-muted"
                onClick={() => onViewDetails(device)}
                onKeyDown={(e) => { if (e.key === 'Enter' || e.key === ' ') onViewDetails(device) }}
              >
                <div className="flex items-start gap-3">
                  <div className="min-w-0 flex-1">
                    <div className="flex items-center gap-2">
                      <div className="truncate text-[15px] font-medium text-foreground">{device.name || "-"}</div>
                      <span className={cn("h-1.5 w-1.5 shrink-0 rounded-full", device.online ? "bg-success" : "bg-muted-foreground")} />
                    </div>
                    <div className="mt-1 truncate text-xs text-muted-foreground">
                      {device.device_type} · {device.adapter_type || 'mqtt'}
                    </div>
                    <div className="mt-1 text-xs text-muted-foreground">
                      {t('devices:headers.lastReport')} {formatTimestamp(device.last_seen, false)}
                    </div>
                  </div>
                  <div className="flex shrink-0 items-center gap-1">
                    <span className="pt-2 text-xs text-muted-foreground">
                      {device.online ? t('common:online', '在线') : t('common:offline', '离线')}
                    </span>
                    <DropdownMenu>
                      <DropdownMenuTrigger asChild onClick={(e) => e.stopPropagation()}>
                        <IconButton className="h-11 w-11">
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
                </div>
              </div>
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
              // Plain muted mono text — non-interactive metadata shouldn't
              // wear a bordered chip (reads as a button and litters the table).
              return (
                <span className="text-xs text-muted-foreground font-mono">
                  {device.device_type}
                </span>
              )

            case 'adapter':
              return (
                <div className="flex items-center gap-2">
                  <AdapterIcon className="h-4 w-4 text-muted-foreground" />
                  <span className="text-xs text-muted-foreground">
                    {device.adapter_type || 'mqtt'}
                  </span>
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
            title={t('devices:empty.title', 'No devices')}
            description={t('devices:empty.description', 'Add your first device to get started')}
          />
        }
      />
      )}
    </>
  )
}
