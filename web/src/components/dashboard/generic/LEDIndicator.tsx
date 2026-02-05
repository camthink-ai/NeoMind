/**
 * LED Indicator Component
 *
 * State indicator with LED-like visual feedback.
 * Simplified design with unified state mapping rules.
 */

import { useMemo } from 'react'
import { useTranslation } from 'react-i18next'
import { cn } from '@/lib/utils'
import { DataMapper } from '@/lib/dataMapping'
import { useDataSource } from '@/hooks/useDataSource'
import { Skeleton } from '@/components/ui/skeleton'
import { dashboardComponentSize, dashboardCardBase } from '@/design-system/tokens/size'
import {
  indicatorFontWeight,
  indicatorColors,
  type IndicatorState,
} from '@/design-system/tokens/indicator'
import type { DataSource } from '@/types/dashboard'
import { ErrorState } from '../shared'

export type LEDState = 'on' | 'off' | 'error' | 'warning' | 'unknown'

/**
 * Unified state mapping rule
 * One rule can match by threshold, string values, or regex pattern
 * When matched, it determines the state, optional label, and optional color
 */
export interface StateRule {
  // Match condition (exactly one should be set for meaningful matching)
  threshold?: { operator: '>' | '<' | '>=' | '<=' | '==' | '!='; value: number }
  values?: string      // Comma-separated values, e.g., "online,active,true"
  pattern?: string     // Regex pattern, e.g., "^on|active$"

  // Result when matched
  state: LEDState
  label?: string       // Custom label for this specific match (overrides stateLabels)
  color?: string       // Custom color for this specific match
}

export interface LEDIndicatorProps {
  dataSource?: DataSource

  // Unified state mapping rules
  rules?: StateRule[]

  // Fallback when no rules match
  defaultState?: LEDState

  // Optional global label overrides (applied when rule doesn't specify label)
  stateLabels?: Record<string, string>

  // Display options
  title?: string       // Primary label (static, e.g., "Living Room Light")
  size?: 'sm' | 'md' | 'lg'
  showCard?: boolean
  showGlow?: boolean
  showAnimation?: boolean

  className?: string
}

// Default state configuration
function getStateConfig(t: (key: string) => string) {
  return {
    on: {
      indicatorState: 'success' as IndicatorState,
      label: t('ledIndicator.on'),
      color: indicatorColors.success,
    },
    off: {
      indicatorState: 'neutral' as IndicatorState,
      label: t('ledIndicator.off'),
      color: indicatorColors.neutral,
    },
    error: {
      indicatorState: 'error' as IndicatorState,
      label: t('ledIndicator.error'),
      color: indicatorColors.error,
    },
    warning: {
      indicatorState: 'warning' as IndicatorState,
      label: t('ledIndicator.warning'),
      color: indicatorColors.warning,
    },
    unknown: {
      indicatorState: 'neutral' as IndicatorState,
      label: t('ledIndicator.unknown'),
      color: indicatorColors.neutral,
    },
  }
}

// Extract value from data for matching
function extractValue(data: unknown): string | number | null {
  if (data === null || data === undefined) {
    return null
  }

  // Direct number
  if (typeof data === 'number') {
    return data
  }

  // Direct string
  if (typeof data === 'string') {
    // Try to parse as number first
    const num = parseFloat(data)
    if (!isNaN(num) && /^\s*-?\d+(\.\d+)?\s*$/.test(data)) {
      return num
    }
    return data.trim().toLowerCase()
  }

  // Boolean
  if (typeof data === 'boolean') {
    return data ? 1 : 0
  }

  // Array - take last element (most recent)
  if (Array.isArray(data)) {
    if (data.length > 0) {
      return extractValue(data[data.length - 1])
    }
    return null
  }

  // Object - try to extract value field
  if (typeof data === 'object') {
    const obj = data as Record<string, unknown>
    const valueField = obj.value ?? obj.v ?? obj.val ?? obj.result ?? obj.data ?? obj.state
    if (valueField !== undefined) {
      return extractValue(valueField)
    }
  }

  return String(data).trim().toLowerCase()
}

// Match a single rule against the data
function matchRule(rule: StateRule, data: unknown): boolean {
  const value = extractValue(data)
  if (value === null) return false

  // Threshold matching (for numeric values)
  if (rule.threshold) {
    if (typeof value !== 'number') return false
    return DataMapper.evaluateThreshold(value, rule.threshold.operator, rule.threshold.value)
  }

  // String values matching
  if (rule.values) {
    const valueStr = String(value).toLowerCase().trim()
    const values = rule.values.toLowerCase().split(',').map(v => v.trim())
    return values.some(v => v === valueStr)
  }

  // Regex pattern matching
  if (rule.pattern) {
    try {
      const valueStr = String(value)
      return new RegExp(rule.pattern, 'i').test(valueStr)
    } catch {
      return false
    }
  }

  // Rule with no condition matches everything (fallback rule)
  return true
}

