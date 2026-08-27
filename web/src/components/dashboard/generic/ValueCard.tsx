/**
 * Value Card Component (Unified Styles)
 *
 * Fills 100% of container using unified dashboard styles.
 * Size prop controls relative scale, not fixed dimensions.
 * Uses raw data values directly.
 */

import { useMemo, memo } from 'react'
import { ArrowUpRight, ArrowDownRight, Minus, Activity, TrendingUp, TrendingDown } from 'lucide-react'
import { cn, getIconForEntity } from '@/lib/utils'
import { chartColors, indicatorFontWeight, indicatorColors, dashboardCardBase, dashboardCardHorizontal } from '@/design-system'
import { valueCardSize, type ValueCardSize } from '@/design-system/tokens/size'
import type { DataSourceOrList, DataSource, TelemetryAggregate } from '@/types/dashboard'
import { normalizeDataSource } from '@/types/dashboard'
import { useDataSource } from '@/hooks/useDataSource'
import { aggregateData, getEffectiveAggregate } from '@/lib/telemetryTransform'
import { ErrorState, LoadingState } from '../shared'
import { useTrendCache } from '../shared/useTrendCache'

// ============================================================================
// Trend cache key helper
// ============================================================================

function getTrendCacheKey(dataSource: DataSourceOrList | undefined): string {
  if (!dataSource) return ''
  if (Array.isArray(dataSource)) return `multi:${dataSource.length}`
  if (typeof dataSource === 'string') return `ref:${dataSource}`
  return JSON.stringify(dataSource)
}

// ============================================================================
// Props
// ============================================================================

export interface ValueCardProps {
  dataSource?: DataSourceOrList

  // Display
  title?: string
  unit?: string
  prefix?: string
  icon?: string
  iconType?: 'auto' | 'entity' | 'emoji'
  description?: string

  // Trend - auto-calculated from data
  showTrend?: boolean

  // Styling - controls relative scale, not fixed size
  size?: ValueCardSize
  variant?: 'default' | 'vertical' | 'compact' | 'minimal'
  iconColor?: string
  valueColor?: string

  className?: string
}

// ============================================================================
// Icon Renderer (Responsive)
// ============================================================================

interface ValueIconProps {
  icon?: string
  title?: string
  iconType?: 'auto' | 'entity' | 'emoji'
  size: ValueCardSize
  className?: string
  iconColor?: string
}

function ValueIcon({ icon, title, iconType = 'entity', size, className, iconColor }: ValueIconProps) {
  const config = valueCardSize[size]

  // Emoji fallback - use fixed font-size to prevent scaling
  if (icon && iconType === 'emoji') {
    const emojiSize = size === 'sm' ? '1.125rem' : size === 'md' ? '1.25rem' : '1.5rem'
    return <span className={cn('opacity-80 shrink-0', className)} style={{ fontSize: emojiSize }}>{icon}</span>
  }

  // Get SVG icon
  const getIcon = () => {
    if (!icon && title) return getIconForEntity(title)
    if (icon) return getIconForEntity(icon)
    return Activity
  }

  const IconComponent = getIcon()

  // Convert hex to rgba for background opacity
  const hexToRgba = (hex: string, alpha: number) => {
    const cleanHex = hex.replace('#', '')
    const r = parseInt(cleanHex.substring(0, 2), 16)
    const g = parseInt(cleanHex.substring(2, 4), 16)
    const b = parseInt(cleanHex.substring(4, 6), 16)
    return `rgba(${r}, ${g}, ${b}, ${alpha})`
  }

  // Use custom icon color or default primary color
  const iconBgColor = iconColor ? hexToRgba(iconColor, 0.15) : undefined
  const iconTextColor = iconColor || undefined

  // Get icon size in pixels for fixed sizing
  const getIconSize = () => {
    switch (size) {
      case 'sm': return 14
      case 'md': return 16
      case 'lg': return 20
      default: return 16
    }
  }

  return (
    <div
      className={cn(
        'flex items-center justify-center rounded-lg shrink-0',
        'bg-muted text-primary',
        config.iconContainer,
        className
      )}
      style={{
        backgroundColor: iconBgColor,
        color: iconTextColor
      }}
    >
      <IconComponent
        className={cn('shrink-0')}
        width={getIconSize()}
        height={getIconSize()}
        strokeWidth={2}
      />
    </div>
  )
}

