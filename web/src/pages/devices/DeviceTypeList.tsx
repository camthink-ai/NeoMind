import { useTranslation } from "react-i18next"
import { Badge } from "@/components/ui/badge"
import { Card } from "@/components/ui/card"
import { DropdownMenu, DropdownMenuTrigger, DropdownMenuContent, DropdownMenuItem } from "@/components/ui/dropdown-menu"
import { IconButton } from "@/components/ui/button"
import { ResponsiveTable, EmptyState, EmptyStateCard } from "@/components/shared"
import { ChevronRight, Eye, Pencil, Trash2, Download, MoreVertical, Database } from "lucide-react"
import type { DeviceType } from "@/types"
import { api } from "@/lib/api"
import { cn } from "@/lib/utils"
import { useToast } from "@/hooks/use-toast"
import { TransformsBadge } from "@/components/automation"
import { useIsMobile } from "@/hooks/useMobile"
import { useTransformCounts } from "@/hooks/useTransformCounts"
import { textNano } from "@/design-system/tokens/typography"

interface DeviceTypeListProps {
  deviceTypes: DeviceType[]
  loading: boolean
  paginatedDeviceTypes: DeviceType[]
  deviceTypePage: number
  deviceTypesPerPage: number
  /** Attached device count per device_type id — for the devices column and delete guard */
  deviceCounts: Record<string, number>
  onRefresh: () => void
  onViewDetails: (type: DeviceType) => void
  onEdit: (type: DeviceType) => void
  onDelete: (id: string) => void
  onPageChange: (page: number) => void
  addTypeDialog: React.ReactNode
}

