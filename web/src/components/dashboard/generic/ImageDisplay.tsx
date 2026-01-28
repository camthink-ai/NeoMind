/**
 * Image Display Component
 *
 * Displays images from URLs or data sources.
 * Supports various object-fit modes, captions, and loading states.
 * Enhanced base64 support for various device formats.
 */

import { useState, useCallback, useRef, useEffect, useMemo } from 'react'
import { useTranslation } from 'react-i18next'
import { Skeleton } from '@/components/ui/skeleton'
import { Button } from '@/components/ui/button'
import { cn } from '@/lib/utils'
import { useDataSource } from '@/hooks/useDataSource'
import { dashboardCardBase, dashboardComponentSize } from '@/design-system/tokens/size'
import { Maximize2, Minimize2, Download, ImageOff, AlertTriangle, RefreshCw, Image as ImageIcon } from 'lucide-react'
import type { DataSource } from '@/types/dashboard'

export interface ImageDisplayProps {
  dataSource?: DataSource
  src?: string
  alt?: string
  title?: string
  caption?: string
  size?: 'sm' | 'md' | 'lg'

  // Display options
  fit?: 'contain' | 'cover' | 'fill' | 'none' | 'scale-down'
  objectPosition?: string
  rounded?: boolean
  showShadow?: boolean
  showTitle?: boolean

  // Interactive features
  zoomable?: boolean
  downloadable?: boolean
  openInNewTab?: boolean

  className?: string
}

interface FullscreenImageProps {
  src: string | undefined
  alt: string
  onClose: () => void
}

function FullscreenImage({ src, alt, onClose }: FullscreenImageProps) {
  if (!src) return null
  return (
    <div
      className="fixed inset-0 z-50 bg-background/95 backdrop-blur-sm flex items-center justify-center"
      onClick={onClose}
    >
      <Button
        variant="ghost"
        size="icon"
        className="absolute top-4 right-4"
        onClick={onClose}
      >
        <Minimize2 className="h-5 w-5" />
      </Button>
      <img
        src={src}
        alt={alt}
        className="max-w-[95vw] max-h-[95vh] object-contain"
        onClick={(e) => e.stopPropagation()}
      />
    </div>
  )
}

type ImageLoadState = 'loading' | 'loaded' | 'error' | 'no-source' | 'invalid-format'

type ImageFormatType = 'png' | 'jpeg' | 'jpg' | 'gif' | 'webp' | 'bmp' | 'svg' | 'tiff' | 'ico' | 'unknown'

// Magic bytes for image type detection
const IMAGE_MAGIC_BYTES: Record<string, { magic: number[]; type: ImageFormatType; mime: string }> = {
  png: { magic: [0x89, 0x50, 0x4E, 0x47], type: 'png', mime: 'image/png' },
  jpeg: { magic: [0xFF, 0xD8, 0xFF], type: 'jpeg', mime: 'image/jpeg' },
  jpg: { magic: [0xFF, 0xD8, 0xFF], type: 'jpg', mime: 'image/jpeg' },
  gif: { magic: [0x47, 0x49, 0x46], type: 'gif', mime: 'image/gif' },
  webp: { magic: [0x52, 0x49, 0x46, 0x46], type: 'webp', mime: 'image/webp' },
  bmp: { magic: [0x42, 0x4D], type: 'bmp', mime: 'image/bmp' },
  tiff: { magic: [0x49, 0x49, 0x2A, 0x00], type: 'tiff', mime: 'image/tiff' },
  ico: { magic: [0x00, 0x00, 0x01, 0x00], type: 'ico', mime: 'image/x-icon' },
}

/**
 * Detect image format from magic bytes (first few bytes of data)
 */
function detectImageFormatFromMagicBytes(base64Data: string): { type: ImageFormatType; mime: string } | null {
  try {
    // Remove any data URL prefix and get pure base64
    const pureBase64 = base64Data.replace(/^data:image\/[^;]+;base64,/, '').replace(/^data:,/, '')
    const binaryString = atob(pureBase64.slice(0, 32)) // Only check first 32 chars

    for (const [name, info] of Object.entries(IMAGE_MAGIC_BYTES)) {
      if (info.magic.every((byte, i) => binaryString.charCodeAt(i) === byte)) {
        return { type: info.type as ImageFormatType, mime: info.mime }
      }
    }
  } catch {
    // Invalid base64, continue to next check
  }
  return null
}

/**
 * Detect image format from mime type string
 */