// ============================================================================
// Main Component
// ============================================================================

export const ValueCard = memo(function ValueCard({
  dataSource,
  title,
  unit,
  prefix = '',
  icon,
  iconType = 'entity',
  description,
  showTrend = false,
  size = 'md',
  variant = 'default',
  iconColor,
  valueColor,
  className,
}: ValueCardProps) {
  const trendCache = useTrendCache()
  const { data, loading, error } = useDataSource<unknown>(dataSource, {
    fallback: null,
  })

  // Treat empty arrays as "no data" — the pipeline uses [] for empty fetches.
  const hasData = data !== null && data !== undefined && !(Array.isArray(data) && data.length === 0)
  const showLoading = loading && !hasData

  // Check if dataSource is configured
  const hasDataSource = dataSource !== undefined

  // Get effective aggregate from dataSource (after resolveDataSource normalization)
  const effectiveAggregate = useMemo<TelemetryAggregate>(() => {
    const sources = normalizeDataSource(dataSource)
    return sources.length > 0 ? getEffectiveAggregate(sources[0]) : 'latest'
  }, [dataSource])

  // Extract numeric value from data for calculations
  const extractNumericValue = useMemo(() => {
    if (data === null || data === undefined) return null

    // When data is an array of raw telemetry points with timestamps,
    // apply aggregateExt before extracting the value
    if (Array.isArray(data) && data.length > 0 && typeof data[0] === 'object' && data[0] !== null && 'timestamp' in (data[0] as object)) {
      const timePoints = data.map((p: unknown) => {
        const obj = p as Record<string, unknown>
        return { timestamp: (obj.timestamp ?? obj.time ?? 0) as number, value: (obj.value ?? obj.v ?? 0) as number }
      })
      const aggregated = aggregateData(timePoints, effectiveAggregate)
      if (aggregated !== null) return aggregated
    }

    let rawValue: unknown = data
    if (Array.isArray(data) && data.length > 0) {
      rawValue = data[0]
    }

    if (typeof rawValue === 'object' && rawValue !== null) {
      const obj = rawValue as Record<string, unknown>
      const extractedValue = obj.value ?? obj.v ?? obj.avg ?? obj.min ?? obj.max ?? obj.result
      if (extractedValue !== undefined) {
        rawValue = extractedValue
      }
    }

    if (typeof rawValue === 'number') return rawValue
    if (typeof rawValue === 'string') {
      const parsed = parseFloat(rawValue)
      if (!isNaN(parsed)) return parsed
    }
    return null
  }, [data, effectiveAggregate])

  // Calculate trend using module-level cache (like useDataSource does)
  const { trendDirection, trendValue, hasValidTrend } = useMemo(() => {
    if (!showTrend) {
      return { trendDirection: null, trendValue: 0, hasValidTrend: false }
    }

    // Get cache key for this dataSource (independent of data reference)
    const cacheKey = getTrendCacheKey(dataSource)
    if (!cacheKey) {
      return { trendDirection: null, trendValue: 0, hasValidTrend: false }
    }

    // Extract value helper (inline for useMemo)
    const extractValue = (item: unknown): number | null => {
      if (typeof item === 'number') return item
      if (typeof item === 'object' && item !== null) {
        const obj = item as Record<string, unknown>
        const val = obj.value ?? obj.v ?? obj.avg ?? obj.min ?? obj.max ?? obj.result
        if (typeof val === 'number') return val
      }
      if (typeof item === 'string') {
        const parsed = parseFloat(item)
        if (!isNaN(parsed)) return parsed
      }
      return null
    }

    // Extract values from data array
    let currentVal: number | null = null
    let previousVal: number | null = null

    if (Array.isArray(data) && data.length >= 1) {
      // Data is sorted ascending (oldest first), so last element is latest
      currentVal = extractValue(data[data.length - 1])

      // Find a different value for comparison (skip identical adjacent values)
      if (data.length >= 2) {
        for (let i = data.length - 2; i >= 0; i--) {
          const val = extractValue(data[i])
          if (val !== null && val !== currentVal) {
            previousVal = val
            break
          }
        }
      }

      // If all values are the same, use the first element as previous
      if (previousVal === null && data.length >= 2) {
        previousVal = extractValue(data[0])
      }
    }

    // Create data hash from current+previous values for cache
    const dataHash = (currentVal !== null && previousVal !== null)
      ? `${currentVal}_${previousVal}`
      : ''

    // First, try to get cached trend with matching dataHash
    const cached = trendCache.getCached(cacheKey, dataHash)
    if (cached) {
      return {
        trendDirection: cached.direction,
        trendValue: cached.value,
        hasValidTrend: cached.direction !== null
      }
    }

    // If we have valid data, calculate and cache new trend
    if (currentVal !== null && previousVal !== null && previousVal !== 0) {
      const percentChange = ((currentVal - previousVal) / Math.abs(previousVal)) * 100
      const direction = percentChange > 0 ? 'up' : percentChange < 0 ? 'down' : 'neutral'
      const value = Math.round(percentChange * 10) / 10

      // Store in cache
      trendCache.setCached(cacheKey, dataHash, direction, value)

      return { trendDirection: direction, trendValue: value, hasValidTrend: true }
    }

    // Data is insufficient right now, try to return last cached trend for this dataSource
    const lastCached = trendCache.getLastCached(cacheKey)
    if (lastCached && lastCached.direction !== null) {
      return {
        trendDirection: lastCached.direction,
        trendValue: lastCached.value,
        hasValidTrend: true
      }
    }

    // No valid data and no cache
    return { trendDirection: null, trendValue: 0, hasValidTrend: false }
  }, [showTrend, data, dataSource])

  // Format the value with unit and prefix - uses raw data
  // For arrays, use the first value (API returns data DESCENDING, so first is latest)
  // For objects, extract the 'value' property (handles both {value} and {time, value} formats)
  const formattedValue = useMemo(() => {
    if (error || data === null || data === undefined || (Array.isArray(data) && data.length === 0)) {
      return '-'
    }

    // If data is an array, get the last value (latest)
    // Pipeline sorts telemetry data ascending (oldest first), so last element is newest
    let rawValue: unknown = data
    if (Array.isArray(data) && data.length > 0) {
      rawValue = data[data.length - 1]
    }

    // If rawValue is an object, extract the value from various possible formats
    // Handles: { value: ... }, { time, value }, { v: ... }, telemetry point objects
    if (typeof rawValue === 'object' && rawValue !== null) {
      // Try common value property names
      const obj = rawValue as Record<string, unknown>
      const extractedValue = obj.value ?? obj.v ?? obj.avg ?? obj.min ?? obj.max ?? obj.result
      if (extractedValue !== undefined) {
        rawValue = extractedValue
      }
    }

    // Handle null/undefined after extraction
    if (rawValue === null || rawValue === undefined) {
      return '-'
    }

    // Convert to string and add prefix/unit
    // Raw floats render compactly — an unformatted ratio like
    // 0.0596160888671875 reads as noise on a metric card. Tiny magnitudes
    // keep two significant digits instead of collapsing to "0"; exact
    // integers and short decimals pass through unchanged.
    let valueStr = String(rawValue)
    if (typeof rawValue === 'number' && Number.isFinite(rawValue)) {
      const abs = Math.abs(rawValue)
      valueStr = String(
        abs !== 0 && abs < 0.01 ? Number(rawValue.toPrecision(2)) : Math.round(rawValue * 100) / 100,
      )
    }
    const prefixStr = prefix || ''
    const unitStr = unit ? ` ${unit}` : ''

    return `${prefixStr}${valueStr}${unitStr}`
  }, [data, error, prefix, unit])

  // Get size config with fallback - only 'sm', 'md', 'lg' are valid
  const safeSize: ValueCardSize = (size === 'sm' || size === 'md' || size === 'lg') ? size : 'md'
  const sizeConfig = valueCardSize[safeSize]

  // Color styling for value text - use prop or fall back to trend colors
  const finalValueColor = valueColor || (
    trendDirection === 'up' ? indicatorColors.success.text :
    trendDirection === 'down' ? indicatorColors.error.text :
    undefined
  )

  // Unified error state for all variants
  if (error && hasDataSource) {
    return <ErrorState size={safeSize} className={className} />
  }

  if (showLoading) {
    return <LoadingState size={safeSize} className={className} />
  }

  // ============================================================================
  // Minimal variant - just value with optional label
  // ============================================================================

  if (variant === 'minimal') {
    return (
      <div className={cn(dashboardCardBase, 'flex flex-col justify-center', sizeConfig.padding, className)}>
        {title && (
          <span className={cn(indicatorFontWeight.title, 'text-muted-foreground mb-1', sizeConfig.labelText)}>{title}</span>
        )}
        <span className={cn(indicatorFontWeight.value, 'text-foreground tracking-tight tabular-nums', sizeConfig.valueText)} style={{ color: finalValueColor }}>
          {formattedValue}
        </span>
        {showTrend && hasValidTrend && trendDirection && (
          <div className={cn(
            'flex items-center gap-1 mt-1',
            trendDirection === 'up' && indicatorColors.success.text,
            trendDirection === 'down' && indicatorColors.error.text,
            trendDirection === 'neutral' && indicatorColors.neutral.text
          )}>
            {trendDirection === 'up' && <TrendingUp className={cn('h-4 w-4')} />}
            {trendDirection === 'down' && <TrendingDown className={cn('h-4 w-4')} />}
            <span className={cn(indicatorFontWeight.meta, 'text-xs', sizeConfig.labelText)}>
              {Math.abs(trendValue ?? 0)}%
            </span>
          </div>
        )}
      </div>
    )
  }

  // ============================================================================
  // Vertical variant
  // ============================================================================

  if (variant === 'vertical') {
    return (
      <div className={cn(
        dashboardCardBase,
        'flex-col items-center justify-center',
        sizeConfig.padding,
        className
      )}>
        {/* Icon */}
        {icon && (
          <div className={cn('mb-3', sizeConfig.contentGap)}>
            <ValueIcon icon={icon} title={title} iconType={iconType} size={safeSize} iconColor={iconColor} />
          </div>
        )}

        {/* Value */}
        <span className={cn(indicatorFontWeight.value, 'text-foreground tracking-tight tabular-nums text-center', sizeConfig.valueText)} style={{ color: finalValueColor }}>
          {formattedValue}
        </span>

        {/* Label */}
        {title && (
          <span className={cn(indicatorFontWeight.title, 'text-muted-foreground text-center max-w-full truncate mt-1', sizeConfig.labelText)}>
            {title}
          </span>
        )}

        {/* Trend */}
        {showTrend && hasValidTrend && trendDirection && (
          <div className={cn(
            'flex items-center gap-1 mt-2 px-2 py-1 rounded-full',
            trendDirection === 'up' && cn(indicatorColors.success.bg, indicatorColors.success.text),
            trendDirection === 'down' && cn(indicatorColors.error.bg, indicatorColors.error.text),
            trendDirection === 'neutral' && cn(indicatorColors.neutral.bg, indicatorColors.neutral.text)
          )}>
            {trendDirection === 'up' && <ArrowUpRight className="h-4 w-4" />}
            {trendDirection === 'down' && <ArrowDownRight className="h-4 w-4" />}
            {trendDirection === 'neutral' && <Minus className="h-4 w-4" />}
            <span className={cn(indicatorFontWeight.meta, 'text-xs', sizeConfig.labelText)}>
              {Math.abs(trendValue ?? 0)}%
            </span>
          </div>
        )}
      </div>
    )
  }

  // ============================================================================
  // Compact variant - icon + value in single row
  // ============================================================================

  if (variant === 'compact') {
    return (
      <div className={cn(dashboardCardBase, 'flex-row items-center gap-3', sizeConfig.padding, className)}>
        {/* Icon */}
        {icon && <ValueIcon icon={icon} title={title} iconType={iconType} size={safeSize} iconColor={iconColor} />}

        {/* Content */}
        <div className="flex flex-col min-w-0 flex-1">
          {title && (
            <span className={cn(indicatorFontWeight.title, 'text-muted-foreground truncate', sizeConfig.labelText)}>{title}</span>
          )}
          <div className="flex items-baseline gap-1">
            <span className={cn(indicatorFontWeight.value, 'text-foreground tabular-nums', sizeConfig.valueText)} style={{ color: finalValueColor }}>
              {formattedValue}
            </span>
          </div>
        </div>

        {/* Trend indicator */}
        {showTrend && hasValidTrend && trendDirection && (
          <div className={cn(
            'flex items-center gap-1 px-2 py-1 rounded-full shrink-0',
            trendDirection === 'up' && cn(indicatorColors.success.bg, indicatorColors.success.text),
            trendDirection === 'down' && cn(indicatorColors.error.bg, indicatorColors.error.text),
          )}>
            {trendDirection === 'up' && <ArrowUpRight className="h-4 w-4" />}
            {trendDirection === 'down' && <ArrowDownRight className="h-4 w-4" />}
            <span className={cn(indicatorFontWeight.meta, 'text-xs tabular-nums')}>{Math.abs(trendValue ?? 0)}%</span>
          </div>
        )}
      </div>
    )
  }

  // ============================================================================
  // Default variant - horizontal with icon section (LED-style layout)
  // ============================================================================

  return (
    <div className={cn(dashboardCardHorizontal, sizeConfig.padding, className)}>
      {/* Content wrapper with fixed left margin */}
      <div className="flex items-center ml-2.5">
        {/* Icon section - fixed size */}
        <div className={cn('flex items-center justify-center shrink-0', sizeConfig.iconContainer)}>
          <ValueIcon icon={icon} title={title} iconType={iconType} size={safeSize} iconColor={iconColor} />
        </div>

        {/* Content section - left-aligned like LEDIndicator */}
        <div className="flex flex-col min-w-0 flex-1 ml-2.5">
          {/* Title - primary text */}
          {title && (
            <span className={cn(indicatorFontWeight.title, 'text-foreground truncate', sizeConfig.titleText)}>
              {title}
            </span>
          )}

          {/* Value - secondary text */}
          <span className={cn(indicatorFontWeight.value, 'tabular-nums', sizeConfig.labelText)} style={{ color: finalValueColor }}>
            {formattedValue}
          </span>
        </div>

        {/* Optional trend indicator on the right */}
        {showTrend && hasValidTrend && trendDirection && (
          <div className={cn(
            'flex items-center gap-1 px-2 py-0.5 rounded-full text-xs font-medium shrink-0',
            trendDirection === 'up' && cn(indicatorColors.success.bg, indicatorColors.success.text),
            trendDirection === 'down' && cn(indicatorColors.error.bg, indicatorColors.error.text),
            trendDirection === 'neutral' && cn(indicatorColors.neutral.bg, indicatorColors.neutral.text)
          )}>
            {trendDirection === 'up' && <ArrowUpRight className="h-4 w-4" />}
            {trendDirection === 'down' && <ArrowDownRight className="h-4 w-4" />}
            {trendDirection === 'neutral' && <Minus className="h-4 w-4" />}
            <span>{Math.abs(trendValue ?? 0)}%</span>
          </div>
        )}
      </div>
    </div>
  )
})