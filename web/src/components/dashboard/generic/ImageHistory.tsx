/**
 * Image History Component
 *
 * Displays historical image data with manual slider navigation.
 * Supports base64 images, URLs, and data URLs.
 */

import { useState, useCallback, useRef, useEffect, useMemo } from 'react'
import { Slider } from '@/components/ui/slider'
import { cn } from '@/lib/utils'
import { useDataSource } from '@/hooks/useDataSource'
import { dashboardCardBase, dashboardComponentSize } from '@/design-system/tokens/size'
import { ImageOff, AlertTriangle, RefreshCw, Images } from 'lucide-react'
import type { DataSource } from '@/types/dashboard'

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

function detectImageFormatFromMagicBytes(base64Data: string): { type: ImageFormatType; mime: string } | null {
  try {
    const pureBase64 = base64Data.replace(/^data:image\/[^;]+;base64,/, '').replace(/^data:,/, '')
    const binaryString = atob(pureBase64.slice(0, 32))

    for (const [name, info] of Object.entries(IMAGE_MAGIC_BYTES)) {
      if (info.magic.every((byte, i) => binaryString.charCodeAt(i) === byte)) {
        return { type: info.type as ImageFormatType, mime: info.mime }
      }
    }
  } catch {
    // Invalid base64
  }
  return null
}

function isPureBase64(str: string): boolean {
  if (!str || str.length < 100) return false
  const cleaned = str.trim()

  if (cleaned.startsWith('http://') || cleaned.startsWith('https://') || cleaned.startsWith('/')) {
    return false
  }
  if (cleaned.startsWith('data:')) {
    return false
  }

  const base64Regex = /^[A-Za-z0-9+/=_-]+$/
  if (!base64Regex.test(cleaned)) {
    return false
  }

  try {
    atob(cleaned.slice(0, 100))
    return true
  } catch {
    return false
  }
}

function normalizeImageUrl(value: string): string | null {
  if (!value) return null

  const trimmed = value.trim()

  if (trimmed === '-' || trimmed === 'undefined' || trimmed === 'null' || trimmed === '') {
    return null
  }

  if (trimmed.startsWith('data:image/')) {
    return trimmed
  }

  if (trimmed.startsWith('data:base64,')) {
    const base64Data = trimmed.slice(12)
    const formatInfo = detectImageFormatFromMagicBytes(base64Data) || { type: 'png', mime: 'image/png' }
    return `data:${formatInfo.mime};base64,${base64Data}`
  }

  if (trimmed.startsWith('data:')) {
    return trimmed
  }

  if (isPureBase64(trimmed)) {
    const formatInfo = detectImageFormatFromMagicBytes(trimmed) || { type: 'png', mime: 'image/png' }
    return `data:${formatInfo.mime};base64,${trimmed}`
  }

  return trimmed
}

export interface ImageHistoryItem {
  src: string
  timestamp?: string | number
  label?: string
  alt?: string
}

export interface ImageHistoryProps {
  dataSource?: DataSource
  images?: ImageHistoryItem[] | string[]
  title?: string
  size?: 'sm' | 'md' | 'lg'

  // Display options
  fit?: 'contain' | 'cover' | 'fill' | 'none' | 'scale-down'
  rounded?: boolean
  showTitle?: boolean

  // Data options for telemetry
  limit?: number
  timeRange?: number  // hours

  className?: string
}

type ImageLoadState = 'loading' | 'loaded' | 'error'

/**
 * Transform telemetry points to ImageHistoryItem array
 */
