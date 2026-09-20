import { useState, useRef, useCallback } from 'react'
import { useTranslation } from 'react-i18next'
import { format } from 'date-fns'
import { UnifiedFormDialog } from '@/components/dialog/UnifiedFormDialog'
import { Input } from '@/components/ui/input'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Popover, PopoverContent, PopoverTrigger } from '@/components/ui/popover'
import { Calendar } from '@/components/ui/calendar'

import { Download, CalendarIcon } from 'lucide-react'
import { api, getServerOrigin, tokenManager, getApiKey } from '@/lib/api'
import { useToast } from '@/hooks/use-toast'
import { textNano } from '@/design-system/tokens/typography'
import { cn } from '@/lib/utils'
import type { UnifiedDataSourceInfo } from '@/types'

interface ExportDataDialogProps {
  open: boolean
  onOpenChange: (open: boolean) => void
  source: UnifiedDataSourceInfo | null
}

/** Excel cell max length */
const EXCEL_CELL_MAX = 32000

/** Format Date for display: "yyyy-MM-dd HH:mm:ss" */
function formatDateTimeDisplay(date: Date): string {
  return format(date, 'yyyy-MM-dd HH:mm:ss')
}

/** Format Unix seconds to human-readable "yyyy-MM-dd HH:mm:ss" */
function formatTimestamp(unixSeconds: number): string {
  return formatDateTimeDisplay(new Date(unixSeconds * 1000))
}

/** Sanitize string for use in filenames */
function sanitizeFilename(s: string): string {
  return s.replace(/[:\s]/g, '_').replace(/[^a-zA-Z0-9_\-.]/g, '')
}

/** Get current time as filename-safe string: "YYYYMMDD_HHmmss" */
function fileTimestamp(): string {
  return format(new Date(), 'yyyyMMdd_HHmmss')
}

/**
 * Time Picker following shadcn/ui convention:
 * Uses <Input type="time"> with native picker indicator hidden via CSS.
 * Renders as a clean text input (HH:mm:ss) that works in Tauri.
 */
function TimePicker({ value, onChange }: { value: string; onChange: (v: string) => void }) {
  return (
    <Input
      type="time"
      step="1"
      value={value}
      onChange={e => onChange(e.target.value)}
      className="h-9 w-full text-sm appearance-none bg-background [&::-webkit-calendar-picker-indicator]:hidden [&::-webkit-calendar-picker-indicator]:appearance-none"
    />
  )
}

/** Truncate value string to Excel cell limit */
function truncateForExcel(val: string): string {
  if (val.length <= EXCEL_CELL_MAX) return val
  return val.slice(0, EXCEL_CELL_MAX) + '...[truncated]'
}

/** Check if a value looks like base64 image data */
function isBase64Image(value: unknown): value is string {
  if (typeof value !== 'string') return false
  return value.startsWith('data:image/') || value.startsWith('data:application/octet-stream')
}

/** Check if a string is likely base64-encoded binary data */
function isBase64Binary(value: unknown): value is string {
  if (typeof value !== 'string') return false
  // Long strings that match base64 pattern (no data: prefix)
  if (value.length < 100) return false
  return /^[A-Za-z0-9+/=\s]+$/.test(value.slice(0, 200))
}

/** Check if a value is a stored /api/images/ URL (image file on disk) */
function isApiImageUrl(value: unknown): value is string {
  if (typeof value !== 'string') return false
  return value.startsWith('/api/images/')
}

/** True if a data point value is any kind of image content we can bundle. */
function isImageValue(value: unknown): boolean {
  return isBase64Image(value) || isBase64Binary(value) || isApiImageUrl(value)
}

/** Detect if the data contains image/binary content that should go to ZIP */
function detectImageContent(data: Array<{ value: unknown }>): boolean {
  if (data.length === 0) return false
  // Check first few data points
  const sample = data.slice(0, 5)
  return sample.some(p => isImageValue(p.value))
}

/** Extract a normalized image file extension from an /api/images/ URL path. */
function extFromUrlPath(url: string): string {
  const m = url.split('?')[0].match(/\.([a-zA-Z0-9]+)$/)
  const ext = m ? m[1].toLowerCase() : ''
  const known: Record<string, string> = {
    png: 'png', jpg: 'jpg', jpeg: 'jpg', gif: 'gif',
    webp: 'webp', bmp: 'bmp', svg: 'svg', tiff: 'tiff', tif: 'tiff',
  }
  return known[ext] ?? 'bin'
}

