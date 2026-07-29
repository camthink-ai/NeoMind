import { useTranslation } from "react-i18next"
import { Badge } from "@/components/ui/badge"
import { Card } from "@/components/ui/card"
import { DropdownMenu, DropdownMenuTrigger, DropdownMenuContent, DropdownMenuItem } from "@/components/ui/dropdown-menu"
import { IconButton } from "@/components/ui/button"
import { ResponsiveTable, EmptyState } from "@/components/shared"
import { Eye, Pencil, Trash2, Download, MoreVertical, Cpu, Database, Activity } from "lucide-react"
import type { DeviceType } from "@/types"
import { api } from "@/lib/api"
import { cn } from "@/lib/utils"
import { useToast } from "@/hooks/use-toast"
import { TransformsBadge } from "@/components/automation"
import { useIsMobile } from "@/hooks/useMobile"
import { useTransformCounts } from "@/hooks/useTransformCounts"
import { textNano, textMini } from "@/design-system/tokens/typography"

interface DeviceTypeListProps {
  deviceTypes: DeviceType[]
  loading: boolean
  paginatedDeviceTypes: DeviceType[]
  deviceTypePage: number
  deviceTypesPerPage: number
  onRefresh: () => void
  onViewDetails: (type: DeviceType) => void
  onEdit: (type: DeviceType) => void
  onDelete: (id: string) => void
  onPageChange: (page: number) => void
  addTypeDialog: React.ReactNode
}

export function DeviceTypeList({
  deviceTypes,
  loading,
  paginatedDeviceTypes,
  deviceTypePage,
  deviceTypesPerPage,
  onRefresh,
  onViewDetails,
  onEdit,
  onDelete,
  onPageChange,
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
          {paginatedDeviceTypes.map((dt) => (
            <Card
              key={dt.device_type}
              className="overflow-hidden border-border shadow-sm cursor-pointer active:scale-[0.99] transition-all"
              onClick={() => onViewDetails(dt)}
            >
              <div className="px-3 py-2.5">
                {/* Row 1: icon + name + stats + actions */}
                <div className="flex items-center gap-2">
                  <div className="w-8 h-8 rounded-lg flex items-center justify-center shrink-0 bg-info-light text-info">
                    <Database className="h-4 w-4" />
                  </div>
                  <div className="flex-1 min-w-0">
                    <div className="font-medium text-sm truncate">{dt.name}</div>
                  </div>
                  {/* Compact stats: icon + count */}
                  <span className={cn("flex items-center gap-0.5", textMini, "text-info shrink-0")}>
                    <Activity className="h-3 w-3" />
                    {dt.metrics?.length ?? dt.metric_count ?? 0}
                  </span>
                  <span className={cn("flex items-center gap-0.5", textMini, "text-accent-purple shrink-0")}>
                    <Cpu className="h-3 w-3" />
                    {dt.commands?.length ?? dt.command_count ?? 0}
                  </span>
                  <DropdownMenu>
                    <DropdownMenuTrigger asChild onClick={(e) => e.stopPropagation()}>
                      <IconButton className="shrink-0">
                        <MoreVertical className="h-4 w-4" />
                      </IconButton>
                    </DropdownMenuTrigger>
                    <DropdownMenuContent align="end">
                      <DropdownMenuItem onClick={(e) => { e.stopPropagation(); onViewDetails(dt) }}>
                        <Eye className="h-4 w-4 mr-2" />
                        {t('devices:types.actions.view')}
                      </DropdownMenuItem>
                      <DropdownMenuItem onClick={(e) => { e.stopPropagation(); handleExport(dt) }}>
                        <Download className="h-4 w-4 mr-2" />
                        {t('devices:types.actions.export')}
                      </DropdownMenuItem>
                      <DropdownMenuItem onClick={(e) => { e.stopPropagation(); onEdit(dt) }}>
                        <Pencil className="h-4 w-4 mr-2" />
                        {t('common:edit')}
                      </DropdownMenuItem>
                      <DropdownMenuItem
                        className="text-error"
                        onClick={(e) => { e.stopPropagation(); onDelete(dt.device_type) }}
                      >
                        <Trash2 className="h-4 w-4 mr-2" />
                        {t('common:delete')}
                      </DropdownMenuItem>
                    </DropdownMenuContent>
                  </DropdownMenu>
                </div>
                {/* Row 2: device_type code + description */}
                <div className="flex items-center gap-1.5 mt-1 ml-[40px]">
                  <code className={cn(textNano, "text-muted-foreground font-mono shrink-0")}>{dt.device_type}</code>
                  {dt.description && (
                    <>
                      <span className={cn(textNano, "text-muted-foreground")}>·</span>
                      <span className={cn(textNano, "text-muted-foreground truncate min-w-0")}>
                        {dt.description}
                      </span>
                    </>
                  )}
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
            key: 'transforms',
            label: t('automation:transforms', { defaultValue: 'Transforms' }),
            align: 'center',
            width: 'w-24',
          },
        ]}
        data={paginatedDeviceTypes as unknown as Record<string, unknown>[]}
        rowKey={(type) => (type as unknown as DeviceType).device_type}
        onRowClick={(row) => onViewDetails(row as unknown as DeviceType)}
        loading={loading}
        renderCell={(columnKey, rowData) => {
          const type = rowData as unknown as DeviceType

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
              const type = rowData as unknown as DeviceType
              onViewDetails(type)
            },
          },
          {
            label: t('devices:types.actions.export'),
            icon: <Download className="h-4 w-4" />,
            onClick: (rowData) => {
              const type = rowData as unknown as DeviceType
              handleExport(type)
            },
          },
          {
            label: t('common:edit'),
            icon: <Pencil className="h-4 w-4" />,
            onClick: (rowData) => {
              const type = rowData as unknown as DeviceType
              onEdit(type)
            },
          },
          {
            label: t('common:delete'),
            icon: <Trash2 className="h-4 w-4" />,
            variant: 'destructive',
            onClick: (rowData) => {
              const type = rowData as unknown as DeviceType
              onDelete(type.device_type)
            },
          },
        ]}
        emptyState={
          <EmptyState
            icon={<Database className="h-12 w-12" />}
            title={t('devices:emptyType.title', 'No device types')}
            description={t('devices:emptyType.description', 'Create a device type to define device categories')}
          />
        }
      />
      )}
    </>
  )
}
