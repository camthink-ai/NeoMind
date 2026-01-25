/**
 * LED Indicator Component
 *
 * State indicator with LED-like visual feedback.
 * Layout matches ValueCard: left LED container + right content.
 */

import { useMemo } from 'react'
import { cn } from '@/lib/utils'
import { useDataSource } from '@/hooks/useDataSource'
import { Skeleton } from '@/components/ui/skeleton'
import { dashboardComponentSize, dashboardCardBase } from '@/design-system/tokens/size'
import {
  indicatorFontWeight,
  indicatorColors,
  getLedGlow,
  getLedAnimation,
  type IndicatorState,
} from '@/design-system/tokens/indicator'
import type { DataSource } from '@/types/dashboard'

export type LEDState = 'on' | 'off' | 'error' | 'warning' | 'unknown'

export interface ValueStateMapping {
  values?: string
  pattern?: string
  state: LEDState
  label?: string
  color?: string
}

export interface LEDIndicatorProps {
  dataSource?: DataSource
  state?: LEDState
  label?: string
  size?: 'sm' | 'md' | 'lg'
  valueMap?: ValueStateMapping[]
  defaultState?: LEDState
  color?: string
  showCard?: boolean
  showGlow?: boolean
  className?: string
}

// State configuration
const stateConfig = {
  on: {
    indicatorState: 'success' as IndicatorState,
    label: '开启',
    color: indicatorColors.success,
  },
  off: {
    indicatorState: 'neutral' as IndicatorState,
    label: '关闭',
    color: indicatorColors.neutral,
  },
  error: {
    indicatorState: 'error' as IndicatorState,
    label: '错误',
    color: indicatorColors.error,
  },
  warning: {
    indicatorState: 'warning' as IndicatorState,
    label: '警告',
    color: indicatorColors.warning,
  },
  unknown: {
    indicatorState: 'neutral' as IndicatorState,
    label: '未知',
    color: indicatorColors.neutral,
  },
}

// LED container sizes
const getLedContainerSize = (size: 'xs' | 'sm' | 'md' | 'lg') => {
  if (size === 'xs') return 'w-6 h-6'
  return size === 'sm' ? 'w-8 h-8' : size === 'md' ? 'w-10 h-10' : 'w-12 h-12'
}

// LED dot sizes
const getLedDotSize = (size: 'xs' | 'sm' | 'md' | 'lg') => {
  if (size === 'xs') return 'h-2 w-2'
  return size === 'sm' ? 'h-3 w-3' : size === 'md' ? 'h-4 w-4' : 'h-5 w-5'
}

// Match value to state
function matchValueToState(
  value: unknown,
  valueMap: ValueStateMapping[],
  defaultState: LEDState
): LEDState {
  if (value === null || value === undefined) {
    return defaultState
  }

  const normalizedValue = String(value).trim().toLowerCase()

  for (const mapping of valueMap) {
    if (mapping.values) {
      const values = mapping.values.toLowerCase().split(',').map(v => v.trim())
      if (values.some(v => v === normalizedValue)) {
        return mapping.state
      }
    }

    if (mapping.pattern) {
      try {
        const regex = new RegExp(mapping.pattern, 'i')
        if (regex.test(normalizedValue)) {
          return mapping.state
        }
      } catch {
        // Skip invalid regex
      }
    }
  }

  return defaultState
}

function getCustomLabel(
  value: unknown,
  valueMap: ValueStateMapping[],
  matchedState: LEDState
): string | undefined {
  if (!valueMap) return undefined

  const normalizedValue = String(value).trim().toLowerCase()

  for (const mapping of valueMap) {
    if (mapping.state === matchedState && mapping.label) {
      if (mapping.values) {
        const values = mapping.values.toLowerCase().split(',').map(v => v.trim())
        if (values.some(v => v === normalizedValue)) {
          return mapping.label
        }
      }
      if (mapping.pattern) {
        try {
          if (new RegExp(mapping.pattern, 'i').test(normalizedValue)) {
            return mapping.label
          }
        } catch {
          // Skip
        }
      }
    }
  }

  return undefined
}

function getCustomColor(
  value: unknown,
  valueMap: ValueStateMapping[],
  matchedState: LEDState
): string | undefined {
  if (!valueMap) return undefined

  const normalizedValue = String(value).trim().toLowerCase()

  for (const mapping of valueMap) {
    if (mapping.state === matchedState && mapping.color) {
      if (mapping.values) {
        const values = mapping.values.toLowerCase().split(',').map(v => v.trim())
        if (values.some(v => v === normalizedValue)) {
          return mapping.color
        }
      }
      if (mapping.pattern) {
        try {
          if (new RegExp(mapping.pattern, 'i').test(normalizedValue)) {
            return mapping.color
          }
        } catch {
          // Skip
        }
      }
    }
  }

  return undefined
}

