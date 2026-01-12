import { useState } from "react"
import { useTranslation } from "react-i18next"
import { ScrollArea } from "@/components/ui/scroll-area"
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
import { LoadingState, EmptyState, Pagination, BulkActionBar } from "@/components/shared"
import { Eye, Pencil, Trash2, RefreshCw, Plus, FileJson } from "lucide-react"
import { Dialog, DialogTrigger } from "@/components/ui/dialog"
import { cn } from "@/lib/utils"
import type { DeviceType } from "@/types"
import { api } from "@/lib/api"
import { useToast } from "@/hooks/use-toast"

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
  onAddType: () => void
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
  onAddType,
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

  const allOnPageSelected = paginatedDeviceTypes.length > 0 && paginatedDeviceTypes.every((t) => selectedIds.has(t.device_type))

  return (
    <>
      {/* Toolbar */}
      <div className="flex items-center justify-between mb-4">
        <div className="flex gap-2">
          <Dialog>
            <DialogTrigger asChild>
              <Button size="sm" onClick={onAddType}>
                <Plus className="mr-2 h-4 w-4" />
                {t('devices:addDeviceType')}
              </Button>
            </DialogTrigger>
            {addTypeDialog}
          </Dialog>
          <Button variant="outline" size="sm" onClick={onRefresh}>
            <RefreshCw className="mr-2 h-4 w-4" />
            {t('common:refresh')}
          </Button>
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
        <LoadingState text={t('devices:types.loading')} />
      ) : deviceTypes.length === 0 ? (
        <EmptyState
          icon={<FileJson className="h-12 w-12 text-muted-foreground" />}
          title={t('devices:types.noTypes')}
          description={t('devices:types.startUsing')}
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
                  <TableHead className="text-center">{t('devices:types.headers.id')}</TableHead>
                  <TableHead className="text-center">{t('devices:types.headers.name')}</TableHead>
                  <TableHead className="text-center">{t('devices:types.headers.description')}</TableHead>
                  <TableHead className="text-center">{t('devices:types.headers.metrics')}</TableHead>
                  <TableHead className="text-center">{t('devices:types.headers.commands')}</TableHead>
                  <TableHead className="text-center">{t('devices:types.headers.actions')}</TableHead>
                </TableRow>
              </TableHeader>
              <TableBody>
                {paginatedDeviceTypes.map((type) => (
                  <TableRow key={type.device_type} className={cn(selectedIds.has(type.device_type) && "bg-muted/50")}>
                    <TableCell>
                      <Checkbox
                        checked={selectedIds.has(type.device_type)}
                        onCheckedChange={() => toggleSelection(type.device_type)}
                      />
                    </TableCell>
                    <TableCell className="font-mono text-xs text-center">
                      {type.device_type}
                    </TableCell>
                    <TableCell className="text-center">{type.name}</TableCell>
                    <TableCell className="text-sm text-muted-foreground text-center">
                      {type.description || "-"}
                    </TableCell>
                    <TableCell className="text-center">
                      {type.uplink?.metrics?.length ?? type.metric_count ?? 0}
                    </TableCell>
                    <TableCell className="text-center">
                      {type.downlink?.commands?.length ?? type.command_count ?? 0}
                    </TableCell>
                    <TableCell className="text-center">
                      <div className="flex justify-center gap-1">
                        <Button
                          variant="ghost"
                          size="icon"
                          onClick={() => onViewDetails(type)}
                        >
                          <Eye className="h-4 w-4" />
                        </Button>
                        <Button
                          variant="ghost"
                          size="icon"
                          onClick={() => onEdit(type)}
                        >
                          <Pencil className="h-4 w-4" />
                        </Button>
                        <Button
                          variant="ghost"
                          size="icon"
                          onClick={() => onDelete(type.device_type)}
                        >
                          <Trash2 className="h-4 w-4" />
                        </Button>
                      </div>
                    </TableCell>
                  </TableRow>
                ))}
              </TableBody>
            </Table>
          </div>

          {/* Pagination Controls */}
          {deviceTypes.length > deviceTypesPerPage && (
            <div className="px-4 pt-4">
              <Pagination
                total={deviceTypes.length}
                pageSize={deviceTypesPerPage}
                currentPage={deviceTypePage}
                onPageChange={onPageChange}
              />
            </div>
          )}
        </ScrollArea>
      )}
    </>
  )
}
