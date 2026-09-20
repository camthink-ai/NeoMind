/**
 * DualModeSourceField
 *
 * A unified input field that integrates manual entry (URL/text/upload)
 * with data source binding via a dialog picker.
 *
 * UX Flow:
 * - Default (unbound): Shows manual input with a "Link" icon button.
 * - Click "Link" / "Bind data source": Opens a fixed-height dialog with
 *   the full UnifiedDataSourceConfig picker. Changes are staged locally
 *   and only committed when user clicks "Confirm".
 * - When bound: Shows bound source summary badge + "Change" / "Unbind" actions.
 * - Click "Unbind": Returns to manual input mode.
 */

import { useState, useRef, useCallback } from 'react'
import { useTranslation } from 'react-i18next'
import { Link2, Unlink, Upload, Trash2, Database, Server, Zap, Activity, Puzzle, Workflow, MapPin } from 'lucide-react'
import { Input } from '@/components/ui/input'
import { Button } from '@/components/ui/button'
import { normalizeImageUrl } from '@/lib/imageUtils'
import { Label } from '@/components/ui/label'
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
  DialogFooter,
} from '@/components/ui/dialog'
import { cn } from '@/lib/utils'
import { compressImageFile } from '@/lib/imageUtils'
import { textNano } from '@/design-system/tokens/typography'
import { UnifiedDataSourceConfig } from './UnifiedDataSourceConfig'
import { getSourceSummary } from './DataSourceIndicator'
import type { DataSourceOrList } from '@/types/dashboard'
import { normalizeDataSource } from '@/types/dashboard'

// ============================================================================
// Types
// ============================================================================

export type DualModeInputType = 'url' | 'text' | 'image'

export interface DualModeSourceFieldProps {
  /** Type of manual input to render */
  inputType: DualModeInputType
  /** Current manual input value */
  value: string
  /** Callback when manual input value changes */
  onValueChange: (value: string) => void
  /** Current data source (if bound) */
  dataSource?: DataSourceOrList
  /** Callback when data source changes */
  onDataSourceChange: (ds: DataSourceOrList | undefined) => void
  /** Allowed data source types */
  allowedTypes?: Array<'device-metric' | 'device-command' | 'device-info' | 'device' | 'metric' | 'command' | 'system' | 'extension' | 'extension-command' | 'transform'>
  /** Label for the field */
  label: string
  /** Placeholder text for manual input */
  placeholder?: string
  /** Textarea rows (for 'text' type) */
  rows?: number
  /** Additional class name */
  className?: string
}

// ============================================================================
// Helpers
// ============================================================================

function getTypeIcon(type?: string) {
  switch (type) {
    case 'device-metric': return Server
    case 'device-command': return Zap
    case 'device': return MapPin
    case 'system': return Activity
    case 'extension': return Puzzle
    case 'extension-command': return Zap
    case 'transform': return Workflow
    default: return Database
  }
}

// ============================================================================
// Component
// ============================================================================

