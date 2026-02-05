/**
 * Progress Bar Component (Enhanced)
 *
 * Features:
 * - Unified color system with OKLCH colors
 * - Gradient fills for decorative progress bar
 * - Glow effects for active states
 * - Multiple variants (default, icon, circular)
 * - Icon variant with customizable icon, colors, and circular progress
 * - Telemetry data support with DataMapper integration
 */

import { useMemo } from 'react'
import { Skeleton } from '@/components/ui/skeleton'
import { cn } from '@/lib/utils'
import { DataMapper } from '@/lib/dataMapping'
import { useDataSource } from '@/hooks/useDataSource'
import { dashboardComponentSize, dashboardCardBase } from '@/design-system/tokens/size'
import {
  indicatorFontWeight,
  indicatorColors,
  getValueStateColor,
  getValueTextColor,
  getGradientStops,
  getLinearGradient,
  type IndicatorState,
} from '@/design-system/tokens/indicator'
import type { DataSourceOrList } from '@/types/dashboard'
import { EmptyState, ErrorState } from '../shared'
import type { SingleValueMappingConfig } from '@/lib/dataMapping'
import { IconDisplay } from '@/components/ui/icon-picker'

export interface ProgressBarProps {
  dataSource?: DataSourceOrList
  value?: number
  max?: number
  title?: string
  size?: 'sm' | 'md' | 'lg'
  color?: string
  warningThreshold?: number
  dangerThreshold?: number
  dataMapping?: SingleValueMappingConfig
  showCard?: boolean
  variant?: 'default' | 'icon' | 'circular'
  // Icon variant props
  icon?: string
  iconColor?: string
  backgroundColor?: string
  className?: string
}

// Map percentage to indicator state
function getProgressState(percentage: number, warningThreshold: number, dangerThreshold: number): IndicatorState {
  if (percentage >= dangerThreshold) return 'error'
  if (percentage >= warningThreshold) return 'warning'
  return 'success'
}

