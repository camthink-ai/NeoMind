/**
 * Value Card Component (Unified Styles)
 *
 * Fills 100% of container using unified dashboard styles.
 * Size prop controls relative scale, not fixed dimensions.
 */

import { ArrowUpRight, ArrowDownRight, Minus, Activity, TrendingUp, TrendingDown } from 'lucide-react'
import { Skeleton } from '@/components/ui/skeleton'
import { cn, formatValue, getIconForEntity } from '@/lib/utils'
import { useDataSource } from '@/hooks/useDataSource'
import { chartColors } from '@/design-system'
import { dashboardComponentSize, dashboardCardBase } from '@/design-system/tokens/size'
import type { DataSourceOrList } from '@/types/dashboard'

export interface ValueCardProps {
  dataSource?: DataSourceOrList

  // Display
  label?: string
  unit?: string
  prefix?: string
  icon?: string
  iconType?: 'auto' | 'entity' | 'emoji'
  description?: string

  // Trend
  showTrend?: boolean
  trendValue?: number
  trendPeriod?: string

  // Sparkline
  showSparkline?: boolean
  sparklineData?: number[]

  // Styling - controls relative scale, not fixed size
  size?: 'sm' | 'md' | 'lg'
  variant?: 'default' | 'vertical' | 'compact' | 'minimal'
  color?: string

  className?: string
}

// ============================================================================
// Sparkline Renderer
// ============================================================================

interface SparklineProps {
  data: number[]
  color?: string
  trendDirection?: 'up' | 'down' | 'neutral' | null
}

function Sparkline({ data, color, trendDirection }: SparklineProps) {
  const validData = data.filter((v): v is number => typeof v === 'number' && !isNaN(v))
  if (validData.length < 2) return null

  const min = Math.min(...validData)
  const max = Math.max(...validData)
  const range = max - min || 1

  // Use 100% width/height, responsive via svg viewBox
  const points = validData.map((v, i) => {
    const x = (i / (validData.length - 1)) * 100
    const y = 100 - ((v - min) / range) * 100
    return `${x},${y}`
  }).join(' ')

  const strokeColor = color || (
    trendDirection === 'up'
      ? chartColors[2]
      : trendDirection === 'down'
        ? chartColors[4]
        : 'hsl(var(--muted-foreground) / 0.5)'
  )

  // Fill gradient
  const fillPoints = `${points} 100,0 0,0`

  return (
    <svg viewBox="0 0 100 25" className="w-full h-auto opacity-80" preserveAspectRatio="none">
      <defs>
        <linearGradient id={`gradient-${strokeColor}`} x1="0%" y1="0%" x2="0%" y2="100%">
          <stop offset="0%" stopColor={strokeColor} stopOpacity="0.2" />
          <stop offset="100%" stopColor={strokeColor} stopOpacity="0" />
        </linearGradient>
      </defs>
      <polygon
        points={fillPoints}
        fill={`url(#gradient-${strokeColor})`}
      />
      <polyline
        points={points}
        fill="none"
        stroke={strokeColor}
        strokeWidth="2"
        vectorEffect="non-scaling-stroke"
        strokeLinecap="round"
        strokeLinejoin="round"
      />
    </svg>
  )
}

// ============================================================================
// Icon Renderer (Responsive)
// ============================================================================

interface ValueIconProps {
  icon?: string
  label?: string
  iconType?: 'auto' | 'entity' | 'emoji'
  size: 'sm' | 'md' | 'lg'
  className?: string
}

function ValueIcon({ icon, label, iconType = 'entity', size, className }: ValueIconProps) {
  const config = dashboardComponentSize[size]

  // Emoji fallback
  if (icon && iconType === 'emoji') {
    return <span className={cn('opacity-80', size === 'sm' ? 'text-lg' : size === 'md' ? 'text-xl' : 'text-2xl', className)}>{icon}</span>
  }

  // Get SVG icon
  const getIcon = () => {
    if (!icon && label) return getIconForEntity(label)
    if (icon) return getIconForEntity(icon)
    return Activity
  }

  const IconComponent = getIcon()

  return (
    <div className={cn(
      'flex items-center justify-center rounded-lg',
      'bg-primary/10 text-primary',
      config.iconContainer,
      className
    )}>
      <IconComponent className={cn(config.iconSize)} />
    </div>
  )
}

// ============================================================================
// Main Component
// ============================================================================

