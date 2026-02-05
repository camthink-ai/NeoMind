/**
 * Value Card Component (Unified Styles)
 *
 * Fills 100% of container using unified dashboard styles.
 * Size prop controls relative scale, not fixed dimensions.
 * Uses raw data values directly.
 */

import { useMemo } from 'react'
import { ArrowUpRight, ArrowDownRight, Minus, Activity, TrendingUp, TrendingDown } from 'lucide-react'
import { Skeleton } from '@/components/ui/skeleton'
import { cn, getIconForEntity } from '@/lib/utils'
import { chartColors, indicatorFontWeight, indicatorColors, dashboardCardBase, dashboardCardHorizontal } from '@/design-system'
import { valueCardSize, type ValueCardSize } from '@/design-system/tokens/size'
import type { DataSourceOrList } from '@/types/dashboard'
import { useDataSource } from '@/hooks/useDataSource'
import { ErrorState } from '../shared'

// ============================================================================
// Module-level cache for trend data (persists across component remounts)
// Similar to telemetryCache in useDataSource.ts
// ============================================================================
interface TrendCacheEntry {
  direction: 'up' | 'down' | 'neutral' | null
  value: number
  timestamp: number
  dataHash: string  // Hash of current+previous values to detect real changes
}

const trendCache = new Map<string, TrendCacheEntry>()
const TREND_CACHE_TTL = 60000 // 60 seconds - longer than telemetry cache
const MAX_TREND_CACHE_SIZE = 100

function getTrendCacheKey(dataSource: DataSourceOrList | undefined): string {
  if (!dataSource) return ''
  if (Array.isArray(dataSource)) return `multi:${dataSource.length}`
  if (typeof dataSource === 'string') return `ref:${dataSource}`
  return JSON.stringify(dataSource)
}

function getCachedTrend(cacheKey: string, dataHash: string): TrendCacheEntry | null {
  if (!cacheKey) return null
  const cached = trendCache.get(cacheKey)
  if (cached && cached.dataHash === dataHash && Date.now() - cached.timestamp < TREND_CACHE_TTL) {
    return cached
  }
  return null
}