export function ProgressBar({
  dataSource,
  value: propValue,
  max = 100,
  title,
  size = 'md',
  color,
  warningThreshold = 70,
  dangerThreshold = 90,
  dataMapping,
  showCard = true,
  variant = 'default',
  icon,
  iconColor,
  backgroundColor,
  className,
}: ProgressBarProps) {
  // Check if dataSource is configured
  const hasDataSource = dataSource !== undefined

  // Fetch data - may be telemetry array or single value
  const { data, loading, error } = useDataSource<unknown>(dataSource, {
    fallback: propValue ?? 0,
  })

  // Extract value using DataMapper for proper data handling
  const value = useMemo(() => {
    // If there's an error, fall back to prop value
    if (error) return propValue ?? 0

    // If no data source, use prop value
    if (!hasDataSource) return propValue ?? 0

    // Use DataMapper to extract numeric value from data
    const extractedValue = DataMapper.extractValue(data, dataMapping)
    return extractedValue
  }, [data, error, hasDataSource, propValue, dataMapping])

  const percentage = Math.min(100, Math.max(0, (value / max) * 100))

  const sizeConfig = dashboardComponentSize[size]
  const barHeight = size === 'sm' ? 'h-1.5' : size === 'md' ? 'h-2' : 'h-2.5'

  // Derive thresholds from dataMapping if not explicitly provided
  // dataMapping.thresholds.warning corresponds to warningThreshold
  // dataMapping.thresholds.error corresponds to dangerThreshold
  const effectiveWarningThreshold = warningThreshold ?? dataMapping?.thresholds?.warning?.value ?? 70
  const effectiveDangerThreshold = dangerThreshold ?? dataMapping?.thresholds?.error?.value ?? 90

  // Use unified color and gradient system
  const state = getProgressState(percentage, effectiveWarningThreshold, effectiveDangerThreshold)
  const progressColor = getValueStateColor(value, max, effectiveWarningThreshold, effectiveDangerThreshold, color)
  const textColor = getValueTextColor(value, max, effectiveWarningThreshold, effectiveDangerThreshold)
  const colorConfig = indicatorColors[state]

  // Get gradient for the progress fill
  const progressGradient = getLinearGradient(state, 'to right', color)

  // Unified error state for all variants (only when dataSource is configured)
  if (error && hasDataSource) {
    return <ErrorState size={size} className={className} />
  }

  // Unified empty state for all variants (only when dataSource is configured but no value)
  if (!loading && !error && hasDataSource && (data === null || data === undefined)) {
    return <EmptyState size={size} className={className} message={title ? `${title} - No Data Available` : undefined} />
  }

  // ============================================================================
  // Icon variant - icon with fill effect based on percentage
  // ============================================================================

  if (variant === 'icon') {
    const uniqueId = Math.random().toString(36).substring(2, 9)

    // Get fill color based on state
    const fillColor = iconColor || progressColor
    const emptyColor = 'hsl(var(--muted-foreground) / 0.3)'

    // Active state for glow effect
    const isActive = state !== 'neutral'

    // Glow effect based on state
    const getGlowStyle = (): string | undefined => {
      if (!isActive) return undefined
      if (percentage >= effectiveDangerThreshold) return `0 0 12px ${fillColor}`
      if (percentage >= effectiveWarningThreshold) return `0 0 8px ${fillColor}aa`
      return undefined
    }

    const glowStyle = getGlowStyle()

    // Icon size relative to container - based on size prop
    const iconSizeClass = size === 'sm' ? 'w-12 h-12' : size === 'md' ? 'w-16 h-16' : 'w-20 h-20'

    const content = (
      <div className="relative flex items-center justify-center w-full h-full">
        {/* Title at top-left (absolute positioned) */}
        {title && (
          <div className={cn('absolute top-0 left-0', indicatorFontWeight.title, sizeConfig.labelText)}>{title}</div>
        )}

        {/* Centered icon with percentage below */}
        <div className="flex flex-col items-center justify-center gap-2">
          {/* Icon with fill effect */}
          <div className="relative shrink-0 flex items-center justify-center">
            <svg viewBox="0 0 24 24" className={cn(iconSizeClass)}>
              <defs>
                {/* Mask for fill effect - white fills from bottom based on percentage */}
                <mask id={`fill-mask-${uniqueId}`}>
                  <rect x="0" y="0" width="24" height="24" fill="black" />
                  <rect
                    x="0"
                    y={24 - (24 * percentage / 100)}
                    width="24"
                    height={24 * percentage / 100}
                    fill="white"
                  />
                </mask>

                {/* Gradient for the filled portion */}
                <linearGradient id={`fill-gradient-${uniqueId}`} x1="0%" y1="0%" x2="100%" y2="100%">
                  {getGradientStops(state, color).map((stop, i) => (
                    <stop key={i} offset={stop.offset} stopColor={stop.color} stopOpacity={stop.opacity} />
                  ))}
                </linearGradient>
              </defs>

              {/* Background icon (empty state - always gray) */}
              <g style={{ color: emptyColor }}>
                <foreignObject x="2" y="2" width="20" height="20">
                  <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'center', width: '100%', height: '100%' }}>
                    <IconDisplay name={icon || 'Circle'} className="w-5 h-5" />
                  </div>
                </foreignObject>
              </g>

              {/* Foreground icon (filled state - masked by percentage) */}
              <g mask={`url(#fill-mask-${uniqueId})`} style={{ color: fillColor }}>
                <foreignObject x="2" y="2" width="20" height="20">
                  <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'center', width: '100%', height: '100%' }}>
                    <IconDisplay name={icon || 'Circle'} className="w-5 h-5" />
                  </div>
                </foreignObject>
              </g>
            </svg>

            {/* Glow effect */}
            {glowStyle && (
              <div
                className="absolute inset-0 rounded-full pointer-events-none"
                style={{ boxShadow: glowStyle }}
              />
            )}
          </div>

          {/* Percentage text below icon */}
          {!loading && (
            <span className={cn(
              indicatorFontWeight.value,
              'tabular-nums text-xs',
              sizeConfig.valueText
            )} style={{ color: progressColor }}>
              {Math.round(percentage)}%
            </span>
          )}
        </div>
      </div>
    )

    if (showCard) {
      return (
        <div className={cn(dashboardCardBase, sizeConfig.padding, className)}>
          {content}
        </div>
      )
    }

    return <div className={cn('w-full', sizeConfig.padding, className)}>{content}</div>
  }

  // ============================================================================
  // Circular variant with glow effect
  // ============================================================================

  if (variant === 'circular') {
    const radius = size === 'sm' ? 28 : size === 'md' ? 32 : 36
    const strokeWidth = size === 'sm' ? 3 : size === 'md' ? 3.5 : 4
    const circumference = 2 * Math.PI * (radius - strokeWidth / 2)
    const offset = circumference - (percentage / 100) * circumference
    const gradientId = `circular-gradient-${Math.random().toString(36).substring(2, 9)}`

    // SVG size relative to container - based on size prop
    const svgSizeClass = size === 'sm' ? 'w-16 h-16' : size === 'md' ? 'w-20 w-20' : 'w-24 h-24'

    const content = (
      <div className="relative flex items-center justify-center w-full h-full">
        {/* Title at top-left (absolute positioned) */}
        {title && (
          <div className={cn('absolute top-0 left-0', indicatorFontWeight.title, sizeConfig.labelText)}>{title}</div>
        )}

        {/* Centered circular progress */}
        <div className="relative">
          <svg className={cn('transform -rotate-90', svgSizeClass)} viewBox={`0 0 ${radius * 2} ${radius * 2}`}>
            <defs>
              <linearGradient id={gradientId} x1="0%" y1="0%" x2="100%" y2="100%">
                {getGradientStops(state, color).map((stop, i) => (
                  <stop key={i} offset={stop.offset} stopColor={stop.color} stopOpacity={stop.opacity + 0.5} />
                ))}
              </linearGradient>
              <filter id={`glow-${gradientId}`} x="-50%" y="-50%" width="200%" height="200%">
                <feGaussianBlur stdDeviation="2" result="coloredBlur" />
                <feMerge>
                  <feMergeNode in="coloredBlur" />
                  <feMergeNode in="SourceGraphic" />
                </feMerge>
              </filter>
            </defs>
            {/* Background track */}
            <circle
              cx={radius}
              cy={radius}
              r={radius - strokeWidth / 2}
              fill="none"
              stroke="hsl(var(--muted) / 0.3)"
              strokeWidth={strokeWidth}
            />
            {/* Progress with gradient and glow */}
            <circle
              cx={radius}
              cy={radius}
              r={radius - strokeWidth / 2}
              fill="none"
              stroke={`url(#${gradientId})`}
              strokeWidth={strokeWidth}
              strokeDasharray={circumference}
              strokeDashoffset={offset}
              strokeLinecap="round"
              className="transition-all duration-500 ease-out"
              filter={`url(#glow-${gradientId})`}
            />
          </svg>
          <div className="absolute inset-0 flex items-center justify-center">
            <span className={cn(indicatorFontWeight.value, 'text-foreground tabular-nums', sizeConfig.valueText)}>
              {Math.round(percentage)}%
            </span>
          </div>
        </div>
      </div>
    )

    if (showCard) {
      return (
        <div className={cn(dashboardCardBase, sizeConfig.padding, className)}>
          {content}
        </div>
      )
    }

    return <div className={cn('w-full', sizeConfig.padding, className)}>{content}</div>
  }

  // ============================================================================
  // Default variant - Sparkline-style card layout with gradient
  // ============================================================================

  const content = (
    <div className="flex flex-col w-full min-h-0">
      {/* Header: label (left) + percentage (right) */}
      <div className="flex items-center justify-between mb-1">
        {title && (
          <span className={cn(indicatorFontWeight.title, 'text-foreground truncate text-xs', sizeConfig.labelText)} title={title}>
            {title}
          </span>
        )}
        {loading ? (
          <Skeleton className={cn('h-3 w-7 shrink-0 rounded')} />
        ) : (
          <span className={cn(
            indicatorFontWeight.value,
            'tabular-nums text-xs',
            textColor
          )}>
            {Math.round(percentage)}%
          </span>
        )}
      </div>

      {/* Progress bar with gradient fill */}
      <div className="flex-1 min-h-0 flex items-center">
        {loading ? (
          <div className={cn('w-full rounded-full bg-muted/30 overflow-hidden', barHeight)}>
            <Skeleton className={cn('h-full w-full rounded-full', barHeight)} />
          </div>
        ) : (
          <div className={cn('w-full rounded-full bg-muted/30 overflow-hidden relative', barHeight)}>
            <div
              className={cn(
                'h-full rounded-full transition-all duration-500 ease-out',
                percentage >= dangerThreshold && 'animate-pulse'
              )}
              style={{
                width: `${percentage}%`,
                background: progressGradient,
              }}
            />
          </div>
        )}
      </div>
    </div>
  )

  if (showCard) {
    return (
      <div className={cn(dashboardCardBase, 'flex items-center justify-center', sizeConfig.padding, className)}>
        {content}
      </div>
    )
  }

  return <div className={cn('w-full flex items-center justify-center', sizeConfig.padding, className)}>{content}</div>
}