function detectFormatFromMimeType(mime: string): { type: ImageFormatType; mime: string } | null {
  const mimeToFormat: Record<string, ImageFormatType> = {
    'image/png': 'png',
    'image/jpeg': 'jpg',
    'image/jpg': 'jpg',
    'image/gif': 'gif',
    'image/webp': 'webp',
    'image/bmp': 'bmp',
    'image/svg+xml': 'svg',
    'image/tiff': 'tiff',
    'image/x-icon': 'ico',
    'image/vnd.microsoft.icon': 'ico',
  }

  const format = mimeToFormat[mime.toLowerCase()]
  if (format) {
    return { type: format, mime: mime.toLowerCase() }
  }
  return null
}

/**
 * Check if a string looks like pure base64 (no prefix)
 */
function isPureBase64(str: string): boolean {
  if (!str || str.length < 100) return false
  // Remove any whitespace
  const cleaned = str.trim()

  // Check if it starts with http(s) - then it's a URL, not base64
  if (cleaned.startsWith('http://') || cleaned.startsWith('https://') || cleaned.startsWith('/')) {
    return false
  }

  // Check if it starts with data: - then it's already a data URL
  if (cleaned.startsWith('data:')) {
    return false
  }

  // Check if it looks like base64 (only base64 characters and reasonable length)
  const base64Regex = /^[A-Za-z0-9+/=_-]+$/
  if (!base64Regex.test(cleaned)) {
    return false
  }

  // Try to decode it - if successful, it's likely base64
  try {
    atob(cleaned.slice(0, 100))
    return true
  } catch {
    return false
  }
}

/**
 * Normalize various image formats to a standard data URL
 */
function normalizeImageUrl(value: string | number | undefined | null): {
  src: string
  format: ImageFormatType
  isBase64: boolean
  isDataUrl: boolean
  originalValue: string
} | null {
  if (!value) return null

  // Convert to string first, then trim
  const valueStr = String(value)
  const trimmed = valueStr.trim()

  // Handle empty/placeholder values
  if (trimmed === '-' || trimmed === 'undefined' || trimmed === 'null' || trimmed === '') {
    return null
  }

  // 1. Already a proper data URL
  if (trimmed.startsWith('data:image/')) {
    // Extract mime type
    const mimeMatch = trimmed.match(/data:image\/([^;]+)/i)
    const mime = mimeMatch ? `image/${mimeMatch[1].toLowerCase()}` : 'image/png'
    const formatInfo = detectFormatFromMimeType(mime) || { type: 'png', mime }

    return {
      src: trimmed,
      format: formatInfo.type,
      isBase64: trimmed.includes('base64'),
      isDataUrl: true,
      originalValue: valueStr,
    }
  }

  // 2. Data URL without image/ prefix (malformed)
  if (trimmed.startsWith('data:base64,')) {
    const base64Data = trimmed.slice(12)
    const formatInfo = detectImageFormatFromMagicBytes(base64Data) || { type: 'png', mime: 'image/png' }
    return {
      src: `data:${formatInfo.mime};base64,${base64Data}`,
      format: formatInfo.type,
      isBase64: true,
      isDataUrl: true,
      originalValue: valueStr,
    }
  }

  // 3. Data URL with charset (e.g., data:image/jpeg;charset=utf-8;base64,...)
  if (trimmed.startsWith('data:')) {
    // Just return as-is, browser should handle it
    return {
      src: trimmed,
      format: 'png',
      isBase64: trimmed.includes('base64'),
      isDataUrl: true,
      originalValue: valueStr,
    }
  }

  // 4. Pure base64 string (no prefix)
  if (isPureBase64(trimmed)) {
    const formatInfo = detectImageFormatFromMagicBytes(trimmed) || { type: 'png', mime: 'image/png' }
    return {
      src: `data:${formatInfo.mime};base64,${trimmed}`,
      format: formatInfo.type,
      isBase64: true,
      isDataUrl: true,
      originalValue: valueStr,
    }
  }

  // 5. HTTP/HTTPS URL
  if (trimmed.startsWith('http://') || trimmed.startsWith('https://')) {
    return {
      src: trimmed,
      format: 'unknown',
      isBase64: false,
      isDataUrl: false,
      originalValue: valueStr,
    }
  }

  // 6. Relative URL
  if (trimmed.startsWith('/')) {
    return {
      src: trimmed,
      format: 'unknown',
      isBase64: false,
      isDataUrl: false,
      originalValue: valueStr,
    }
  }

  // 7. Try to detect if it might be a URL without protocol
  if (trimmed.includes('.') && !trimmed.includes(' ')) {
    return {
      src: trimmed.startsWith('//') ? `https:${trimmed}` : `https://${trimmed}`,
      format: 'unknown',
      isBase64: false,
      isDataUrl: false,
      originalValue: valueStr,
    }
  }

  // If we can't determine the format, return null
  return null
}

