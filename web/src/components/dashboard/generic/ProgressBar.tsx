/**
 * Progress Bar Component (Enhanced)
 *
 * Features:
 * - Unified color system with OKLCH colors
 * - Gradient fills for decorative progress bar
 * - Glow effects for active states
 * - Multiple variants (default, compact, circular)
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
  variant?: 'default' | 'compact' | 'circular'
  className?: string
}

function safeToNumber(value: unknown): number {
  if (typeof value === 'number') return value
  if (typeof value === 'string') {
    const num = parseFloat(value)
    return isNaN(num) ? 0 : num
  }
  if (typeof value === 'boolean') return value ? 1 : 0
  return 0
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
  className,
}: ProgressBarProps) {
  // Check if dataSource is configured
  const hasDataSource = dataSource !== undefined

  // Fetch data - may be telemetry array or single value
  const { data, loading, error } = useDataSource<unknown>(dataSource, {
    fallback: propValue ?? 0,
  })

  // Extract the latest value from telemetry data or use the data directly
  const rawValue = error ? propValue : toLatestValue(data, propValue ?? 0)
  const value = safeToNumber(typeof rawValue === 'number' ? rawValue : 0)
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
  // Compact variant - just the progress bar
  // ============================================================================

  if (variant === 'compact') {
    return (
      <div className={cn('w-full h-full flex items-center', className)}>
        {loading ? (
          <Skeleton className={cn('w-full h-full rounded-full', barHeight)} />
        ) : (
          <div className={cn('relative w-full h-full rounded-full bg-muted/40 overflow-hidden', barHeight)}>
            <div
              className={cn('h-full rounded-full transition-all duration-500 ease-out')}
              style={{
                width: `${percentage}%`,
                background: progressGradient,
              }}
            />
          </div>
        )}
      </div>
    )
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

    return (
      <div className={cn(dashboardCardBase, 'flex flex-col items-center justify-center', sizeConfig.padding, className)}>
        {loading ? (
          <Skeleton className={cn('rounded-full', size === 'sm' ? 'h-16 w-16' : size === 'md' ? 'h-20 w-20' : 'h-24 w-24')} />
        ) : (
          <div className="relative">
            <svg className={cn('transform -rotate-90', size === 'sm' ? 'h-16 w-16' : size === 'md' ? 'h-20 w-20' : 'h-24 w-24')} viewBox={`0 0 ${radius * 2} ${radius * 2}`}>
              <defs>
                <linearGradient id={gradientId} x1="0%" y1="0%" x2="100%" y2="0%">
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
        )}
        {title && (
          <span className={cn(indicatorFontWeight.label, colorConfig.text, 'mt-2', sizeConfig.labelText)}>
            {title}
          </span>
        )}
      </div>
    )
  }

  // ============================================================================
  // Default variant - Sparkline-style card layout with gradient
  // ============================================================================

  const content = (
    <div className="flex flex-col w-full min-h-0">
      {/* Header: label (left) + percentage (right) */}
      <div className="flex items-center justify-between mb-1">
        {title && (
          <span className={cn(indicatorFontWeight.title, 'text-muted-foreground truncate text-xs', sizeConfig.labelText)} title={title}>
            {title}
          </span>
        )}
        {!title && <span />}
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
