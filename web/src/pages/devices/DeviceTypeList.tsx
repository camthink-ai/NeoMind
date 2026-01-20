import { useState } from "react"
import { useTranslation } from "react-i18next"
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
import { EmptyStateInline, Pagination, BulkActionBar } from "@/components/shared"
import { Card } from "@/components/ui/card"
import { Eye, Pencil, Trash2, Download } from "lucide-react"
import { cn } from "@/lib/utils"
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
    const pageIds = new Set(paginatedDeviceTypes.map((t) => t.device_type))
    if (paginatedDeviceTypes.every((t) => selectedIds.has(t.device_type))) {
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
    if (!confirm(t('devices:types.confirmDeleteSelected', { count: selectedIds.size }))) return

    setBulkProcessing(true)
    try {
      const response = await api.bulkDeleteDeviceTypes(Array.from(selectedIds))
      toast({ title: t('common:success'), description: t('devices:types.deletedCount', { count: response.deleted }) })
      setSelectedIds(new Set())
      onRefresh()
    } catch (error) {
      toast({ title: t('common:failed'), description: t('devices:types.bulkDeleteFailed'), variant: "destructive" })
    } finally {
      setBulkProcessing(false)
    }
  }

  // Export single device type as JSON file
  // Note: Need to fetch full details first since list API only returns counts
  const handleExportSingle = async (deviceType: DeviceType) => {
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

  // Export selected device types
  const handleExportSelected = async () => {
    if (selectedIds.size === 0) {
      toast({ title: t('common:failed'), description: 'No device types selected', variant: 'destructive' })
      return
    }
    try {
      // Fetch full details for each selected device type
      const selectedTypes = deviceTypes.filter(t => selectedIds.has(t.device_type))
      const fullTypes = await Promise.all(
        selectedTypes.map(t => api.getDeviceType(t.device_type))
      )
      const data = JSON.stringify(fullTypes, null, 2)
      const blob = new Blob([data], { type: 'application/json' })
      const url = URL.createObjectURL(blob)
      const link = document.createElement('a')
      link.href = url
      link.download = `device-types-${selectedIds.size}.json`
      document.body.appendChild(link)
      link.click()
      document.body.removeChild(link)
      URL.revokeObjectURL(url)
      toast({ title: t('common:success'), description: `Exported ${selectedIds.size} device types` })
    } catch (error) {
      toast({ title: t('common:failed'), description: 'Failed to export device types', variant: 'destructive' })
    }
  }

  const allOnPageSelected = paginatedDeviceTypes.length > 0 && paginatedDeviceTypes.every((t) => selectedIds.has(t.device_type))

  return (
    <>
      {/* Dialogs - addTypeDialog is controlled by parent PageTabs actions */}
      {addTypeDialog}

      {/* Bulk Actions Bar */}
      <BulkActionBar
        selectedCount={selectedIds.size}
        actions={[
          {
            label: 'Export Selected',
            icon: <Download className="h-4 w-4" />,
            onClick: handleExportSelected,
            disabled: bulkProcessing,
            variant: "outline",
          },
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

      <Card>
        <Table>
          <TableHeader>
            <TableRow>
              <TableHead align="center" className="w-[50px]">
                <Checkbox checked={allOnPageSelected} onCheckedChange={toggleAll} />
              </TableHead>
              <TableHead>{t('devices:types.headers.id')}</TableHead>
              <TableHead>{t('devices:types.headers.name')}</TableHead>
              <TableHead>{t('devices:types.headers.description')}</TableHead>
              <TableHead align="center">{t('devices:types.headers.metrics')}</TableHead>
              <TableHead align="center">{t('devices:types.headers.commands')}</TableHead>
              <TableHead align="center">{t('automation:transforms', { defaultValue: 'Transforms' })}</TableHead>
              <TableHead align="right">{t('devices:types.headers.actions')}</TableHead>
            </TableRow>
          </TableHeader>
          <TableBody>
            {loading ? (
              <EmptyStateInline title={t('common:loading')} colSpan={8} />
            ) : deviceTypes.length === 0 ? (
              <EmptyStateInline title={t('devices:types.noTypes')} colSpan={8} />
            ) : (
                paginatedDeviceTypes.map((type) => (
                  <TableRow key={type.device_type} className={cn(selectedIds.has(type.device_type) && "bg-muted/50")}>
                    <TableCell align="center">
                      <Checkbox
                        checked={selectedIds.has(type.device_type)}
                        onCheckedChange={() => toggleSelection(type.device_type)}
                      />
                    </TableCell>
                    <TableCell className="font-mono text-xs">
                      {type.device_type}
                    </TableCell>
                    <TableCell>{type.name}</TableCell>
                    <TableCell className="text-sm text-muted-foreground">
                      {type.description || "-"}
                    </TableCell>
                    <TableCell align="center">
                      {type.metrics?.length ?? type.metric_count ?? 0}
                    </TableCell>
                    <TableCell align="center">
                      {type.commands?.length ?? type.command_count ?? 0}
                    </TableCell>
                    <TableCell align="center">
                      <TransformsBadge deviceTypeId={type.device_type} onRefresh={onRefresh} />
                    </TableCell>
                    <TableCell align="right">
                      <div className="flex justify-end gap-1">
                        <Button variant="ghost" size="icon" className="h-8 w-8" onClick={() => onViewDetails(type)}>
                          <Eye className="h-4 w-4" />
                        </Button>
                        <Button variant="ghost" size="icon" className="h-8 w-8" onClick={() => handleExportSingle(type)} title="Export">
                          <Download className="h-4 w-4" />
                        </Button>
                        <Button variant="ghost" size="icon" className="h-8 w-8" onClick={() => onEdit(type)}>
                          <Pencil className="h-4 w-4" />
                        </Button>
                        <Button variant="ghost" size="icon" className="h-8 w-8" onClick={() => onDelete(type.device_type)}>
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

      {deviceTypes.length > deviceTypesPerPage && (
        <div className="pt-4">
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
