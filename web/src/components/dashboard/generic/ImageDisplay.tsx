/**
 * Image Display Component
 *
 * Displays images from URLs or data sources.
 * Supports various object-fit modes, captions, and loading states.
 * Enhanced base64 support for various device formats.
 */

import { useState, useCallback, useRef, useEffect, useMemo } from 'react'
import { Skeleton } from '@/components/ui/skeleton'
import { Button } from '@/components/ui/button'
import { cn } from '@/lib/utils'
import { useDataSource } from '@/hooks/useDataSource'
import { dashboardCardBase, dashboardComponentSize } from '@/design-system/tokens/size'
import { Maximize2, Minimize2, Download, ImageOff, AlertTriangle, FileCode, RefreshCw } from 'lucide-react'
import type { DataSource } from '@/types/dashboard'

export interface ImageDisplayProps {
  dataSource?: DataSource
  src?: string
  alt?: string
  caption?: string
  size?: 'sm' | 'md' | 'lg'

  // Display options
  fit?: 'contain' | 'cover' | 'fill' | 'none' | 'scale-down'
  objectPosition?: string
  rounded?: boolean
  showShadow?: boolean

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
function normalizeImageUrl(value: string | undefined): {
  src: string
  format: ImageFormatType
  isBase64: boolean
  isDataUrl: boolean
  originalValue: string
} | null {
  if (!value) return null

  const trimmed = value.trim()

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
      originalValue: value,
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
      originalValue: value,
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
      originalValue: value,
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
      originalValue: value,
    }
  }

  // 5. HTTP/HTTPS URL
  if (trimmed.startsWith('http://') || trimmed.startsWith('https://')) {
    return {
      src: trimmed,
      format: 'unknown',
      isBase64: false,
      isDataUrl: false,
      originalValue: value,
    }
  }

  // 6. Relative URL
  if (trimmed.startsWith('/')) {
    return {
      src: trimmed,
      format: 'unknown',
      isBase64: false,
      isDataUrl: false,
      originalValue: value,
    }
  }

  // 7. Try to detect if it might be a URL without protocol
  if (trimmed.includes('.') && !trimmed.includes(' ')) {
    return {
      src: trimmed.startsWith('//') ? `https:${trimmed}` : `https://${trimmed}`,
      format: 'unknown',
      isBase64: false,
      isDataUrl: false,
      originalValue: value,
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

export function ImageDisplay({
  dataSource,
  src: propSrc,
  alt = 'Image',
  caption,
  size = 'md',
  fit = 'contain',
  objectPosition = 'center',
  rounded = true,
  showShadow = false,
  zoomable = true,
  downloadable = false,
  openInNewTab = false,
  className,
}: ImageDisplayProps) {
  const { data, loading, error } = useDataSource<string>(dataSource, {
    fallback: propSrc,
  })

  const rawSrc = error ? propSrc : (data ?? propSrc ?? '')

  // Normalize the image source
  const normalizedImage = useMemo(() => normalizeImageUrl(rawSrc), [rawSrc])
  const src = normalizedImage?.src || rawSrc
  const [isFullscreen, setIsFullscreen] = useState(false)
  const [imageLoadState, setImageLoadState] = useState<ImageLoadState>('no-source')
  const imageRef = useRef<HTMLImageElement>(null)

  // Determine if we have a valid image source
  const hasValidSource = normalizedImage !== null

  const formatInfo = useMemo(() => getFormatInfo(normalizedImage), [normalizedImage])

  const handleDownload = useCallback(() => {
    if (!src) return

    // For base64 images, extract the correct extension
    let filename = alt.replace(/[^a-z0-9]/gi, '_') || 'image'

    if (normalizedImage?.isBase64) {
      filename += `.${getFileExtension(normalizedImage.format)}`
    } else {
      // Try to get extension from URL
      try {
        const url = new URL(src)
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
    link.href = src
    link.download = filename
    document.body.appendChild(link)
    link.click()
    document.body.removeChild(link)
  }, [src, alt, normalizedImage])

  const sizeConfig = dashboardComponentSize[size]

  // Update image load state when src changes
  useEffect(() => {
    if (loading) {
      setImageLoadState('loading')
    } else if (!hasValidSource) {
      setImageLoadState('no-source')
    } else {
      setImageLoadState('loading')
    }
  }, [src, loading, hasValidSource])

  const handleImageLoad = useCallback(() => {
    setImageLoadState('loaded')
  }, [])

  const handleImageError = useCallback(() => {
    setImageLoadState('error')
  }, [])

  // Loading state from data source
  if (loading && imageLoadState === 'loading') {
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
          <p className="text-muted-foreground text-sm font-medium">No Image Source</p>
          <p className="text-muted-foreground/50 text-xs mt-1">Configure an image URL or data source</p>
        </div>
        {displaySrc && displaySrc !== '-' && (
          <details className="mt-2">
            <summary className="text-xs text-muted-foreground cursor-pointer hover:text-foreground">
              View raw data
            </summary>
            <div className="mt-2 p-2 bg-background/50 rounded border border-dashed border-border max-w-full">
              <FileCode className="h-3 w-3 text-muted-foreground mx-auto mb-1" />
              <p className="text-xs text-muted-foreground font-mono break-all max-h-16 overflow-auto" title={displaySrc}>
                {displaySrc.slice(0, 100)}{displaySrc.length > 100 ? '...' : ''}
              </p>
            </div>
          </details>
        )}
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
          <p className="text-muted-foreground text-sm font-medium">Failed to Load Image</p>
          <p className="text-muted-foreground/50 text-xs mt-1">The image could not be loaded</p>
        </div>
        {formatInfo && (
          <p className="text-xs text-muted-foreground/60">
            Format: {formatInfo.format} ({formatInfo.type})
          </p>
        )}
        <Button
          variant="outline"
          size="sm"
          className="gap-1.5"
          onClick={() => {
            setImageLoadState('loading')
            if (imageRef.current && src) {
              imageRef.current.src = src
            }
          }}
        >
          <RefreshCw className="h-3.5 w-3.5" />
          Retry
        </Button>
      </div>
    )
  }

  return (
    <>
      <div className={cn(dashboardCardBase, 'relative overflow-hidden', className)}>
        {/* Image container */}
        <div className="relative w-full h-full flex items-center justify-center bg-muted/10">
          <img
            ref={imageRef}
            key={src}
            src={src}
            alt={alt}
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

          {/* Action buttons overlay */}
          {(zoomable || downloadable || openInNewTab) && (
            <div className="absolute top-2 right-2 flex gap-1 opacity-0 hover:opacity-100 transition-opacity">
              {downloadable && (
                <Button
                  variant="secondary"
                  size="icon"
                  className="h-7 w-7"
                  onClick={handleDownload}
                  title={normalizedImage?.isBase64 ? `Download as ${normalizedImage.format.toUpperCase()}` : 'Download'}
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
                  title="View fullscreen"
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
          src={src}
          alt={alt}
          onClose={() => setIsFullscreen(false)}
        />
      )}
    </>
  )
}