function transformTelemetryToImages(data: unknown): ImageHistoryItem[] {
  if (!Array.isArray(data)) return []

  const result: ImageHistoryItem[] = []

  for (let index = 0; index < data.length; index++) {
    const point = data[index]

    if (typeof point === 'string') {
      const normalizedSrc = normalizeImageUrl(point)
      if (normalizedSrc) {
        result.push({ src: normalizedSrc, alt: `Image ${index + 1}` })
      }
    } else if (typeof point === 'object' && point !== null) {
      const obj = point as Record<string, unknown>

      // Try various value fields
      const rawSrc = String(
        obj.src ??
        obj.url ??
        obj.image ??
        obj.value ??
        obj.v ??
        ''
      )

      const normalizedSrc = normalizeImageUrl(rawSrc)
      if (!normalizedSrc) continue

      // Extract timestamp
      let timestamp: string | number | undefined = undefined
      if (obj.timestamp) {
        timestamp = typeof obj.timestamp === 'number' ? obj.timestamp : String(obj.timestamp)
      } else if (obj.time) {
        timestamp = typeof obj.time === 'number' ? obj.time : String(obj.time)
      } else if (obj.t) {
        timestamp = typeof obj.t === 'number' ? obj.t : String(obj.t)
      }

      // Extract label
      const label = typeof obj.label === 'string' ? obj.label :
                    typeof obj.name === 'string' ? obj.name : undefined

      result.push({
        src: normalizedSrc,
        timestamp,
        label,
        alt: `Image ${index + 1}`,
      })
    }
  }

  return result
}

function normalizeImageData(data: unknown): ImageHistoryItem[] {
  if (data === null || data === undefined) {
    return []
  }

  if (Array.isArray(data)) {
    const result: ImageHistoryItem[] = []

    for (let index = 0; index < data.length; index++) {
      const item = data[index]

      if (typeof item === 'string') {
        const normalizedSrc = normalizeImageUrl(item)
        if (normalizedSrc) {
          result.push({
            src: normalizedSrc,
            alt: `Image ${index + 1}`,
          })
        }
      } else if (typeof item === 'object' && item !== null) {
        const obj = item as Record<string, unknown>
        const timestamp = obj.timestamp ?? obj.time
        const label = obj.label ?? obj.name
        const rawSrc = String(obj.src || obj.url || obj.image || obj.value || '')
        const normalizedSrc = normalizeImageUrl(rawSrc)

        if (normalizedSrc) {
          result.push({
            src: normalizedSrc,
            timestamp: (typeof timestamp === 'string' || typeof timestamp === 'number') ? timestamp : undefined,
            label: (typeof label === 'string') ? label : undefined,
            alt: (typeof obj.alt === 'string') ? obj.alt : `Image ${index + 1}`,
          })
        }
      } else {
        const normalizedSrc = normalizeImageUrl(String(item))
        if (normalizedSrc) {
          result.push({
            src: normalizedSrc,
            alt: `Image ${index + 1}`,
          })
        }
      }
    }

    return result
  }

  if (typeof data === 'string') {
    const normalizedSrc = normalizeImageUrl(data)
    if (normalizedSrc) {
      return [{ src: normalizedSrc, alt: 'Image 1' }]
    }
    return []
  }

  if (typeof data === 'object' && data !== null) {
    const obj = data as Record<string, unknown>
    const arrayProps = ['images', 'image', 'history', 'snapshots', 'frames', 'data', 'values', 'items', 'points']

    for (const prop of arrayProps) {
      if (prop in obj) {
        const value = obj[prop]
        if (Array.isArray(value)) {
          return normalizeImageData(value)
        }
      }
    }

    if ('src' in obj || 'url' in obj || 'image' in obj || 'value' in obj) {
      const rawSrc = String(obj.src || obj.url || obj.image || obj.value || '')
      const normalizedSrc = normalizeImageUrl(rawSrc)
      if (normalizedSrc) {
        return [{
          src: normalizedSrc,
          timestamp: (typeof obj.timestamp === 'string' || typeof obj.timestamp === 'number') ? obj.timestamp : undefined,
          label: (typeof obj.label === 'string') ? obj.label : undefined,
          alt: (typeof obj.alt === 'string') ? obj.alt : 'Image 1',
        }]
      }
    }
  }

  return []
}

function formatTimestamp(timestamp: string | number | undefined): string {
  if (!timestamp) return ''

  const date = new Date(typeof timestamp === 'number' ? timestamp * 1000 : timestamp)
  if (isNaN(date.getTime())) return String(timestamp)

  return date.toLocaleTimeString('zh-CN', {
    month: '2-digit',
    day: '2-digit',
    hour: '2-digit',
    minute: '2-digit',
    second: '2-digit',
  })
}