/**
 * Resolve an image point's value to raw bytes + extension. Handles base64 data
 * URIs, raw base64, and /api/images/ URLs (fetched with the same auth headers
 * as the rest of the API). Returns null if it can't be resolved (e.g. file
 * deleted) so the caller can keep the raw value instead of dropping the point.
 */
async function resolveImageBytes(value: string): Promise<{ bytes: Uint8Array; ext: string } | null> {
  if (isBase64Image(value) || isBase64Binary(value)) {
    const parsed = parseImageData(value)
    const binaryStr = atob(parsed.base64)
    const bytes = new Uint8Array(binaryStr.length)
    for (let j = 0; j < binaryStr.length; j++) bytes[j] = binaryStr.charCodeAt(j)
    return { bytes, ext: parsed.ext }
  }
  if (isApiImageUrl(value)) {
    try {
      const headers: Record<string, string> = {}
      const token = tokenManager.getToken()
      if (token) headers['Authorization'] = `Bearer ${token}`
      const apiKey = getApiKey()
      if (apiKey) headers['X-API-Key'] = apiKey
      const resp = await fetch(getServerOrigin() + value, { headers })
      if (!resp.ok) return null
      return { bytes: new Uint8Array(await resp.arrayBuffer()), ext: extFromUrlPath(value) }
    } catch {
      return null
    }
  }
  return null
}

/** Extract MIME type and extension from data URI or guess from base64 */
function parseImageData(value: string): { mime: string; ext: string; base64: string } {
  const dataUriMatch = value.match(/^data:([^;]+);base64,(.+)$/s)
  if (dataUriMatch) {
    const mime = dataUriMatch[1]
    const base64 = dataUriMatch[2]
    const extMap: Record<string, string> = {
      'image/png': 'png', 'image/jpeg': 'jpg', 'image/jpg': 'jpg',
      'image/gif': 'gif', 'image/webp': 'webp', 'image/bmp': 'bmp',
      'image/svg+xml': 'svg',
    }
    return { mime, ext: extMap[mime] || 'bin', base64 }
  }
  // Raw base64 — assume png
  return { mime: 'image/png', ext: 'png', base64: value }
}

