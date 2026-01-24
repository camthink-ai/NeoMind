import { useTranslation } from "react-i18next"
import { Button } from "@/components/ui/button"
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table"
import { EmptyStateInline, Pagination } from "@/components/shared"
import { Card } from "@/components/ui/card"
import { Badge } from "@/components/ui/badge"
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu"
import { Eye, Pencil, Trash2, Download, MoreVertical, Cpu, Database, Activity } from "lucide-react"
import type { DeviceType } from "@/types"
import { api } from "@/lib/api"
import { useToast } from "@/hooks/use-toast"
import { TransformsBadge } from "@/components/automation"

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

      <Card className="overflow-hidden">
        <Table>
          <TableHeader>
            <TableRow className="hover:bg-transparent border-b bg-muted/30">
              <TableHead className="w-10 text-center">#</TableHead>
              <TableHead>
                <div className="flex items-center gap-2 text-xs font-semibold uppercase tracking-wide text-muted-foreground">
                  <Database className="h-4 w-4" />
                  {t('devices:types.headers.name')}
                </div>
              </TableHead>
              <TableHead>
                <div className="flex items-center gap-2 text-xs font-semibold uppercase tracking-wide text-muted-foreground">
                  <Activity className="h-4 w-4" />
                  {t('devices:types.headers.metrics')}
                </div>
              </TableHead>
              <TableHead>
                <div className="flex items-center gap-2 text-xs font-semibold uppercase tracking-wide text-muted-foreground">
                  <Cpu className="h-4 w-4" />
                  {t('devices:types.headers.commands')}
                </div>
              </TableHead>
              <TableHead align="center">
                <div className="flex items-center justify-center gap-2 text-xs font-semibold uppercase tracking-wide text-muted-foreground">
                  {t('automation:transforms', { defaultValue: 'Transforms' })}
                </div>
              </TableHead>
              <TableHead className="w-12"></TableHead>
            </TableRow>
          </TableHeader>
          <TableBody>
            {loading ? (
              <EmptyStateInline title={t('common:loading')} colSpan={6} />
            ) : deviceTypes.length === 0 ? (
              <EmptyStateInline title={t('devices:types.noTypes')} colSpan={6} />
            ) : (
              paginatedDeviceTypes.map((type, index) => (
                <TableRow key={type.device_type} className="group transition-colors hover:bg-muted/50">
                  <TableCell className="text-center">
                    <span className="text-xs text-muted-foreground font-medium">{index + 1}</span>
                  </TableCell>
                  <TableCell>
                    <div className="flex items-center gap-3">
                      <div className="w-9 h-9 rounded-lg flex items-center justify-center bg-blue-500/10 text-blue-600">
                        <Database className="h-4 w-4" />
                      </div>
                      <div>
                        <div className="font-medium text-sm">{type.name}</div>
                        <div className="flex items-center gap-2">
                          <code className="text-xs text-muted-foreground font-mono">{type.device_type}</code>
                          {type.description && (
                            <span className="text-xs text-muted-foreground line-clamp-1 max-w-[200px]">
                              {type.description}
                            </span>
                          )}
                        </div>
                      </div>
                    </div>
                  </TableCell>
                  <TableCell>
                    <Badge variant="outline" className="text-xs bg-blue-50 text-blue-700 border-blue-200 dark:bg-blue-950/30 dark:text-blue-400 dark:border-blue-800">
                      {type.metrics?.length ?? type.metric_count ?? 0}
                    </Badge>
                  </TableCell>
                  <TableCell>
                    <Badge variant="outline" className="text-xs bg-purple-50 text-purple-700 border-purple-200 dark:bg-purple-950/30 dark:text-purple-400 dark:border-purple-800">
                      {type.commands?.length ?? type.command_count ?? 0}
                    </Badge>
                  </TableCell>
                  <TableCell align="center">
                    <TransformsBadge deviceTypeId={type.device_type} onRefresh={onRefresh} />
                  </TableCell>
                  <TableCell>
                    <DropdownMenu>
                      <DropdownMenuTrigger asChild>
                        <Button variant="ghost" size="icon" className="h-8 w-8 opacity-0 group-hover:opacity-100 transition-opacity">
                          <MoreVertical className="h-4 w-4" />
                        </Button>
                      </DropdownMenuTrigger>
                      <DropdownMenuContent align="end" className="w-40">
                        <DropdownMenuItem onClick={() => onViewDetails(type)}>
                          <Eye className="mr-2 h-4 w-4" />
                          {t('devices:types.actions.view')}
                        </DropdownMenuItem>
                        <DropdownMenuItem onClick={() => handleExport(type)}>
                          <Download className="mr-2 h-4 w-4" />
                          {t('devices:types.actions.export')}
                        </DropdownMenuItem>
                        <DropdownMenuItem onClick={() => onEdit(type)}>
                          <Pencil className="mr-2 h-4 w-4" />
                          {t('common:edit')}
                        </DropdownMenuItem>
                        <DropdownMenuSeparator />
                        <DropdownMenuItem
                          onClick={() => onDelete(type.device_type)}
                          className="text-destructive"
                        >
                          <Trash2 className="mr-2 h-4 w-4" />
                          {t('common:delete')}
                        </DropdownMenuItem>
                      </DropdownMenuContent>
                    </DropdownMenu>
                  </TableCell>
                </TableRow>
              ))
            )}
          </TableBody>
        </Table>
      </Card>

      {deviceTypes.length > deviceTypesPerPage && (
        <div className="sticky bottom-0 bg-background/95 backdrop-blur supports-[backdrop-filter]:bg-background/60 pt-4 pb-2 border-t mt-4">
          <Pagination
            total={deviceTypes.length}
            pageSize={deviceTypesPerPage}
            currentPage={deviceTypePage}
            onPageChange={onPageChange}
          />
        </div>
      )}
    </>
  )
}
