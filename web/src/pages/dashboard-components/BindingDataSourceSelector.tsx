import { useState, useCallback } from 'react'
import { useTranslation } from 'react-i18next'
import { Plus, Database } from 'lucide-react'
import { Button } from '@/components/ui/button'
import { Label } from '@/components/ui/label'
import { UnifiedFormDialog } from '@/components/dialog/UnifiedFormDialog'
import { UnifiedDataSourceConfig } from '@/components/dashboard/config'
import type { DataSourceOrList } from '@/types/dashboard'
import { normalizeDataSource } from '@/types/dashboard'

export function BindingDataSourceSelector({
  dataSource,
  onConfirm,
  allowedTypes,
  multiple = true,
  maxSources,
  title,
}: {
  dataSource?: DataSourceOrList
  onConfirm: (ds: DataSourceOrList | undefined) => void
  allowedTypes: string[]
  multiple?: boolean
  maxSources?: number
  title: string
}) {
  const { t } = useTranslation('dashboardComponents')
  const [pickerOpen, setPickerOpen] = useState(false)
  const [stagedDataSource, setStagedDataSource] = useState<DataSourceOrList | undefined>(undefined)

  const normalizedSources = dataSource ? normalizeDataSource(dataSource) : []
  const isBound = normalizedSources.length > 0

  const openPicker = useCallback(() => {
    setStagedDataSource(dataSource)
    setPickerOpen(true)
  }, [dataSource])

  const handleConfirm = useCallback(() => {
    onConfirm(stagedDataSource)
    setPickerOpen(false)
  }, [onConfirm, stagedDataSource])

  const handleCancel = useCallback(() => {
    setPickerOpen(false)
  }, [])

  const stagedSources = stagedDataSource ? normalizeDataSource(stagedDataSource) : []
  const stagedChanged = isBound
    ? JSON.stringify(normalizedSources) !== JSON.stringify(stagedSources)
    : stagedSources.length > 0

  return (
    <div className="space-y-2">
      <div className="flex items-center justify-between">
        <Label className="text-sm font-medium">{title}</Label>
        {isBound && (
          <span className="inline-flex items-center gap-1 text-xs px-2 py-0.5 rounded-full bg-primary-light text-primary">
            <Database className="h-3 w-3" />
            {t('bindingSelector.boundCount', { count: normalizedSources.length })}
          </span>
        )}
      </div>

      {isBound ? (
        <Button variant="outline" size="sm" onClick={openPicker} className="w-full">
          {t('bindingSelector.changeSource')}
        </Button>
      ) : (
        <Button
          variant="outline"
          onClick={openPicker}
          className="w-full h-10 border-dashed text-muted-foreground hover:text-primary"
        >
          <Plus className="h-4 w-4 mr-1.5" />
          {t('bindingSelector.addSource')}
        </Button>
      )}

      <UnifiedFormDialog
        open={pickerOpen}
        onOpenChange={(open) => { if (!open) handleCancel() }}
        title={t('dualMode.selectDataSource')}
        width="xl"
        className="sm:h-[72vh]"
        contentClassName="p-0 flex flex-col min-h-0 overflow-hidden"
        onSubmit={async () => { handleConfirm() }}
        submitLabel={t('bindingSelector.confirm')}
        cancelLabel={t('bindingSelector.cancel')}
        submitDisabled={!stagedChanged}
      >
        <div className="flex-1 min-h-0 overflow-hidden">
          <UnifiedDataSourceConfig
            value={stagedDataSource}
            onChange={setStagedDataSource}
            allowedTypes={allowedTypes as any}
            multiple={multiple}
            maxSources={maxSources}
            className="border-0 h-full"
          />
        </div>
      </UnifiedFormDialog>
    </div>
  )
}