// Find first matching rule and return its state/label/color
function findMatch(
  rules: StateRule[],
  data: unknown,
  defaultState: LEDState
): { state: LEDState; label?: string; color?: string } {
  if (!rules || rules.length === 0) {
    return { state: defaultState }
  }

  for (const rule of rules) {
    if (matchRule(rule, data)) {
      return {
        state: rule.state,
        label: rule.label,
        color: rule.color,
      }
    }
  }

  return { state: defaultState }
}

export function LEDIndicator({
  dataSource,
  rules = [],
  defaultState = 'unknown',
  stateLabels,
  title,
  size = 'md',
  showCard = true,
  showGlow = true,
  showAnimation = true,
  className,
}: LEDIndicatorProps) {
  const { t } = useTranslation('dashboardComponents')
  const stateConfig = getStateConfig(t)
  const { data, loading, error } = useDataSource<unknown>(dataSource)

  // Determine state, label, and color from matching rule
  const { state: ledState, label: ruleLabel, color: ruleColor } = useMemo(() => {
    if (error) return { state: 'error' as LEDState }
    if (loading) return { state: 'unknown' as LEDState }

    // Find first matching rule
    return findMatch(rules, data, defaultState)
  }, [data, rules, defaultState, error, loading])

  const stateCfg = stateConfig[ledState] || stateConfig.unknown
  const isActive = ledState === 'on' || ledState === 'error' || ledState === 'warning'

  // Label priority: rule.label > stateLabels[ledState] > default state label
  const displayLabel = ruleLabel || (stateLabels?.[ledState]) || stateCfg.label

  // Color priority: rule.color > default state color
  const displayColor = ruleColor || stateCfg.color.base

  // Animation class
  const animationClassName = showAnimation && isActive ? 'animate-pulse' : ''

  // Convert hex to rgba for background
  const hexToRgba = (hex: string, alpha: number) => {
    const cleanHex = hex.replace('#', '')
    const r = parseInt(cleanHex.substring(0, 2), 16)
    const g = parseInt(cleanHex.substring(2, 4), 16)
    const b = parseInt(cleanHex.substring(4, 6), 16)
    return `rgba(${r}, ${g}, ${b}, ${alpha})`
  }

  const containerBgColor = isActive ? hexToRgba(displayColor, 0.15) : undefined

  // Glow effect
  const glowStyle = showGlow && isActive
    ? `0 0 8px ${displayColor}60, 0 0 16px ${displayColor}40, 0 0 24px ${displayColor}20`
    : 'none'

  // Error state
  if (error && dataSource) {
    return <ErrorState size={size} className={className} />
  }

  // Loading state
  if (loading) {
    return (
      <div className={cn(dashboardCardBase, 'flex-row items-center', dashboardComponentSize[size].contentGap, dashboardComponentSize[size].padding, className)}>
        <Skeleton className={cn(dashboardComponentSize[size].iconContainer, 'rounded-full')} />
        <Skeleton className={cn('h-4 w-20 rounded')} />
      </div>
    )
  }

  const content = (
    <>
      {/* LED Section */}
      <div className={cn(
        'flex items-center justify-center shrink-0 rounded-full',
        dashboardComponentSize[size].iconContainer,
        !containerBgColor && (isActive ? stateCfg.color.bg : 'bg-muted/30'),
        animationClassName
      )}
      style={{
        backgroundColor: containerBgColor,
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
            backgroundColor: displayColor,
            boxShadow: isActive ? `inset 0 1px 2px rgba(255,255,255,0.3), inset 0 -1px 2px rgba(0,0,0,0.2)` : undefined,
          }}
        />
      </div>

      {/* Label section */}
      <div className="flex flex-col min-w-0 flex-1">
        {/* Primary label - title */}
        {title && (
          <span className={cn(indicatorFontWeight.title, 'text-foreground truncate', dashboardComponentSize[size].titleText)}>
            {title}
          </span>
        )}
        {/* Secondary label - state */}
        <span className={cn(
          indicatorFontWeight.label,
          title ? 'text-muted-foreground' : 'text-foreground',
          dashboardComponentSize[size].labelText
        )}>
          {displayLabel}
        </span>
      </div>
    </>
  )

  if (showCard) {
    return (
      <div className={cn(dashboardCardBase, 'flex-row items-center', dashboardComponentSize[size].contentGap, dashboardComponentSize[size].padding, className)}>
        {content}
      </div>
    )
  }

  return <div className={cn('flex items-center', dashboardComponentSize[size].contentGap, 'w-full', dashboardComponentSize[size].padding, className)}>{content}</div>
}