export function ExportDataDialog({ open, onOpenChange, source }: ExportDataDialogProps) {
  const { t } = useTranslation('data')
  const { toast } = useToast()
  const abortRef = useRef<AbortController | null>(null)

  const now = new Date()
  const defaultStart = new Date(now.getTime() - 24 * 3600 * 1000)
  const [startDate, setStartDate] = useState<Date>(defaultStart)
  const [startTime, setStartTime] = useState(format(defaultStart, 'HH:mm:ss'))
  const [endDate, setEndDate] = useState<Date>(now)
  const [endTime, setEndTime] = useState(format(now, 'HH:mm:ss'))
  const [exporting, setExporting] = useState(false)
  const [error, setError] = useState<string | null>(null)

  // Reset state when source changes
  const handleOpenChange = useCallback((nextOpen: boolean) => {
    if (!nextOpen) {
      abortRef.current?.abort()
      setExporting(false)
      setError(null)
    }
    onOpenChange(nextOpen)
  }, [onOpenChange])

  const handleExport = useCallback(async () => {
    if (!source) return

    abortRef.current?.abort()
    const controller = new AbortController()
    abortRef.current = controller

    setError(null)
    setExporting(true)

    try {
      // Parse DataSourceId: "device:sensor1:temperature" → source="device:sensor1", metric="temperature"
      const parts = source.id.split(':')
      const sourcePart = `${parts[0]}:${parts[1]}`
      const metricPart = parts.slice(2).join(':')

      // Combine date + time into full Date objects, then convert to Unix seconds
      const [startH, startM, startS] = startTime.split(':').map(Number)
      const startDateFull = new Date(startDate)
      startDateFull.setHours(startH, startM, startS || 0, 0)

      const [endH, endM, endS] = endTime.split(':').map(Number)
      const endDateFull = new Date(endDate)
      endDateFull.setHours(endH, endM, endS || 0, 0)

      const startUnix = Math.floor(startDateFull.getTime() / 1000)
      const endUnix = Math.floor(endDateFull.getTime() / 1000)

      const result = await api.queryTelemetry(sourcePart, metricPart, startUnix, endUnix, 1000)
      if (controller.signal.aborted) return

      const data = result?.data || []
      if (data.length === 0) {
        setError(t('export.noData'))
        return
      }

      // Warn about truncation
      if (result.total_count != null && result.total_count > data.length) {
        toast({
          title: t('export.truncated', { count: data.length, total: result.total_count }),
        })
      }

      // Generate file — auto-detect image content from actual data values
      const baseName = `${sanitizeFilename(source.source_display_name)}_${sanitizeFilename(source.field_display_name)}_${fileTimestamp()}`
      const isImageContent = detectImageContent(data)

      if (isImageContent || source.data_type === 'binary') {
        await generateZip(baseName, data, source)
        toast({
          title: t('export.success'),
          description: `${baseName}.zip`,
        })
      } else {
        await generateCsv(baseName, data)
        toast({
          title: t('export.success'),
          description: `${baseName}.csv`,
        })
      }

      // Close dialog on success
      handleOpenChange(false)
    } catch (err) {
      if (controller.signal.aborted) return
      console.error('[ExportDataDialog] Export failed:', err)
      setError(err instanceof Error ? err.message : String(err))
    } finally {
      if (!controller.signal.aborted) {
        setExporting(false)
      }
    }
  }, [source, startDate, startTime, endDate, endTime, t, toast, handleOpenChange])

  return (
    <UnifiedFormDialog
      open={open}
      onOpenChange={handleOpenChange}
      title={t('export.title')}
      description={source?.id}
      icon={<Download className="h-5 w-5" />}
      width="md"
      submitLabel={exporting ? t('export.exporting') : t('export.button')}
      onSubmit={handleExport}
      isSubmitting={exporting}
      submitDisabled={exporting}
      showCancelButton
    >
      {source && (
        <div className="space-y-4">
          {/* Source Info */}
          <div className="grid grid-cols-2 gap-3">
            <div>
              <p className="text-xs text-muted-foreground">{t('columns.source')}</p>
              <p className="text-sm font-medium">{source.source_display_name}</p>
            </div>
            <div>
              <p className="text-xs text-muted-foreground">{t('columns.field')}</p>
              <p className="text-sm font-medium">{source.field_display_name}</p>
            </div>
            <div>
              <p className="text-xs text-muted-foreground">{t('columns.dataType')}</p>
              <Badge variant="secondary" className={textNano}>{source.data_type}</Badge>
            </div>
            <div>
              <p className="text-xs text-muted-foreground">{t('export.format.auto')}</p>
              <p className="text-sm">{t('export.format.autoDesc')}</p>
            </div>
          </div>

          {/* Time Range - two columns: start | end */}
          <div className="grid grid-cols-2 gap-4">
            {/* Start */}
            <div className="space-y-2">
              <label className="text-xs text-muted-foreground block">{t('export.startTime')}</label>
              <Popover>
                <PopoverTrigger asChild>
                  <Button
                    variant="outline"
                    className={cn(
                      "h-9 w-full justify-start text-left text-sm font-normal",
                      !startDate && "text-muted-foreground"
                    )}
                  >
                    <CalendarIcon className="mr-1.5 h-3.5 w-3.5 shrink-0" />
                    {startDate ? format(startDate, 'yyyy-MM-dd') : 'Pick date'}
                  </Button>
                </PopoverTrigger>
                <PopoverContent className="w-auto p-0" align="start">
                  <Calendar
                    mode="single"
                    selected={startDate}
                    onSelect={(d) => d && setStartDate(d)}
                    initialFocus
                  />
                </PopoverContent>
              </Popover>
              <TimePicker value={startTime} onChange={setStartTime} />
            </div>

            {/* End */}
            <div className="space-y-2">
              <label className="text-xs text-muted-foreground block">{t('export.endTime')}</label>
              <Popover>
                <PopoverTrigger asChild>
                  <Button
                    variant="outline"
                    className={cn(
                      "h-9 w-full justify-start text-left text-sm font-normal",
                      !endDate && "text-muted-foreground"
                    )}
                  >
                    <CalendarIcon className="mr-1.5 h-3.5 w-3.5 shrink-0" />
                    {endDate ? format(endDate, 'yyyy-MM-dd') : 'Pick date'}
                  </Button>
                </PopoverTrigger>
                <PopoverContent className="w-auto p-0" align="start">
                  <Calendar
                    mode="single"
                    selected={endDate}
                    onSelect={(d) => d && setEndDate(d)}
                    initialFocus
                  />
                </PopoverContent>
              </Popover>
              <TimePicker value={endTime} onChange={setEndTime} />
            </div>
          </div>

          {/* Error */}
          {error && (
            <p className="text-sm text-error bg-error-light rounded px-3 py-2">{error}</p>
          )}
        </div>
      )}
    </UnifiedFormDialog>
  )
}