export function LEDIndicator({
  dataSource,
  state: propState = 'off',
  label,
  size = 'md',
  valueMap,
  defaultState = 'unknown',
  color,
  showCard = true,
  showGlow = true,
  className,
}: LEDIndicatorProps) {
  const { data, loading, error } = useDataSource<unknown>(dataSource)

  // Determine the final state
  const ledState = useMemo(() => {
    if (error) return 'error'
    if (loading) return 'unknown'

    if (data !== undefined && valueMap && valueMap.length > 0) {
      return matchValueToState(data, valueMap, defaultState)
    }

    if (data !== undefined) {
      const dataStr = String(data).trim().toLowerCase()
      if (['on', 'true', '1', 'yes', 'enabled', 'active', 'online'].includes(dataStr)) {
        return 'on'
      }
      if (['off', 'false', '0', 'no', 'disabled', 'inactive', 'offline'].includes(dataStr)) {
        return 'off'
      }
      if (['error', 'failed', 'failure', 'critical'].includes(dataStr)) {
        return 'error'
      }
      if (['warning', 'warn'].includes(dataStr)) {
        return 'warning'
      }
    }

    return propState
  }, [data, valueMap, defaultState, propState, loading, error])

  const customLabel = useMemo(() => {
    if (data !== undefined && valueMap) {
      return getCustomLabel(data, valueMap, ledState)
    }
    return undefined
  }, [data, valueMap, ledState])

  const customColor = useMemo(() => {
    if (data !== undefined && valueMap) {
      return getCustomColor(data, valueMap, ledState)
    }
    return undefined
  }, [data, valueMap, ledState])

  const config = dashboardComponentSize[size]
  const stateCfg = stateConfig[ledState] || stateConfig.unknown
  const indicatorState = stateCfg.indicatorState
  const isActive = ledState === 'on' || ledState === 'error' || ledState === 'warning'

  const finalColor = customColor || color || stateCfg.color.base
  const colorConfig = stateCfg.color
  const animation = getLedAnimation(indicatorState, false)

  // Enhanced glow effect
  const glowStyle = showGlow && isActive
    ? `0 0 8px ${finalColor}60, 0 0 16px ${finalColor}40, 0 0 24px ${finalColor}20`
    : 'none'

  const displayLabel = label || customLabel || stateCfg.label

  // Loading state
  if (loading) {
    return (
      <div className={cn(dashboardCardBase, 'flex-row items-center', config.contentGap, config.padding, className)}>
        <Skeleton className={cn(config.iconContainer, 'rounded-full')} />
        <Skeleton className={cn('h-4 w-20 rounded')} />
      </div>
    )
  }

  const content = (
    <>
      {/* LED Section - left side like ValueCard icon */}
      <div className={cn(
        'flex items-center justify-center shrink-0 rounded-full',
        config.iconContainer,
        isActive ? colorConfig.bg : 'bg-muted/30',
        animation.className
      )}
      style={{
        boxShadow: glowStyle !== 'none' ? glowStyle : undefined,
      }}>
        {/* LED dot */}
        <div
          className={cn(
            'rounded-full transition-all duration-300',
            size === 'sm' ? 'h-2.5 w-2.5' : size === 'md' ? 'h-3 w-3' : 'h-4 w-4',
            isActive && 'ring-2 ring-white/20'
          )}
          style={{
            backgroundColor: finalColor,
            boxShadow: isActive ? `inset 0 1px 2px rgba(255,255,255,0.3), inset 0 -1px 2px rgba(0,0,0,0.2)` : undefined,
          }}
        />
      </div>

      {/* Label section - right side */}
      <div className="flex flex-col min-w-0 flex-1">
        <span className={cn(indicatorFontWeight.title, 'text-foreground truncate', config.titleText)}>
          {displayLabel}
        </span>
        {customLabel && (
          <span className={cn(indicatorFontWeight.label, 'text-muted-foreground', config.labelText)}>
            {stateCfg.label}
          </span>
        )}
      </div>
    </>
  )

  if (showCard) {
    return (
      <div className={cn(dashboardCardBase, 'flex-row items-center', config.contentGap, config.padding, className)}>
        {content}
      </div>
    )
  }

  return <div className={cn('flex items-center', config.contentGap, 'w-full', config.padding, className)}>{content}</div>
}
