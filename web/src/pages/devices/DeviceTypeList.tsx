import { useState, useRef } from "react"
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
import { LoadingState, EmptyStateInline, Pagination, BulkActionBar, ActionBar } from "@/components/shared"
import { Card } from "@/components/ui/card"
import { Eye, Pencil, Trash2, FileJson, Plus, Download, Upload } from "lucide-react"
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
  onImportDeviceType?: (definition: DeviceType) => Promise<void>
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
  onImportDeviceType,
}: DeviceTypeListProps) {
  const { t } = useTranslation(['common', 'devices'])
  const { toast } = useToast()
  const [selectedIds, setSelectedIds] = useState<Set<string>>(new Set())
  const [bulkProcessing, setBulkProcessing] = useState(false)
  const [importing, setImporting] = useState(false)
  const fileInputRef = useRef<HTMLInputElement>(null)

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
  const handleExportSingle = (deviceType: DeviceType) => {
    const data = JSON.stringify(deviceType, null, 2)
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
  }

  // Export selected device types
  const handleExportSelected = () => {
    if (selectedIds.size === 0) {
      toast({ title: t('common:failed'), description: 'No device types selected', variant: 'destructive' })
      return
    }
    const selectedTypes = deviceTypes.filter(t => selectedIds.has(t.device_type))
    const data = JSON.stringify(selectedTypes, null, 2)
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
  }

  // Export all device types
  const handleExportAll = () => {
    const data = JSON.stringify(deviceTypes, null, 2)
    const blob = new Blob([data], { type: 'application/json' })
    const url = URL.createObjectURL(blob)
    const link = document.createElement('a')
    link.href = url
    link.download = `all-device-types.json`
    document.body.appendChild(link)
    link.click()
    document.body.removeChild(link)
    URL.revokeObjectURL(url)
    toast({ title: t('common:success'), description: `Exported ${deviceTypes.length} device types` })
  }

  // Import device types from JSON file
  const handleImportClick = () => {
    fileInputRef.current?.click()
  }

  const handleFileChange = async (e: React.ChangeEvent<HTMLInputElement>) => {
    const file = e.target.files?.[0]
    if (!file) return

    setImporting(true)
    try {
      const text = await file.text()
      const imported = JSON.parse(text)

      // Handle single device type or array
      const typesToImport = Array.isArray(imported) ? imported : [imported]

      let successCount = 0
      let errorCount = 0

      for (const type of typesToImport) {
        try {
          if (onImportDeviceType) {
            await onImportDeviceType(type)
          } else {
            await api.addDeviceType(type)
          }
          successCount++
        } catch (err) {
          errorCount++
          console.error(`Failed to import ${type.device_type}:`, err)
        }
      }

      if (successCount > 0) {
        toast({
          title: t('common:success'),
          description: `Imported ${successCount} device type${successCount > 1 ? 's' : ''}${errorCount > 0 ? ` (${errorCount} failed)` : ''}`
        })
        onRefresh()
      } else {
        toast({
          title: t('common:failed'),
          description: 'No device types were imported',
          variant: 'destructive'
        })
      }
    } catch (error) {
      toast({
        title: t('common:failed'),
        description: 'Failed to parse JSON file',
        variant: 'destructive'
      })
    } finally {
      setImporting(false)
      if (fileInputRef.current) {
        fileInputRef.current.value = ''
      }
    }
  }

  const allOnPageSelected = paginatedDeviceTypes.length > 0 && paginatedDeviceTypes.every((t) => selectedIds.has(t.device_type))

  return (
    <>
      {/* Hidden file input for import */}
      <input
        ref={fileInputRef}
        type="file"
        accept=".json"
        className="hidden"
        onChange={handleFileChange}
      />

      {/* Toolbar */}
      <ActionBar
        title={t('devices:types.deviceTypes')}
        titleIcon={<FileJson className="h-5 w-5" />}
        description={`${deviceTypes.length} ${t('devices:types.totalTypes')}`}
        actions={
          <>
            <Button variant="outline" size="sm" onClick={handleImportClick} disabled={importing}>
              <Upload className="mr-2 h-4 w-4" />
              {importing ? 'Importing...' : 'Import'}
            </Button>
            <Button variant="outline" size="sm" onClick={handleExportAll} disabled={deviceTypes.length === 0}>
              <Download className="mr-2 h-4 w-4" />
              Export All
            </Button>
            <Dialog>
              <DialogTrigger asChild>
                <Button size="sm" onClick={onAddType}>
                  <Plus className="mr-2 h-4 w-4" />
                  {t('devices:addDeviceType')}
                </Button>
              </DialogTrigger>
              {addTypeDialog}
            </Dialog>
          </>
        }
        onRefresh={onRefresh}
      />

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

      {loading ? (
        <LoadingState text={t('devices:types.loading')} />
      ) : (
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
                <TableHead align="right">{t('devices:types.headers.actions')}</TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              {deviceTypes.length === 0 ? (
                <EmptyStateInline title={t('devices:types.noTypes')} colSpan={7} />
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
      )}

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