function setCachedTrend(cacheKey: string, dataHash: string, direction: 'up' | 'down' | 'neutral' | null, value: number): void {
  if (!cacheKey) return

  // Enforce cache size limit
  if (trendCache.size >= MAX_TREND_CACHE_SIZE) {
    const firstKey = trendCache.keys().next().value
    if (firstKey) trendCache.delete(firstKey)
  }

  trendCache.set(cacheKey, {
    direction,
    value,
    timestamp: Date.now(),
    dataHash
  })
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
        'bg-primary/10 text-primary',
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

export function ValueCard({
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
  const { data, loading, error } = useDataSource<unknown>(dataSource, {
    fallback: null,
  })

  // Check if dataSource is configured
  const hasDataSource = dataSource !== undefined

  // Extract numeric value from data for calculations
  const extractNumericValue = useMemo(() => {
    if (data === null || data === undefined) return null

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
  }, [data])

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
      // Current value is always the first element (latest)
      currentVal = extractValue(data[0])

      // Find a different value for comparison (skip identical adjacent values)
      if (data.length >= 2) {
        for (let i = 1; i < data.length; i++) {
          const val = extractValue(data[i])
          if (val !== null && val !== currentVal) {
            previousVal = val
            break
          }
        }
      }

      // If all values are the same, use the last element as previous
      if (previousVal === null && data.length >= 2) {
        previousVal = extractValue(data[data.length - 1])
      }
    }

    // Create data hash from current+previous values for cache
    const dataHash = (currentVal !== null && previousVal !== null)
      ? `${currentVal}_${previousVal}`
      : ''

    // First, try to get cached trend with matching dataHash
    const cached = getCachedTrend(cacheKey, dataHash)
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

      // Store in module-level cache
      setCachedTrend(cacheKey, dataHash, direction, value)

      return { trendDirection: direction, trendValue: value, hasValidTrend: true }
    }

    // Data is insufficient right now, try to return last cached trend for this dataSource
    const lastCached = trendCache.get(cacheKey)
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
    if (error || data === null || data === undefined) {
      return '-'
    }

    // If data is an array, get the first value (latest)
    // Backend returns telemetry data sorted DESCENDING (newest first)
    let rawValue: unknown = data
    if (Array.isArray(data) && data.length > 0) {
      rawValue = data[0]
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
    const valueStr = String(rawValue)
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

  // ============================================================================
  // Minimal variant - just value with optional label
  // ============================================================================

  if (variant === 'minimal') {
    return (
      <div className={cn(dashboardCardBase, 'flex flex-col justify-center', sizeConfig.padding, className)}>
        {title && (
          <span className={cn(indicatorFontWeight.title, 'text-muted-foreground mb-1', sizeConfig.labelText)}>{title}</span>
        )}
        {loading ? (
          <Skeleton className={cn('h-6 w-20 rounded')} />
        ) : (
          <span className={cn(indicatorFontWeight.value, 'text-foreground tracking-tight tabular-nums', sizeConfig.valueText)} style={{ color: finalValueColor }}>
            {formattedValue}
          </span>
        )}
        {showTrend && hasValidTrend && trendDirection && (
          <div className={cn(
            'flex items-center gap-1 mt-1',
            trendDirection === 'up' && indicatorColors.success.text,
            trendDirection === 'down' && indicatorColors.error.text,
            trendDirection === 'neutral' && indicatorColors.neutral.text
          )}>
            {trendDirection === 'up' && <TrendingUp className={cn('h-3 w-3')} />}
            {trendDirection === 'down' && <TrendingDown className={cn('h-3 w-3')} />}
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
        {loading ? (
          <Skeleton className={cn('h-7 w-16 rounded')} />
        ) : (
          <span className={cn(indicatorFontWeight.value, 'text-foreground/90 tracking-tight tabular-nums text-center', sizeConfig.valueText)} style={{ color: finalValueColor }}>
            {formattedValue}
          </span>
        )}

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
            trendDirection === 'up' && indicatorColors.success.bg + ' ' + indicatorColors.success.text,
            trendDirection === 'down' && indicatorColors.error.bg + ' ' + indicatorColors.error.text,
            trendDirection === 'neutral' && indicatorColors.neutral.bg + ' ' + indicatorColors.neutral.text
          )}>
            {trendDirection === 'up' && <ArrowUpRight className="h-3 w-3" />}
            {trendDirection === 'down' && <ArrowDownRight className="h-3 w-3" />}
            {trendDirection === 'neutral' && <Minus className="h-3 w-3" />}
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
            {loading ? (
              <Skeleton className={cn('h-5 w-16 rounded')} />
            ) : (
              <span className={cn(indicatorFontWeight.value, 'text-foreground tabular-nums', sizeConfig.valueText)} style={{ color: finalValueColor }}>
                {formattedValue}
              </span>
            )}
          </div>
        </div>

        {/* Trend indicator */}
        {showTrend && hasValidTrend && trendDirection && (
          <div className={cn(
            'flex items-center gap-1 px-2 py-1 rounded-full shrink-0',
            trendDirection === 'up' && indicatorColors.success.bg + ' ' + indicatorColors.success.text,
            trendDirection === 'down' && indicatorColors.error.bg + ' ' + indicatorColors.error.text,
          )}>
            {trendDirection === 'up' && <ArrowUpRight className="h-3 w-3" />}
            {trendDirection === 'down' && <ArrowDownRight className="h-3 w-3" />}
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
      <div className="flex items-center" style={{ marginLeft: '0.625rem' /* 10px */ }}>
        {/* Icon section - fixed size */}
        <div className={cn('flex items-center justify-center shrink-0', sizeConfig.iconContainer)}>
          <ValueIcon icon={icon} title={title} iconType={iconType} size={safeSize} iconColor={iconColor} />
        </div>

        {/* Content section - left-aligned like LEDIndicator */}
        <div className="flex flex-col min-w-0 flex-1" style={{ marginLeft: '0.625rem' /* 10px */ }}>
          {/* Title - primary text */}
          {title && (
            <span className={cn(indicatorFontWeight.title, 'text-foreground truncate', sizeConfig.titleText)}>
              {title}
            </span>
          )}

          {/* Value - secondary text */}
          {loading ? (
            <Skeleton className={cn('h-5 w-16 rounded mt-0.5')} />
          ) : (
            <span className={cn(indicatorFontWeight.value, 'tabular-nums', sizeConfig.labelText)} style={{ color: finalValueColor }}>
              {formattedValue}
            </span>
          )}
        </div>

        {/* Optional trend indicator on the right */}
        {showTrend && hasValidTrend && trendDirection && (
          <div className={cn(
            'flex items-center gap-1 px-2 py-0.5 rounded-full text-xs font-medium shrink-0',
            trendDirection === 'up' && 'bg-emerald-500/10 text-emerald-600 dark:text-emerald-400',
            trendDirection === 'down' && 'bg-rose-500/10 text-rose-600 dark:text-rose-400',
            trendDirection === 'neutral' && 'bg-muted text-muted-foreground'
          )}>
            {trendDirection === 'up' && <ArrowUpRight className="h-3 w-3" />}
            {trendDirection === 'down' && <ArrowDownRight className="h-3 w-3" />}
            {trendDirection === 'neutral' && <Minus className="h-3 w-3" />}
            <span>{Math.abs(trendValue ?? 0)}%</span>
          </div>
        )}
      </div>
    </div>
  )
}