/**
 * Get file extension from format type
 */
function getFileExtension(format: ImageFormatType): string {
  const extensions: Record<ImageFormatType, string> = {
    png: 'png',
    jpeg: 'jpg',
    jpg: 'jpg',
    gif: 'gif',
    webp: 'webp',
    bmp: 'bmp',
    svg: 'svg',
    tiff: 'tiff',
    ico: 'ico',
    unknown: 'png',
  }
  return extensions[format] || 'png'
}

/**
 * Get format info for display
 */
function getFormatInfo(normalized: ReturnType<typeof normalizeImageUrl>) {
  if (!normalized) return null

  if (normalized.isBase64) {
    return {
      type: 'base64',
      format: normalized.format.toUpperCase(),
      size: Math.round((normalized.originalValue.length * 3) / 4 / 1024), // Approx KB
    }
  }

  if (normalized.isDataUrl) {
    return {
      type: 'data-url',
      format: 'DATA',
    }
  }

  return {
    type: 'url',
    format: 'URL',
  }
}

/**
 * Normalize data source for image display
 * Ensures raw points are included for telemetry sources
 */
function normalizeDataSourceForImage(
  ds: DataSource | undefined
): DataSource | undefined {
  if (!ds) return undefined

  // If it's telemetry, ensure raw points are included
  if (ds.type === 'telemetry') {
    return {
      ...ds,
      params: {
        ...ds.params,
        includeRawPoints: true,
      },
      transform: 'raw',
    }
  }

  return ds
}

