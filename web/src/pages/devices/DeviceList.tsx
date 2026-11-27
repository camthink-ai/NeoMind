import { useTranslation } from "react-i18next"
import { ResponsiveTable, EmptyState, EmptyStateCard } from "@/components/shared"
import { DeviceStatusBadge } from "@/components/shared/DeviceStatusBadge"
import { IconButton } from "@/components/ui/button"
import { Skeleton } from "@/components/ui/skeleton"
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu"
import { Eye, Trash2, Box, Cpu, Database, Waves, Pencil, MoreVertical } from "lucide-react"
import { cn } from "@/lib/utils"
import { formatTimestamp } from "@/lib/utils/format"
import { interactiveCardLift } from "@/design-system/tokens/size"
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
  /** Desktop display mode — 'grid' (cards) or 'list' (table). Defaults to 'grid'. */
  viewMode?: 'grid' | 'list'
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
  viewMode = 'grid',
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
        <div className="space-y-2">
          {/* Mobile has no ResponsiveTable, so it must carry its own loading
              and empty states — a bare map() renders as a blank void. */}
          {loading && paginatedDevices.length === 0 && (
            [0, 1, 2].map((i) => (
              <div key={i} className="rounded-2xl border border-border bg-card px-4 py-3.5 space-y-2">
                <Skeleton className="h-4 w-36" />
                <Skeleton className="h-3 w-52" />
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
                className="cursor-pointer rounded-2xl border border-border bg-card px-4 py-3.5 transition-colors active:bg-muted"
                onClick={() => onViewDetails(device)}
                onKeyDown={(e) => { if (e.key === 'Enter' || e.key === ' ') onViewDetails(device) }}
              >
                <div className="flex items-start gap-3">
                  <div className="min-w-0 flex-1">
                    <div className="flex items-center gap-2">
                      <div className="truncate text-heading font-medium text-foreground">{device.name || "-"}</div>
                      <span className={cn("h-1.5 w-1.5 shrink-0 rounded-full", device.online ? "bg-success" : "bg-muted-foreground")} />
                      <span className="sr-only">
                        {device.online ? t('common:online', '在线') : t('common:offline', '离线')}
                      </span>
                    </div>
                    <div className="mt-1 truncate text-xs text-muted-foreground">
                      {device.device_type} · {device.adapter_type || 'mqtt'}
                    </div>
                  </div>
                </div>
              </div>
            )
          })}
        </div>
      ) : viewMode === 'grid' ? (
        loading && paginatedDevices.length === 0 ? (
          <div className="grid grid-cols-[repeat(auto-fill,minmax(max(25%_-_1rem,280px),1fr))] gap-4">
            {Array.from({ length: 6 }).map((_, i) => (
              <div key={i} className="rounded-xl border border-border bg-card p-4 space-y-3">
                <div className="flex items-start gap-3">
                  <Skeleton className="h-9 w-9 rounded-lg shrink-0" />
                  <div className="flex-1 space-y-1.5">
                    <Skeleton className="h-4 w-2/3" />
                    <Skeleton className="h-3 w-1/2" />
                  </div>
                </div>
                <div className="flex items-center gap-2">
                  <Skeleton className="h-5 w-20 rounded-md" />
                  <Skeleton className="h-5 w-14 rounded-md" />
                </div>
                <div className="flex items-center justify-between pt-2.5 border-t border-glass-border">
                  <Skeleton className="h-4 w-16" />
                  <Skeleton className="h-3 w-24" />
                </div>
              </div>
            ))}
          </div>
        ) : paginatedDevices.length === 0 ? (
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
        ) : (
          <div className="grid grid-cols-[repeat(auto-fill,minmax(max(25%_-_1rem,280px),1fr))] gap-4">
            {paginatedDevices.map((device, index) => {
              const AdapterIcon = getAdapterIcon(device.adapter_type)
              const transformCount = deviceCounts[device.id] ?? 0
              return (
                <div
                  key={device.id}
                  role="button"
                  tabIndex={0}
                  onClick={() => onViewDetails(device)}
                  onKeyDown={(e) => { if (e.key === 'Enter' || e.key === ' ') { e.preventDefault(); onViewDetails(device) } }}
                  style={{ animationDelay: `${Math.min(index, 12) * 30}ms`, animationFillMode: 'both' }}
                  className={cn(
                    "group/card animate-fade-in-up motion-reduce:animate-none flex flex-col justify-between rounded-xl border border-border bg-card p-4 cursor-pointer active:scale-[0.99]",
                    interactiveCardLift,
                    "focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2"
                  )}
                >
                  <div>
                    {/* Header: icon + name/id + actions */}
                    <div className="flex items-start gap-3">
                      <div className="flex h-9 w-9 shrink-0 items-center justify-center rounded-lg border border-border bg-muted-30 text-foreground">
                        <Box className="h-4 w-4" strokeWidth={1.75} />
                      </div>
                      <div className="min-w-0 flex-1">
                        <div className="truncate text-sm font-semibold text-foreground leading-snug" title={device.name || undefined}>
                          {device.name || "-"}
                        </div>
                        <code className="mt-0.5 block truncate text-xs font-mono text-muted-foreground" title={device.id}>
                          {device.id}
                        </code>
                      </div>
                      <div onClick={(e) => e.stopPropagation()}>
                        <DropdownMenu>
                          <DropdownMenuTrigger asChild>
                            <IconButton
                              size="sm"
                              aria-label={t('common:actions', { defaultValue: 'Actions' })}
                              className="opacity-45 transition-opacity group-hover/card:opacity-100 focus-visible:opacity-100 data-[state=open]:opacity-100 -mr-1 -mt-1"
                            >
                              <MoreVertical className="h-4 w-4" />
                            </IconButton>
                          </DropdownMenuTrigger>
                          <DropdownMenuContent align="end" className="w-[160px]">
                            <DropdownMenuItem onClick={() => onViewDetails(device)} className="gap-2">
                              <Eye className="h-4 w-4 shrink-0" />
                              <span>{t('devices:actions.viewDetails')}</span>
                            </DropdownMenuItem>
                            <DropdownMenuItem onClick={() => onEdit(device)} className="gap-2">
                              <Pencil className="h-4 w-4 shrink-0" />
                              <span>{t('common:edit')}</span>
                            </DropdownMenuItem>
                            <DropdownMenuItem onClick={() => onDelete(device.id)} className="gap-2 text-error focus:text-error">
                              <Trash2 className="h-4 w-4 shrink-0" />
                              <span>{t('common:delete')}</span>
                            </DropdownMenuItem>
                          </DropdownMenuContent>
                        </DropdownMenu>
                      </div>
                    </div>

                    {/* Metadata row: device type + adapter + transforms */}
                    <div className="mt-3.5 flex flex-wrap items-center gap-1.5">
                      <span className="inline-flex items-center rounded-md border border-glass-border bg-muted-20 px-2 py-0.5 text-xs font-mono text-muted-foreground truncate max-w-[160px]" title={device.device_type}>
                        {device.device_type}
                      </span>
                      <span className="inline-flex items-center gap-1 rounded-md border border-glass-border bg-muted-20 px-2 py-0.5 text-xs text-muted-foreground">
                        <AdapterIcon className="h-3 w-3 shrink-0" />
                        <span>{device.adapter_type || 'mqtt'}</span>
                      </span>
                      {transformCount > 0 && (
                        <div onClick={(e) => e.stopPropagation()}>
                          <TransformsBadge
                            deviceId={device.id}
                            count={transformCount}
                            onRefresh={() => { refreshTransformCounts(); onRefresh() }}
                          />
                        </div>
                      )}
                    </div>
                  </div>

                  {/* Footer: status + last seen */}
                  <div className="mt-3.5 flex items-center justify-between gap-2 border-t border-glass-border pt-2.5">
                    <DeviceStatusBadge device={device} />
                    <span className="text-xs text-muted-foreground tabular-nums truncate">
                      {formatTimestamp(device.last_seen, false)}
                    </span>
                  </div>
                </div>
              )
            })}
          </div>
        )
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
                <div className="flex min-w-0 items-center gap-3">
                  <div className="relative flex h-9 w-9 shrink-0 items-center justify-center rounded-lg border border-border bg-muted-30 text-foreground">
                    <Box className="h-4 w-4" strokeWidth={1.75} />
                    <span
                      className={cn(
                        "absolute -bottom-0.5 -right-0.5 h-2.5 w-2.5 rounded-full ring-2 ring-card",
                        device.status === 'online' ? "bg-success" : "bg-muted-foreground"
                      )}
                    />
                  </div>
                  <div className="min-w-0">
                    <div className="truncate text-sm font-medium text-foreground" title={device.name || undefined}>{device.name || "-"}</div>
                    <code className="block min-w-0 truncate text-xs font-mono text-muted-foreground" title={device.id}>{device.id}</code>
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