export function DualModeSourceField({
  inputType,
  value,
  onValueChange,
  dataSource,
  onDataSourceChange,
  allowedTypes,
  label,
  placeholder,
  rows = 4,
  className,
}: DualModeSourceFieldProps) {
  const { t } = useTranslation('dashboardComponents')
  const fileInputRef = useRef<HTMLInputElement>(null)

  const [pickerOpen, setPickerOpen] = useState(false)
  // Staged selection inside the dialog — only committed on "Confirm"
  const [stagedDataSource, setStagedDataSource] = useState<DataSourceOrList | undefined>(undefined)

  // Check if we have a bound data source
  const normalizedSources = dataSource ? normalizeDataSource(dataSource) : []
  const isBound = normalizedSources.length > 0
  const boundSummary = isBound ? getSourceSummary(normalizedSources[0]) : ''

  // Handle file upload for image type
  const handleFileSelect = useCallback(async (e: React.ChangeEvent<HTMLInputElement>) => {
    const file = e.target.files?.[0]
    if (!file || !file.type.startsWith('image/')) return

    try {
      const compressed = await compressImageFile(file)
      onValueChange(compressed)
    } catch {
      // compression failed — ignore silently
    }
    e.target.value = ''
  }, [onValueChange])

  const handleClearImage = useCallback(() => {
    onValueChange('')
  }, [onValueChange])

  const handleUnbind = useCallback(() => {
    onDataSourceChange(undefined)
  }, [onDataSourceChange])

  const openPicker = useCallback(() => {
    // Pre-populate staged value with current binding
    setStagedDataSource(dataSource)
    setPickerOpen(true)
  }, [dataSource])

  const handleConfirm = useCallback(() => {
    onDataSourceChange(stagedDataSource)
    setPickerOpen(false)
  }, [onDataSourceChange, stagedDataSource])

  const handleCancel = useCallback(() => {
    setPickerOpen(false)
  }, [])

  const TypeIcon = isBound ? getTypeIcon(normalizedSources[0]?.type) : Database
  const isBase64Image = value?.startsWith('data:image') || value?.startsWith('/api/images/')

  // Does the staged selection differ from the current binding?
  const stagedSources = stagedDataSource ? normalizeDataSource(stagedDataSource) : []
  const stagedChanged = isBound
    ? // Was bound, check if staged is different or cleared
      JSON.stringify(normalizedSources) !== JSON.stringify(stagedSources)
    : // Was not bound, check if staged has something
      stagedSources.length > 0

  return (
    <div className={cn('space-y-3', className)}>
      {/* Mode indicator label */}
      <div className="flex items-center justify-between">
        <Label className="text-sm font-medium">{label}</Label>
        <span className={cn(
          'text-xs px-2 py-0.5 rounded-full',
          isBound
            ? 'bg-primary-light text-primary'
            : 'bg-muted text-muted-foreground'
        )}>
          {isBound ? t('dualMode.bound') : t('dualMode.manual')}
        </span>
      </div>

      {/* Bound state: show source summary */}
      {isBound ? (
        <div className="flex items-center gap-3 p-3 rounded-lg border bg-card">
          {/* Left accent bar */}
          <div className="w-1 self-stretch rounded-full bg-primary shrink-0" />
          {/* Type icon */}
          <div className="flex items-center justify-center h-9 w-9 rounded-lg bg-muted-50 shrink-0">
            <TypeIcon className="h-4 w-4 text-primary" />
          </div>
          {/* Info */}
          <div className="flex-1 min-w-0">
            <p className="text-sm font-medium truncate">{boundSummary || t('dualMode.dataBound')}</p>
            <span className={`inline-flex items-center rounded bg-muted px-1.5 py-0.5 ${textNano} font-medium text-muted-foreground mt-0.5`}>
              {normalizedSources[0]?.type}
            </span>
          </div>
          {/* Actions */}
          <div className="flex items-center gap-0.5 shrink-0">
            <Button
              variant="ghost"
              size="sm"
              onClick={openPicker}
              className="h-7 px-2 text-xs text-primary"
            >
              {t('dualMode.changeSource')}
            </Button>
            <Button
              variant="ghost"
              size="sm"
              onClick={handleUnbind}
              className="h-7 w-7 p-0 text-muted-foreground hover:text-error"
            >
              <Unlink className="h-3.5 w-3.5" />
            </Button>
          </div>
        </div>
      ) : (
        /* Unbound state: show manual input */
        <div className="space-y-2">
          {inputType === 'text' ? (
            <textarea
              value={value || ''}
              onChange={(e) => onValueChange(e.target.value)}
              placeholder={placeholder}
              rows={rows}
              className="w-full px-3 py-2 rounded-md border border-input bg-card text-sm resize-y"
            />
          ) : (
            <div className="flex gap-2">
              <Input
                value={value || ''}
                onChange={(e) => onValueChange(e.target.value)}
                placeholder={placeholder}
                className="h-9 flex-1"
              />
              {inputType === 'image' && (
                <Button
                  variant="outline"
                  size="sm"
                  onClick={() => fileInputRef.current?.click()}
                  className="h-9 px-3 shrink-0"
                >
                  <Upload className="h-4 w-4 mr-1.5" />
                  {t('dualMode.upload')}
                </Button>
              )}
              {/* Bind data source button */}
              <Button
                variant="outline"
                size="sm"
                onClick={openPicker}
                className="h-9 px-3 shrink-0"
                title={t('dualMode.bindSource')}
              >
                <Link2 className="h-4 w-4" />
              </Button>
            </div>
          )}

          {/* Image upload hint / preview */}
          {inputType === 'image' && (
            <>
              <input
                ref={fileInputRef}
                type="file"
                accept="image/*"
                onChange={handleFileSelect}
                className="hidden"
              />
              {isBase64Image ? (
                <div className="flex items-center gap-2">
                  <div className="w-10 h-10 rounded border overflow-hidden bg-muted-30">
                    <img src={value?.startsWith('/api/images/') ? normalizeImageUrl(value)?.src ?? value : value} alt="Preview" className="w-full h-full object-contain" />
                  </div>
                  <Button
                    variant="ghost"
                    size="sm"
                    onClick={handleClearImage}
                    className="h-7 text-error hover:text-error"
                  >
                    <Trash2 className="h-3 w-3 mr-1" />
                    {t('dualMode.clear')}
                  </Button>
                </div>
              ) : (
                <p className="text-xs text-muted-foreground">
                  {t('dualMode.orBindDataSource')}
                </p>
              )}
            </>
          )}

          {/* Text type: bind button */}
          {inputType === 'text' && (
            <button
              onClick={openPicker}
              className="flex items-center gap-1.5 text-xs text-primary hover:underline"
            >
              <Link2 className="h-3 w-3" />
              {t('dualMode.bindDataSourceInstead')}
            </button>
          )}
        </div>
      )}

      {/* Data source picker dialog */}
      <Dialog open={pickerOpen} onOpenChange={(open) => { if (!open) handleCancel() }}>
        <DialogContent className="z-[110] max-w-2xl !h-[70vh] flex flex-col !p-0 overflow-hidden">
          <DialogHeader className="px-5 py-3 border-b shrink-0">
            <DialogTitle className="text-base">{t('dualMode.selectDataSource')}</DialogTitle>
          </DialogHeader>

          {/* Picker body — fills remaining space */}
          <div className="flex-1 min-h-0 overflow-hidden">
            <UnifiedDataSourceConfig
              value={stagedDataSource}
              onChange={setStagedDataSource}
              allowedTypes={allowedTypes}
              className="border-0 h-full"
            />
          </div>

          {/* Footer: Cancel / Confirm */}
          <DialogFooter className="px-5 py-3 border-t shrink-0">
            <Button variant="outline" onClick={handleCancel}>
              {t('common.cancel')}
            </Button>
            <Button onClick={handleConfirm} disabled={!stagedChanged}>
              {t('common.confirm')}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </div>
  )
}