export function ImageDisplay({
  dataSource,
  src: propSrc,
  alt,
  title,
  caption,
  size = 'md',
  fit = 'contain',
  objectPosition = 'center',
  rounded = true,
  showShadow = false,
  showTitle = true,
  zoomable = true,
  downloadable = false,
  openInNewTab = false,
  className,
}: ImageDisplayProps) {
  const { t } = useTranslation('dashboardComponents')
  const altText = alt || t('imageDisplay.altText')

  // Normalize data source to ensure raw points are included
  const normalizedDataSource = useMemo(() => normalizeDataSourceForImage(dataSource), [dataSource])

  const { data, loading, error } = useDataSource(normalizedDataSource, {
    fallback: propSrc,
  })

  // Extract image value from various data formats
  const extractImageValue = (value: unknown): string => {
    if (value === null || value === undefined) return ''
    if (typeof value === 'string') return value
    if (typeof value === 'number') return String(value)
    if (typeof value === 'boolean') return String(value)

    // Handle arrays - take the value with the latest timestamp
    if (Array.isArray(value)) {
      if (value.length === 0) return ''

      // Find the item with the latest timestamp
      let latestItem = value[0]
      let latestTime = -1

      for (const item of value) {
        if (typeof item === 'object' && item !== null) {
          const obj = item as Record<string, unknown>
          const time = (obj.time ?? obj.timestamp ?? obj.t ?? 0) as number
          if (time > latestTime) {
            latestTime = time
            latestItem = item
          }
        }
      }

      console.log('[ImageDisplay] Array analysis:', {
        totalItems: value.length,
        latestTime,
        latestItemPreview: JSON.stringify(latestItem)?.slice(0, 200),
      })

      return extractImageValue(latestItem)
    }

    // Handle objects - try to find image-related fields
    if (typeof value === 'object') {
      const obj = value as Record<string, unknown>

      // Common image field names
      for (const key of ['src', 'url', 'image', 'imageUrl', 'image_url', 'data', 'value', 'content']) {
        if (obj[key] !== null && obj[key] !== undefined) {
          const extracted = extractImageValue(obj[key])
          if (extracted && extracted !== '' && extracted !== '-') {
            return extracted
          }
        }
      }
    }

    return ''
  }

  const rawSrc = error ? propSrc : (extractImageValue(data) ?? propSrc ?? '')

  // Debug logging to understand data flow
  useEffect(() => {
    console.log('[ImageDisplay] Data received:', {
      dataType: Array.isArray(data) ? 'array' : typeof data,
      dataLength: Array.isArray(data) ? data.length : 1,
      firstItem: Array.isArray(data) && data.length > 0 ? JSON.stringify(data[0])?.slice(0, 100) : data,
      lastItem: Array.isArray(data) && data.length > 0 ? JSON.stringify(data[data.length - 1])?.slice(0, 100) : data,
      extractedSrc: rawSrc?.slice(0, 100),
    })
  }, [data, rawSrc])

  // Track last update timestamp for cache-busting
  const lastUpdate = useRef(Date.now())

  // Normalize the image source (for metadata extraction)
  const normalizedImage = useMemo(() => normalizeImageUrl(rawSrc), [rawSrc])
  const baseSrc = normalizedImage?.src || rawSrc

  // Update lastUpdate timestamp when rawSrc changes
  useEffect(() => {
    if (rawSrc && rawSrc !== propSrc) {
      lastUpdate.current = Date.now()
    }
  }, [rawSrc, propSrc])

  // For base64 images, add cache-busting parameter to force refresh
  // This ensures that when new data arrives, the image actually reloads
  const displaySrc = useMemo(() => {
    if (!baseSrc) return baseSrc

    // Add cache-busting for base64 images and data URLs
    if (baseSrc.startsWith('data:') || baseSrc.startsWith('blob:')) {
      // Use the last update timestamp as cache buster
      const cacheBuster = lastUpdate.current
      // Append a fragment identifier with timestamp
      // The browser will treat it as a "different" URL
      return `${baseSrc}#${cacheBuster}`
    }

    return baseSrc
  }, [baseSrc])

  // Keep originalSrc without cache-buster for download/fullscreen
  const originalSrc = baseSrc
  const [isFullscreen, setIsFullscreen] = useState(false)
  const [imageLoadState, setImageLoadState] = useState<ImageLoadState>('no-source')
  const imageRef = useRef<HTMLImageElement>(null)

  // Determine if we have a valid image source
  const hasValidSource = normalizedImage !== null

  const formatInfo = useMemo(() => getFormatInfo(normalizedImage), [normalizedImage])

  const handleDownload = useCallback(() => {
    if (!displaySrc || !originalSrc) return

    // For base64 images, extract the correct extension
    let filename = altText.replace(/[^a-z0-9]/gi, '_') || 'image'

    if (normalizedImage?.isBase64) {
      filename += `.${getFileExtension(normalizedImage.format)}`
    } else {
      // Try to get extension from URL (use originalSrc without cache-buster)
      try {
        const url = new URL(originalSrc)
        const ext = url.pathname.split('.').pop()?.toLowerCase()
        if (ext && ['jpg', 'jpeg', 'png', 'gif', 'webp', 'svg', 'bmp', 'ico'].includes(ext)) {
          filename += `.${ext}`
        } else {
          filename += '.png'
        }
      } catch {
        filename += '.png'
      }
    }

    const link = document.createElement('a')
    link.href = originalSrc  // Use originalSrc for download (without cache-buster)
    link.download = filename
    document.body.appendChild(link)
    link.click()
    document.body.removeChild(link)
  }, [displaySrc, originalSrc, alt, normalizedImage])

  const sizeConfig = dashboardComponentSize[size]

  // Determine if we should show loading state
  // Only show loading when there's actually a dataSource to load from
  const hasDataSource = dataSource !== undefined
  const shouldShowLoading = loading && hasDataSource && imageLoadState === 'loading'

  // Update image load state when src changes
  useEffect(() => {
    if (!hasDataSource) {
      // No data source configured - show no-source state
      setImageLoadState('no-source')
    } else if (loading) {
      setImageLoadState('loading')
    } else if (!hasValidSource) {
      setImageLoadState('no-source')
    } else {
      setImageLoadState('loading')
    }
  }, [displaySrc, loading, hasValidSource, hasDataSource])

  const handleImageLoad = useCallback(() => {
    setImageLoadState('loaded')
  }, [])

  const handleImageError = useCallback(() => {
    setImageLoadState('error')
  }, [])

  // Loading state from data source
  // Only show loading when there's actually a dataSource to load from
  if (shouldShowLoading) {
    return (
      <div className={cn(dashboardCardBase, 'flex items-center justify-center', sizeConfig.padding, className)}>
        <Skeleton className={cn('w-full h-full', rounded && 'rounded-lg')} />
      </div>
    )
  }

  // No source configured or invalid format
  if (!hasValidSource && imageLoadState !== 'error') {
    const displaySrc = rawSrc || ''
    return (
      <div className={cn(
        dashboardCardBase,
        'flex flex-col items-center justify-center gap-3 bg-muted/30',
        sizeConfig.padding,
        className
      )}>
        <ImageOff className={cn(
          'text-muted-foreground/60',
          size === 'sm' ? 'h-8 w-8' : size === 'md' ? 'h-12 w-12' : 'h-16 w-16'
        )} />
        <div className="text-center">
          <p className="text-muted-foreground text-sm font-medium">{t('imageDisplay.noImageSource')}</p>
          <p className="text-muted-foreground/50 text-xs mt-1">{t('imageDisplay.configureSource')}</p>
        </div>
      </div>
    )
  }

  // Image load error
  if (imageLoadState === 'error') {
    return (
      <div className={cn(
        dashboardCardBase,
        'flex flex-col items-center justify-center gap-3 bg-muted/30',
        sizeConfig.padding,
        className
      )}>
        <div className="relative">
          <ImageOff className={cn(
            'text-muted-foreground/40',
            size === 'sm' ? 'h-10 w-10' : size === 'md' ? 'h-14 w-14' : 'h-18 w-18'
          )} />
          <AlertTriangle className={cn(
            'absolute -bottom-1 -right-1 text-destructive bg-background rounded-full',
            'h-5 w-5'
          )} />
        </div>
        <div className="text-center">
          <p className="text-muted-foreground text-sm font-medium">{t('imageDisplay.failedToLoad')}</p>
          <p className="text-muted-foreground/50 text-xs mt-1">{t('imageDisplay.couldNotLoad')}</p>
        </div>
        {formatInfo && (
          <p className="text-xs text-muted-foreground/60">
            {t('imageDisplay.format')}: {formatInfo.format} ({formatInfo.type})
          </p>
        )}
        <Button
          variant="outline"
          size="sm"
          className="gap-1.5"
          onClick={() => {
            setImageLoadState('loading')
            if (imageRef.current && displaySrc) {
              imageRef.current.src = displaySrc
            }
          }}
        >
          <RefreshCw className="h-3.5 w-3.5" />
          {t('imageDisplay.retry')}
        </Button>
      </div>
    )
  }

  return (
    <>
      <div className={cn(dashboardCardBase, 'relative overflow-hidden flex flex-col', className)}>
        {/* Image container */}
        <div className="relative w-full h-full flex items-center justify-center bg-muted/10 flex-1 min-h-0">
          <img
            ref={imageRef}
            key={displaySrc}
            src={displaySrc}
            alt={altText}
            className={cn(
              'w-full h-full transition-transform duration-200',
              fit === 'contain' && 'object-contain',
              fit === 'cover' && 'object-cover',
              fit === 'fill' && 'object-fill',
              fit === 'none' && 'object-none',
              fit === 'scale-down' && 'object-scale-down',
              rounded && 'rounded-lg',
              showShadow && 'shadow-lg'
            )}
            style={{ objectPosition }}
            onClick={() => zoomable && setIsFullscreen(true)}
            onLoad={handleImageLoad}
            onError={handleImageError}
          />

          {/* Floating title overlay - same style as ImageHistory */}
          {title && showTitle && (
            <div className="absolute top-2 left-2 flex items-center gap-2 bg-black/60 text-white text-xs px-2 py-1 rounded">
              <ImageIcon className="h-3.5 w-3.5 text-white/90 shrink-0" />
              <span className="font-medium text-xs truncate max-w-[150px] text-white drop-shadow-md">{title}</span>
            </div>
          )}

          {/* Action buttons overlay */}
          {(zoomable || downloadable || openInNewTab) && (
            <div className="absolute top-2 right-2 flex gap-1 opacity-0 hover:opacity-100 transition-opacity">
              {downloadable && (
                <Button
                  variant="secondary"
                  size="icon"
                  className="h-7 w-7"
                  onClick={handleDownload}
                  title={normalizedImage?.isBase64
                    ? t('imageDisplay.downloadAs', { format: normalizedImage.format.toUpperCase() })
                    : t('imageDisplay.download')
                  }
                >
                  <Download className="h-3.5 w-3.5" />
                </Button>
              )}
              {zoomable && (
                <Button
                  variant="secondary"
                  size="icon"
                  className="h-7 w-7"
                  onClick={() => setIsFullscreen(true)}
                  title={t('imageDisplay.viewFullscreen')}
                >
                  <Maximize2 className="h-3.5 w-3.5" />
                </Button>
              )}
            </div>
          )}
        </div>

        {/* Caption */}
        {caption && (
          <div className={cn(
            'absolute bottom-0 left-0 right-0 bg-gradient-to-t from-black/60 to-transparent p-3',
            sizeConfig.padding
          )}>
            <p className="text-white text-sm drop-shadow-md">{caption}</p>
          </div>
        )}
      </div>

      {/* Fullscreen view */}
      {isFullscreen && (
        <FullscreenImage
          src={originalSrc}
          alt={altText}
          onClose={() => setIsFullscreen(false)}
        />
      )}
    </>
  )
}
