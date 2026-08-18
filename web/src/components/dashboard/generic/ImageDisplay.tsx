/**
 * Image Display Component
 *
 * Displays images from URLs or data sources.
 * Supports various object-fit modes, captions, and loading states.
 * Enhanced base64 support for various device formats.
 */

import { useState, useCallback, useRef, useEffect, useMemo, memo } from 'react'
import { createPortal } from 'react-dom'
import { useTranslation } from 'react-i18next'
import { Button } from '@/components/ui/button'
import { cn } from '@/lib/utils'
import { getPortalRoot } from '@/lib/portal'
import { useDataSource } from '@/hooks/useDataSource'
import { dashboardCardBase, dashboardComponentSize } from '@/design-system/tokens/size'
import { Maximize2, Minimize2, Download, ImageOff, AlertTriangle, RefreshCw, Image as ImageIcon } from 'lucide-react'
import type { DataSource } from '@/types/dashboard'
import { getSourceId } from '@/types/dashboard'
import { LoadingState } from '../shared'
import {
  normalizeImageUrl,
  extractImageValue,
  getFileExtension,
  type ImageFormatType,
} from '@/lib/imageUtils'

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

// ============================================================================
// Fullscreen overlay (stable — own component avoids parent re-renders)
// ============================================================================

const FullscreenImage = memo(function FullscreenImage({ src, alt, onClose }: {
  src: string | undefined; alt: string; onClose: () => void
}) {
  if (!src) return null
  return (
    <div className="fixed inset-0 z-[110] bg-bg-95 backdrop-blur-sm flex items-center justify-center" onClick={onClose}>
      <Button variant="ghost" size="icon" className="absolute top-4 right-4" onClick={onClose}>
        <Minimize2 className="h-5 w-5" />
      </Button>
      <img src={src} alt={alt} className="max-w-[95vw] max-h-[95vh] object-contain" onClick={(e) => e.stopPropagation()} />
    </div>
  )
})

// ============================================================================
// Helpers
// ============================================================================

type ImageLoadState = 'loading' | 'loaded' | 'error' | 'no-source'

function getFormatInfo(normalized: ReturnType<typeof normalizeImageUrl>) {
  if (!normalized) return null
  if (normalized.isBase64) {
    return { type: 'base64', format: normalized.format.toUpperCase(), size: Math.round((normalized.originalValue.length * 3) / 4 / 1024) }
  }
  if (normalized.isDataUrl) return { type: 'data-url', format: 'DATA' }
  return { type: 'url', format: 'URL' }
}

function normalizeDataSourceForImage(ds: DataSource | undefined): DataSource | undefined {
  if (!ds) return undefined
  if (ds.type === 'telemetry') {
    return {
      ...ds,
      timeRange: ds.timeRange && ds.timeRange > 24 ? ds.timeRange : 24,
      limit: 2, // Only need latest image — avoid fetching hundreds of base64 blobs
      timeWindow: undefined, // Use timeRange (hours) instead of timeWindow
      params: { ...ds.params, includeRawPoints: true, isImage: true },
      transform: 'raw',
    }
  }
  if (ds.type === 'device' || ds.type === 'metric') {
    return {
      type: 'telemetry',
      sourceId: getSourceId(ds),
      metricId: ds.metricId ?? ds.property ?? 'image',
      timeRange: ds.timeRange && ds.timeRange > 24 ? ds.timeRange : 24,
      limit: 2, // Only need latest image
      aggregate: 'raw',
      params: { includeRawPoints: true, isImage: true },
      transform: 'raw',
      refresh: ds.refresh ?? 30,
    }
  }
  return ds
}

// ============================================================================
// Component
// ============================================================================