export function ValueCard({
  dataSource,
  label,
  unit,
  prefix = '',
  icon,
  iconType = 'entity',
  description,
  showTrend = false,
  trendValue,
  trendPeriod = 'last hour',
  showSparkline = false,
  sparklineData,
  size = 'md',
  variant = 'default',
  color,
  className,
}: ValueCardProps) {
  const { data, loading, error } = useDataSource<number | string | null>(dataSource, {
    fallback: null,
  })

  const displayValue = error ? null : data
  const trendDirection = trendValue !== undefined
    ? trendValue > 0 ? 'up' : trendValue < 0 ? 'down' : 'neutral'
    : null

  const sizeConfig = dashboardComponentSize[size]

  // Color styling
  const getValueColor = () => {
    if (color) return color
    if (trendDirection === 'up') return 'text-emerald-600 dark:text-emerald-500'
    if (trendDirection === 'down') return 'text-rose-600 dark:text-rose-500'
    return undefined
  }

  const valueColor = getValueColor()

  // ============================================================================
  // Minimal variant - just value with optional label
  // ============================================================================

  if (variant === 'minimal') {
    return (
      <div className={cn(dashboardCardBase, 'flex flex-col justify-center', sizeConfig.padding, className)}>
        {label && (
          <span className={cn('text-muted-foreground mb-1', sizeConfig.labelText)}>{label}</span>
        )}
        {loading ? (
          <Skeleton className={cn('h-6 w-20 rounded')} />
        ) : (
          <span className={cn('font-bold text-foreground tracking-tight tabular-nums', sizeConfig.valueText, valueColor)}>
            {formatValue(displayValue ?? '-', { unit, prefix, locale: 'en' })}
          </span>
        )}
        {showTrend && trendDirection && (
          <div className={cn(
            'flex items-center gap-1 mt-1',
            trendDirection === 'up' && 'text-emerald-600 dark:text-emerald-500',
            trendDirection === 'down' && 'text-rose-600 dark:text-rose-500',
            trendDirection === 'neutral' && 'text-muted-foreground'
          )}>
            {trendDirection === 'up' && <TrendingUp className={cn('h-3 w-3')} />}
            {trendDirection === 'down' && <TrendingDown className={cn('h-3 w-3')} />}
            <span className={cn('text-xs font-medium', sizeConfig.labelText)}>
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
            <ValueIcon icon={icon} label={label} iconType={iconType} size={size} />
          </div>
        )}

        {/* Value */}
        {loading ? (
          <Skeleton className={cn('h-7 w-16 rounded')} />
        ) : (
          <span className={cn('font-bold text-foreground/90 tracking-tight tabular-nums text-center', sizeConfig.valueText, valueColor)}>
            {formatValue(displayValue ?? '-', { unit, prefix, locale: 'en' })}
          </span>
        )}

        {/* Label */}
        {label && (
          <span className={cn('text-muted-foreground text-center max-w-full truncate mt-1', sizeConfig.labelText)}>
            {label}
          </span>
        )}

        {/* Sparkline */}
        {showSparkline && sparklineData && sparklineData.length >= 2 && (
          <div className="w-full mt-3">
            <Sparkline data={sparklineData} trendDirection={trendDirection} />
          </div>
        )}

        {/* Trend */}
        {showTrend && trendDirection && (
          <div className={cn(
            'flex items-center gap-1 mt-2 px-2 py-1 rounded-full',
            trendDirection === 'up' && 'bg-emerald-500/10 text-emerald-600 dark:text-emerald-400',
            trendDirection === 'down' && 'bg-rose-500/10 text-rose-600 dark:text-rose-400',
            trendDirection === 'neutral' && 'bg-muted text-muted-foreground'
          )}>
            {trendDirection === 'up' && <ArrowUpRight className="h-3 w-3" />}
            {trendDirection === 'down' && <ArrowDownRight className="h-3 w-3" />}
            {trendDirection === 'neutral' && <Minus className="h-3 w-3" />}
            <span className={cn('text-xs font-medium', sizeConfig.labelText)}>
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
        {icon && <ValueIcon icon={icon} label={label} iconType={iconType} size={size} />}

        {/* Content */}
        <div className="flex flex-col min-w-0 flex-1">
          {label && (
            <span className={cn('text-muted-foreground truncate', sizeConfig.labelText)}>{label}</span>
          )}
          <div className="flex items-baseline gap-1">
            {loading ? (
              <Skeleton className={cn('h-5 w-16 rounded')} />
            ) : (
              <span className={cn('font-semibold text-foreground tabular-nums', sizeConfig.valueText, valueColor)}>
                {formatValue(displayValue ?? '-', { unit, prefix, locale: 'en' })}
              </span>
            )}
          </div>
        </div>

        {/* Trend indicator */}
        {showTrend && trendDirection && (
          <div className={cn(
            'flex items-center gap-1 px-2 py-1 rounded-full shrink-0',
            trendDirection === 'up' && 'bg-emerald-500/10 text-emerald-600',
            trendDirection === 'down' && 'bg-rose-500/10 text-rose-600',
          )}>
            {trendDirection === 'up' && <ArrowUpRight className="h-3 w-3" />}
            {trendDirection === 'down' && <ArrowDownRight className="h-3 w-3" />}
            <span className={cn('text-xs font-semibold tabular-nums')}>{Math.abs(trendValue ?? 0)}%</span>
          </div>
        )}
      </div>
    )
  }

  // ============================================================================
  // Default variant - horizontal with icon section (LED-style layout)
  // ============================================================================

  return (
    <div className={cn(dashboardCardBase, 'flex-row items-center', sizeConfig.contentGap, sizeConfig.padding, className)}>
      {/* Icon section */}
      <div className={cn('flex items-center justify-center shrink-0', sizeConfig.iconContainer)}>
        <ValueIcon icon={icon} label={label} iconType={iconType} size={size} />
      </div>

      {/* Content section - left-aligned like LEDIndicator */}
      <div className="flex flex-col min-w-0 flex-1 overflow-hidden">
        {/* Label - primary text */}
        {label && (
          <span className={cn('font-medium text-foreground truncate', sizeConfig.titleText)}>
            {label}
          </span>
        )}

        {/* Value - secondary text */}
        {loading ? (
          <Skeleton className={cn('h-5 w-16 rounded mt-0.5')} />
        ) : (
          <span className={cn('font-semibold tabular-nums', sizeConfig.labelText, valueColor)}>
            {formatValue(displayValue ?? '-', { unit, prefix, locale: 'en' })}
          </span>
        )}
      </div>

      {/* Optional trend indicator on the right */}
      {showTrend && trendDirection && (
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
  )
}