export function DeviceTypeList({
  deviceTypes: _deviceTypes,
  loading,
  paginatedDeviceTypes,
  deviceTypePage: _deviceTypePage,
  deviceTypesPerPage: _deviceTypesPerPage,
  deviceCounts,
  onRefresh,
  onViewDetails,
  onEdit,
  onDelete,
  onPageChange: _onPageChange,
  addTypeDialog,
}: DeviceTypeListProps) {
  const { t } = useTranslation(['common', 'devices'])
  const { toast } = useToast()
  const isMobile = useIsMobile()
  const { deviceTypeCounts, refresh: refreshTransformCounts } = useTransformCounts()

  // Export single device type as JSON file
  const handleExport = async (deviceType: DeviceType) => {
    try {
      // Fetch full device type details with metrics and commands
      const fullType = await api.getDeviceType(deviceType.device_type)
      const data = JSON.stringify(fullType, null, 2)
      const blob = new Blob([data], { type: 'application/json' })
      const url = URL.createObjectURL(blob)
      const link = document.createElement('a')
      link.href = url
      link.download = `device-type-${deviceType.device_type}.json`
      document.body.appendChild(link)
      link.click()
      document.body.removeChild(link)
      URL.revokeObjectURL(url)
      toast({ title: t('common:success'), description: `Exported ${deviceType.name}` })
    } catch (error) {
      toast({ title: t('common:failed'), description: 'Failed to export device type', variant: 'destructive' })
    }
  }

  return (
    <>
      {/* Dialogs - addTypeDialog is controlled by parent PageTabs actions */}
      {addTypeDialog}

      {isMobile ? (
        <div className="space-y-2">
          {/* Mobile has no ResponsiveTable — carry its own empty state. */}
          {!loading && paginatedDeviceTypes.length === 0 && (
            <EmptyStateCard>
              <EmptyState
                title={t('devices:emptyType.title', 'No device types')}
                description={t('devices:emptyType.description', 'Create a device type to define device categories')}
              />
            </EmptyStateCard>
          )}
          {loading && paginatedDeviceTypes.length === 0 && (
            [0, 1, 2].map((i) => (
              <Card key={i} className="overflow-hidden rounded-2xl border-border bg-card shadow-none">
                <div className="px-3 py-2.5 animate-pulse">
                  <div className="flex items-center gap-2">
                    <div className="w-8 h-8 rounded-lg bg-muted" />
                    <div className="h-4 w-32 rounded bg-muted" />
                  </div>
                  <div className="mt-2 ml-[40px] h-3 w-44 rounded bg-muted" />
                </div>
              </Card>
            ))
          )}
          {paginatedDeviceTypes.map((dt) => (
            <Card
              key={dt.device_type}
              className={cn(
                "cursor-pointer overflow-hidden rounded-2xl border border-border bg-card shadow-none transition-colors active:bg-muted",
              )}
              onClick={() => onViewDetails(dt)}
            >
              <div className="px-4 py-3.5">
                <div className="flex items-center gap-2">
                  <div className="flex-1 min-w-0">
                    <div className="truncate text-[15px] font-medium">{dt.name}</div>
                    <div className="mt-1 flex min-w-0 items-center gap-1.5 text-xs text-muted-foreground">
                      <code className="shrink-0 truncate font-mono">{dt.device_type}</code>
                      {dt.description && <span className="truncate">· {dt.description}</span>}
                    </div>
                  </div>
                  {dt.builtin_version && (
                    <Badge variant="outline" className={cn("shrink-0", textNano, "bg-muted-30 text-muted-foreground border-muted")}>
                      {t('devices:types.builtinBadge')}
                    </Badge>
                  )}
                  <ChevronRight className="h-4 w-4 shrink-0 text-muted-foreground" aria-hidden="true" />
                </div>
              </div>
            </Card>
          ))}
        </div>
      ) : (
      <ResponsiveTable
        columns={[
          {
            key: 'name',
            label: t('devices:types.headers.name'),
            width: '28%',
          },
          {
            key: 'description',
            label: t('common:description'),
            width: 'max-w-xs',
          },
          {
            key: 'metrics',
            label: t('devices:types.headers.metrics'),
            align: 'center',
            width: 'w-16',
          },
          {
            key: 'commands',
            label: t('devices:types.headers.commands'),
            align: 'center',
            width: 'w-16',
          },
          {
            key: 'devices',
            label: t('devices:types.headers.devices'),
            align: 'center',
            width: 'w-16',
          },
          {
            key: 'transforms',
            label: t('automation:transforms', { defaultValue: 'Transforms' }),
            align: 'center',
            width: 'w-24',
          },
        ]}
        data={paginatedDeviceTypes}
        rowKey={(type: DeviceType) => type.device_type}
        onRowClick={(row: DeviceType) => onViewDetails(row)}
        loading={loading}
        renderCell={(columnKey, rowData) => {
          const type = rowData

          switch (columnKey) {
            case 'name':
              return (
                <div className="flex items-center gap-3">
                  <div className="w-9 h-9 rounded-lg flex items-center justify-center bg-info-light text-info">
                    <Database className="h-4 w-4" />
                  </div>
                  <div className="min-w-0">
                    <div className="font-medium text-sm truncate">{type.name}</div>
                    <code className="text-xs text-muted-foreground font-mono">{type.device_type}</code>
                    {type.builtin_version && (
                      <div>
                        <Badge variant="outline" className={cn("mt-0.5", textNano, "bg-muted-30 text-muted-foreground border-muted")}>
                          {t('devices:types.builtinBadge')}
                        </Badge>
                      </div>
                    )}
                  </div>
                </div>
              )

            case 'description':
              return (
                <div className="text-sm text-muted-foreground line-clamp-2 leading-relaxed" title={type.description || undefined}>
                  {type.description || <span className="text-muted-foreground">-</span>}
                </div>
              )

            case 'metrics':
              return (
                <Badge variant="outline" className="text-xs bg-info-light text-info border-info">
                  {type.metrics?.length ?? type.metric_count ?? 0}
                </Badge>
              )

            case 'commands':
              return (
                <Badge variant="outline" className="text-xs bg-accent-purple-light text-accent-purple border-accent-purple-light">
                  {type.commands?.length ?? type.command_count ?? 0}
                </Badge>
              )

            case 'devices':
              return (
                <Badge variant="outline" className="text-xs bg-muted-30 text-muted-foreground border-muted">
                  {deviceCounts[type.device_type] ?? 0}
                </Badge>
              )

            case 'transforms':
              return <TransformsBadge deviceTypeId={type.device_type} count={deviceTypeCounts[type.device_type] ?? 0} onRefresh={() => { refreshTransformCounts(); onRefresh() }} />

            default:
              return null
          }
        }}
        actions={[
          {
            label: t('devices:types.actions.view'),
            icon: <Eye className="h-4 w-4" />,
            onClick: (rowData) => {
              const type = rowData
              onViewDetails(type)
            },
          },
          {
            label: t('devices:types.actions.export'),
            icon: <Download className="h-4 w-4" />,
            onClick: (rowData) => {
              const type = rowData
              handleExport(type)
            },
          },
          {
            label: t('common:edit'),
            icon: <Pencil className="h-4 w-4" />,
            onClick: (rowData) => {
              const type = rowData
              onEdit(type)
            },
          },
          {
            label: t('common:delete'),
            icon: <Trash2 className="h-4 w-4" />,
            variant: 'destructive',
            onClick: (rowData) => {
              const type = rowData
              onDelete(type.device_type)
            },
          },
        ]}
        emptyState={
          <EmptyState
            title={t('devices:emptyType.title', 'No device types')}
            description={t('devices:emptyType.description', 'Create a device type to define device categories')}
          />
        }
      />
      )}
    </>
  )
}