export const ImageDisplay = memo(function ImageDisplay({
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

  // Data source — memoized to avoid re-normalizing
  const normalizedDataSource = useMemo(() => normalizeDataSourceForImage(dataSource), [dataSource])
  const hasNormalizedDataSource = normalizedDataSource !== undefined

  const { data, loading, error, lastUpdate: dataSourceLastUpdate } = useDataSource(
    hasNormalizedDataSource ? normalizedDataSource : undefined,
    { fallback: propSrc },
  )

  // Derive effective data — when there's no DS, use propSrc directly
  const effectiveData = hasNormalizedDataSource ? data : propSrc

  // Extract raw image src from data — memoized
  const rawSrc = useMemo(() => {
    if (error) return propSrc ?? ''
    return extractImageValue(effectiveData) || propSrc || ''
  }, [error, effectiveData, propSrc])

  // Normalize image URL — memoized; normalizeImageUrl has internal cache
  const normalizedImage = useMemo(() => normalizeImageUrl(rawSrc), [rawSrc])
  const baseSrc = normalizedImage?.src || rawSrc
  const hasValidSource = normalizedImage !== null
  const formatInfo = useMemo(() => getFormatInfo(normalizedImage), [normalizedImage])

  // Cache-bust timestamp — derived, not state
  const cacheBustTimestamp = dataSourceLastUpdate ?? 0

  // Display src — only add cache buster for data URLs
  const displaySrc = useMemo(() => {
    if (!baseSrc) return baseSrc
    if ((baseSrc.startsWith('data:') || baseSrc.startsWith('blob:')) && cacheBustTimestamp) {
      return `${baseSrc}#${cacheBustTimestamp}`
    }
    return baseSrc
  }, [baseSrc, cacheBustTimestamp])

  const originalSrc = baseSrc
  const sizeConfig = dashboardComponentSize[size]
  const hasDataSource = dataSource !== undefined

  // States
  const [isFullscreen, setIsFullscreen] = useState(false)
  const [imageLoadState, setImageLoadState] = useState<ImageLoadState>('no-source')
  const imageRef = useRef<HTMLImageElement>(null)
  const prevDisplaySrcRef = useRef<string | undefined>()

  // Update load state when src / loading changes.
  // For real-time WS push updates (HTTP→HTTP src change): DON'T show loading spinner.
  // Keep showing the old image until the new one loads (like NE101 Camera pattern).
  // Only show loading on initial load (no previous image) or API fetch loading.
  useEffect(() => {
    const srcChanged = displaySrc !== prevDisplaySrcRef.current
    const wasHttp = prevDisplaySrcRef.current?.startsWith('http')
    if (displaySrc) prevDisplaySrcRef.current = displaySrc

    if (!hasDataSource) {
      setImageLoadState('no-source')
    } else if (loading && !displaySrc) {
      // Only show loading if we have NO image yet (initial load)
      setImageLoadState('loading')
    } else if (!hasValidSource) {
      setImageLoadState('no-source')
    } else if (srcChanged && !wasHttp) {
      // Show loading only for base64→URL or no-source→source transitions.
      // Skip for HTTP→HTTP (real-time WS push with URL storage).
      setImageLoadState('loading')
    }
  }, [displaySrc, loading, hasValidSource, hasDataSource])

  // Callbacks
  const handleDownload = useCallback(() => {
    if (!originalSrc) return
    let filename = altText.replace(/[^a-z0-9]/gi, '_') || 'image'
    if (normalizedImage?.isBase64) {
      filename += `.${getFileExtension(normalizedImage.format)}`
    } else {
      try {
        const ext = new URL(originalSrc).pathname.split('.').pop()?.toLowerCase()
        filename += (ext && ['jpg', 'jpeg', 'png', 'gif', 'webp', 'svg', 'bmp', 'ico'].includes(ext)) ? `.${ext}` : '.png'
      } catch { filename += '.png' }
    }
    const link = document.createElement('a')
    link.href = originalSrc
    link.download = filename
    document.body.appendChild(link)
    link.click()
    document.body.removeChild(link)
  }, [originalSrc, altText, normalizedImage])

  const handleImageLoad = useCallback(() => setImageLoadState('loaded'), [])
  const handleImageError = useCallback(() => setImageLoadState('error'), [])

  const shouldShowLoading = loading && hasDataSource && !displaySrc

  // --- Loading skeleton ---
  if (shouldShowLoading) {
    return <LoadingState size={size} className={className} />
  }

  // --- No source ---
  if (!loading && !hasValidSource && imageLoadState !== 'error') {
    return (
      <div className={cn(dashboardCardBase, 'h-full flex flex-col items-center justify-center gap-3', sizeConfig.padding, className)}>
        <ImageOff className={cn('text-muted-foreground', size === 'sm' ? 'h-8 w-8' : size === 'md' ? 'h-12 w-12' : 'h-16 w-16')} />
        <div className="text-center">
          <p className="text-muted-foreground text-sm font-medium">{t('imageDisplay.noImageSource')}</p>
          <p className="text-muted-foreground text-xs mt-1">{t('imageDisplay.configureSource')}</p>
        </div>
      </div>
    )
  }

  // --- Error ---
  if (imageLoadState === 'error') {
    return (
      <div className={cn(dashboardCardBase, 'flex flex-col items-center justify-center gap-3', sizeConfig.padding, className)}>
        <div className="relative">
          <ImageOff className={cn('text-muted-foreground', size === 'sm' ? 'h-10 w-10' : size === 'md' ? 'h-14 w-14' : 'h-18 w-18')} />
          <AlertTriangle className="absolute -bottom-1 -right-1 text-error bg-background rounded-full h-5 w-5" />
        </div>
        <div className="text-center">
          <p className="text-muted-foreground text-sm font-medium">{t('imageDisplay.failedToLoad')}</p>
          <p className="text-muted-foreground text-xs mt-1">{t('imageDisplay.couldNotLoad')}</p>
        </div>
        {formatInfo && <p className="text-xs text-muted-foreground">{t('imageDisplay.format')}: {formatInfo.format} ({formatInfo.type})</p>}
        <Button variant="outline" size="sm" className="gap-1.5" onClick={() => { setImageLoadState('loading'); if (imageRef.current && displaySrc) imageRef.current.src = displaySrc }}>
          <RefreshCw className="h-4 w-4" />{t('imageDisplay.retry')}
        </Button>
      </div>
    )
  }

  // --- Main render ---
  return (
    <>
      <div className={cn(dashboardCardBase, 'relative overflow-hidden flex flex-col', className)}>
        <div className="relative w-full h-full flex items-center justify-center bg-muted flex-1 min-h-0">
          <img
            ref={imageRef}
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

          {title && showTitle && (
            <div className="absolute top-2 left-2 flex items-center gap-2 bg-overlay-medium text-white text-xs px-2 py-1 rounded">
              <ImageIcon className="h-4 w-4 text-white/90 shrink-0" />
              <span className="font-medium text-xs truncate max-w-[150px] text-white drop-shadow-md">{title}</span>
            </div>
          )}

          {(zoomable || downloadable || openInNewTab) && (
            <div className="absolute top-2 right-2 flex gap-1 opacity-0 hover:opacity-100 transition-opacity">
              {downloadable && (
                <Button variant="secondary" size="icon" className="h-6 w-6" onClick={handleDownload}
                  title={normalizedImage?.isBase64 ? t('imageDisplay.downloadAs', { format: normalizedImage.format.toUpperCase() }) : t('imageDisplay.download')}>
                  <Download className="h-4 w-4" />
                </Button>
              )}
              {zoomable && (
                <Button variant="secondary" size="icon" className="h-6 w-6" onClick={() => setIsFullscreen(true)} title={t('imageDisplay.viewFullscreen')}>
                  <Maximize2 className="h-4 w-4" />
                </Button>
              )}
            </div>
          )}
        </div>

        {caption && (
          <div className={cn('absolute bottom-0 left-0 right-0 bg-gradient-to-t from-black/60 to-transparent p-3', sizeConfig.padding)}>
            <p className="text-white text-sm drop-shadow-md">{caption}</p>
          </div>
        )}
      </div>

      {isFullscreen && createPortal(
        <FullscreenImage src={originalSrc} alt={altText} onClose={() => setIsFullscreen(false)} />,
        getPortalRoot(),
      )}
    </>
  )
})