/** Generate and download a CSV file (opens natively in Excel).
 *  Replaces the former xlsx-based export: xlsx (SheetJS 0.18.x) carries high
 *  CVEs (CVE-2023-30533, CVE-2024-22363) on its read/parse path, and this
 *  export only ever writes our own tabular data — so CSV removes the
 *  vulnerable dependency entirely with no new dep and the same usability. */
async function generateCsv(
  baseName: string,
  data: Array<{ timestamp: number; value: unknown; quality: number | null }>,
) {
  // RFC 4180: quote any field containing comma / quote / newline; double inner quotes
  const esc = (v: string) => (/[",\n\r]/.test(v) ? `"${v.replace(/"/g, '""')}"` : v)

  const rows = data.map(p => {
    const rawValue = typeof p.value === 'object' ? JSON.stringify(p.value) : String(p.value ?? '')
    return [
      formatTimestamp(p.timestamp),
      truncateForExcel(rawValue),
      p.quality !== null ? `${(p.quality * 100).toFixed(0)}%` : '-',
    ]
      .map(esc)
      .join(',')
  })

  const header = ['Timestamp', 'Value', 'Quality'].map(esc).join(',')
  // Leading UTF-8 BOM so Excel decodes Unicode (Chinese etc.) correctly
  const csv = '﻿' + [header, ...rows].join('\n')
  const blob = new Blob([csv], { type: 'text/csv;charset=utf-8;' })
  const url = URL.createObjectURL(blob)
  const a = document.createElement('a')
  a.href = url
  a.download = `${baseName}.csv`
  document.body.appendChild(a)
  a.click()
  document.body.removeChild(a)
  setTimeout(() => URL.revokeObjectURL(url), 100)
}

/** Generate and download a ZIP file with decoded images */
async function generateZip(
  baseName: string,
  data: Array<{ timestamp: number; value: unknown; quality: number | null }>,
  source: UnifiedDataSourceInfo,
) {
  const JSZip = (await import('jszip')).default
  const zip = new JSZip()
  let _imageCount = 0

  // manifest.json with metadata
  zip.file('manifest.json', JSON.stringify({
    source: source.id,
    source_name: source.source_display_name,
    field: source.field_display_name,
    data_type: source.data_type,
    point_count: data.length,
    exported_at: new Date().toISOString(),
  }, null, 2))

  // data.csv: one row per point. Image points resolve to a file under images/;
  // points that can't be resolved (e.g. /api/images/ file deleted) or aren't
  // images keep their raw value so no data is lost.
  const csvRows = ['Index,Filename,Timestamp,Quality,Size']
  for (let i = 0; i < data.length; i++) {
    const p = data[i]
    const valStr = typeof p.value === 'string' ? p.value : JSON.stringify(p.value)
    const timeStr = formatTimestamp(p.timestamp).replace(/[: ]/g, '-')
    const q = p.quality !== null ? `${(p.quality * 100).toFixed(0)}%` : '-'

    if (isImageValue(p.value)) {
      const resolved = await resolveImageBytes(valStr)
      if (resolved) {
        const filename = `${String(i + 1).padStart(4, '0')}_${timeStr}.${resolved.ext}`
        zip.file(`images/${filename}`, resolved.bytes)
        _imageCount += 1
        csvRows.push(`${i + 1},${filename},${formatTimestamp(p.timestamp)},${q},${resolved.bytes.length} bytes`)
      } else {
        csvRows.push(`${i + 1},-,${formatTimestamp(p.timestamp)},${q},"${valStr.replace(/"/g, '""')}"`)
      }
    } else {
      csvRows.push(`${i + 1},-,${formatTimestamp(p.timestamp)},${q},"${valStr.replace(/"/g, '""')}"`)
    }
  }
  zip.file('data.csv', csvRows.join('\n'))

  const blob = await zip.generateAsync({ type: 'blob' })
  const url = URL.createObjectURL(blob)
  const a = document.createElement('a')
  a.href = url
  a.download = `${baseName}.zip`
  document.body.appendChild(a)
  a.click()
  document.body.removeChild(a)
  setTimeout(() => URL.revokeObjectURL(url), 100)
}