/**
 * Convert device data source to telemetry for historical data
 */
function normalizeDataSourceForImages(
  ds: DataSource | undefined,
  limit: number = 50,
  timeRange: number = 1
): DataSource | undefined {
  if (!ds) return undefined

  // If it's already telemetry, return as-is with raw transform and custom limits
  if (ds.type === 'telemetry') {
    return {
      ...ds,
      limit: ds.limit ?? limit,
      timeRange: ds.timeRange ?? timeRange,
      params: {
        ...ds.params,
        includeRawPoints: true,
      },
      transform: 'raw',
    }
  }

  // If it's a device type, convert to telemetry for historical data
  if (ds.type === 'device') {
    return {
      type: 'telemetry',
      deviceId: ds.deviceId,
      metricId: ds.property || 'image',
      timeRange: timeRange,
      limit: limit,
      aggregate: 'raw',
      params: {
        includeRawPoints: true,
      },
      transform: 'raw',
    }
  }

  return ds
}

export function ImageHistory({
  dataSource,
  images: propImages,
  title,
  size = 'md',
  fit = 'fill',
  rounded = true,
  showTitle = true,
  limit = 50,
  timeRange = 1,
  className,
}: ImageHistoryProps) {
  // Get size configuration
  const sizeConfig = dashboardComponentSize[size]

  // Normalize data source for image history (convert device to telemetry)
  const normalizedDataSource = useMemo(() => normalizeDataSourceForImages(dataSource, limit, timeRange), [dataSource, limit, timeRange])

  const { data, loading } = useDataSource<ImageHistoryItem[] | string[]>(normalizedDataSource, {
    fallback: propImages,
  })

  // Memoize images to prevent unnecessary recalculation
  const dataKey = useMemo(() => {
    if (!data) return 'no-data'
    if (Array.isArray(data)) {
      return `array-${data.length}-${JSON.stringify(data).slice(0, 100)}`
    }
    return `object-${JSON.stringify(data).slice(0, 50)}`
  }, [data])

  // Transform data to images - handles telemetry raw points
  const images = useMemo(() => {
    const normalized = normalizeImageData(data ?? propImages ?? [])
    return normalized.length > 0 ? normalized : transformTelemetryToImages(data ?? propImages ?? [])
  }, [dataKey, propImages])

  // Track images length to detect when images actually change
  const imagesLengthRef = useRef<number>(0)

  const [currentIndex, setCurrentIndex] = useState(0)
  const [imageLoadState, setImageLoadState] = useState<ImageLoadState>('loading')

  // Cache loaded image states
  const loadedImagesRef = useRef<Set<number>>(new Set())

  const currentImage = images[currentIndex]
  const currentImageSrc = currentImage?.src
  const hasImages = images.length > 0
  const canNavigate = images.length > 1

  // Reset index if images length changes
  useEffect(() => {
    const currentLength = images.length
    if (currentLength !== imagesLengthRef.current) {
      imagesLengthRef.current = currentLength
      setCurrentIndex(0)
      setImageLoadState('loading')
      loadedImagesRef.current.clear()
    }
  }, [images.length])

  // Update image load state when image source changes
  const prevImageSrcRef = useRef<string | undefined>(undefined)
  const prevIndexRef = useRef<number>(-1)
  useEffect(() => {
    const imageChanged = currentImageSrc && currentImageSrc !== prevImageSrcRef.current
    const indexChanged = currentIndex !== prevIndexRef.current

    if (imageChanged) {
      if (loadedImagesRef.current.has(currentIndex)) {
        setImageLoadState('loaded')
      } else {
        setImageLoadState('loading')
      }
      prevImageSrcRef.current = currentImageSrc
    } else if (indexChanged && currentImageSrc) {
      setImageLoadState(loadedImagesRef.current.has(currentIndex) ? 'loaded' : 'loading')
    }

    prevIndexRef.current = currentIndex
  }, [currentImageSrc, currentIndex])

  const handleImageLoad = useCallback(() => {
    setImageLoadState('loaded')
    loadedImagesRef.current.add(currentIndex)
  }, [currentIndex])

  const handleImageError = useCallback(() => {
    setImageLoadState('error')
  }, [])

  const handleSliderChange = useCallback((values: number[]) => {
    const index = values[0] ?? 0
    setCurrentIndex(index)
  }, [])

  // Loading state
  if (loading) {
    return (
      <div className={cn(dashboardCardBase, className)}>
        <div className={cn(
          'w-full flex items-center justify-center bg-muted/10',
          size === 'sm' ? 'h-[120px]' : size === 'md' ? 'h-[180px]' : 'h-[240px]'
        )}>
          <RefreshCw className="h-6 w-6 text-muted-foreground animate-spin" />
        </div>
      </div>
    )
  }

  // No images state
  if (!hasImages) {
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
          <p className="text-muted-foreground text-sm font-medium">No Images</p>
          <p className="text-muted-foreground/50 text-xs mt-1">Configure an image data source</p>
        </div>
      </div>
    )
  }

  return (
    <div className={cn(
      dashboardCardBase,
      'relative flex flex-col overflow-hidden',
      className
    )}>
      {/* Title header */}
      {title && showTitle && (
        <div className="flex items-center gap-2 px-3 py-2 bg-muted/30 border-b shrink-0">
          <Images className="h-4 w-4 text-muted-foreground shrink-0" />
          <span className="font-medium text-sm truncate flex-1">{title}</span>
        </div>
      )}

      <div className={cn(
        'w-full flex-1 relative',
        size === 'sm' ? 'h-[120px]' : size === 'md' ? 'h-[180px]' : 'h-[240px]'
      )}>
      {/* Image fills entire container */}
      <img
        key={`img-${currentIndex}`}
        src={currentImageSrc}
        alt={currentImage?.alt || `Image ${currentIndex + 1}`}
        className={cn(
          'w-full h-full',
          fit === 'contain' && 'object-contain',
          fit === 'cover' && 'object-cover',
          fit === 'fill' && 'object-fill',
          fit === 'none' && 'object-none',
          fit === 'scale-down' && 'object-scale-down'
        )}
        onLoad={handleImageLoad}
        onError={handleImageError}
      />

      {/* Top-left index overlay */}
      {canNavigate && (
        <div className="absolute top-2 left-2 bg-black/60 text-white text-xs px-2 py-1 rounded z-10">
          {currentIndex + 1} / {images.length}
        </div>
      )}

      {/* Top-right timestamp overlay */}
      {currentImage?.timestamp && (
        <div className="absolute top-2 right-2 bg-black/60 text-white text-xs px-2 py-1 rounded z-10">
          {formatTimestamp(currentImage.timestamp)}
        </div>
      )}

      {/* Label overlay */}
      {currentImage?.label && (
        <div className="absolute top-10 left-2 bg-black/60 text-white text-xs px-2 py-1 rounded max-w-[150px] truncate z-10">
          {currentImage.label}
        </div>
      )}

      {/* Loading indicator */}
      {imageLoadState === 'loading' && (
        <div className="absolute inset-0 flex items-center justify-center bg-background/50 z-20">
          <RefreshCw className="h-6 w-6 text-muted-foreground animate-spin" />
        </div>
      )}

      {/* Error indicator */}
      {imageLoadState === 'error' && (
        <div className="absolute inset-0 flex flex-col items-center justify-center bg-background/80 z-20">
          <AlertTriangle className="h-8 w-8 text-destructive mb-2" />
          <span className="text-sm text-muted-foreground">Failed to load</span>
        </div>
      )}

      {/* Floating slider at bottom - always on top */}
      {canNavigate && (
        <div className="absolute bottom-0 left-0 right-0 px-3 py-3 bg-gradient-to-t from-black/70 via-black/40 to-transparent z-30">
          <div className="flex items-center gap-3">
            <Slider
              value={[currentIndex]}
              min={0}
              max={images.length - 1}
              step={1}
              onValueChange={handleSliderChange}
              className="flex-1"
            />
            <span className="text-xs text-white tabular-nums min-w-[50px] text-right shrink-0">
              {currentIndex + 1} / {images.length}
            </span>
          </div>
        </div>
      )}
      </div>
    </div>
  )
}
