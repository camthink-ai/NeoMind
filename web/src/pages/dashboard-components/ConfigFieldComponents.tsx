import { useRef } from 'react'
import { useTranslation } from 'react-i18next'
import { Trash2, Upload } from 'lucide-react'
import { notifyError } from '@/lib/notify'
import { compressImageFile } from '@/lib/imageUtils'
import { Field } from '@/components/ui/field'
import { Label } from '@/components/ui/label'
import { Input } from '@/components/ui/input'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'
import { Button } from '@/components/ui/button'

// ============================================================================
// SelectField - Unified select dropdown for config dialogs
// ============================================================================

export interface SelectOption {
  value: string
  label: string
}

export interface SelectFieldProps {
  label: string
  value: string
  onChange: (value: string) => void
  options: SelectOption[]
  className?: string
}

export function SelectField({ label, value, onChange, options, className }: SelectFieldProps) {
  const { t } = useTranslation('dashboardComponents')

  const handleChange = (newValue: string) => {
    onChange(newValue)
  }

  return (
    <Field className={className}>
      <Label>{label}</Label>
      <Select value={value} onValueChange={handleChange}>
        <SelectTrigger>
          <SelectValue placeholder={t('visualDashboard.selectPlaceholder', { label })} />
        </SelectTrigger>
        <SelectContent>
          {options.map((option) => (
            <SelectItem key={option.value} value={option.value}>
              {option.label}
            </SelectItem>
          ))}
        </SelectContent>
      </Select>
    </Field>
  )
}

// ============================================================================
// ImageSourceField - URL input + file upload for image config
// ============================================================================

export interface ImageSourceFieldProps {
  value: string
  onChange: (value: string) => void
}

export function ImageSourceField({ value, onChange }: ImageSourceFieldProps) {
  const { t } = useTranslation('dashboardComponents')
  const fileInputRef = useRef<HTMLInputElement>(null)

  const handleFileSelect = async (e: React.ChangeEvent<HTMLInputElement>) => {
    const file = e.target.files?.[0]
    if (!file) return

    if (!file.type.startsWith('image/')) {
      notifyError(t('visualDashboard.invalidFileType'))
      return
    }

    try {
      const compressed = await compressImageFile(file)
      onChange(compressed)
    } catch {
      notifyError(t('visualDashboard.imageCompressFailed'))
    }
    // Reset input to allow re-uploading the same file
    e.target.value = ''
  }

  const handleUploadClick = () => {
    fileInputRef.current?.click()
  }

  const handleClear = () => {
    onChange('')
  }

  const isBase64Image = value?.startsWith('data:image')

  return (
    <div className="space-y-3">
      <Field>
        <Label>{t('visualDashboard.imageSource')}</Label>
        <div className="flex gap-2">
          <Input
            value={value || ''}
            onChange={(e) => onChange(e.target.value)}
            placeholder={t('visualDashboard.urlPlaceholder')}
            className="h-9 flex-1"
          />
          <Button
            variant="outline"
            size="sm"
            onClick={handleUploadClick}
            className="h-9 px-3 shrink-0 sm:h-9"
          >
            <Upload className="h-4 w-4 mr-1.5" />
            {t('visualDashboard.upload')}
          </Button>
        </div>
        <input
          ref={fileInputRef}
          type="file"
          accept="image/*"
          onChange={handleFileSelect}
          className="hidden"
        />
        <p className="text-xs text-muted-foreground mt-1">
          {isBase64Image
            ? t('visualDashboard.uploadedHint')
            : t('visualDashboard.urlHint')}
        </p>
      </Field>

      {isBase64Image && (
        <div className="flex items-center gap-2">
          <div className="w-12 h-12 rounded border overflow-hidden bg-muted-30">
            <img
              src={value}
              alt="Preview"
              className="w-full h-full object-contain"
            />
          </div>
          <Button
            variant="ghost"
            size="sm"
            onClick={handleClear}
            className="h-8 text-error hover:text-error sm:h-8"
          >
            <Trash2 className="h-4 w-4 mr-1" />
            {t('visualDashboard.clear')}
          </Button>
        </div>
      )}
    </div>
  )
}
