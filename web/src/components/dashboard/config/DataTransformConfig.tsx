/**
 * DataTransformConfig Component
 *
 * Configuration UI for time-series data transformation.
 * Uses unified Field component for consistent styling.
 */

import { useMemo, useEffect } from 'react'
import { useTranslation } from 'react-i18next'
import { Clock, Layers } from 'lucide-react'
import { Label } from '@/components/ui/label'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'
import { Field } from '@/components/ui/field'
import type {
  TelemetryAggregate,
  TimeWindowType,
  DataSource,
  DataSourceOrList,
} from '@/types/dashboard'
import { normalizeDataSource } from '@/types/dashboard'

export interface DataTransformConfigProps {
  dataSource?: DataSourceOrList
  onChange: (updates: Partial<DataSource>) => void
  // Show/hide specific options based on chart type
  chartType?: 'pie' | 'bar' | 'line' | 'area' | 'card' | 'sparkline' | 'led' | 'progress'
  readonly?: boolean
  // Simplified mode for single-value components (card, led, progress)
  simplified?: boolean
}

// ============================================================================
// Options Data
// ============================================================================

// Time window options factory (uses translations)
function getTimeWindowOptions(t: (key: string) => string): Array<{ value: TimeWindowType; label: string }> {
  return [
    { value: 'now', label: t('dataTransform.timeWindow.now') },
    { value: 'last_5min', label: t('dataTransform.timeWindow.last5min') },
    { value: 'last_15min', label: t('dataTransform.timeWindow.last15min') },
    { value: 'last_30min', label: t('dataTransform.timeWindow.last30min') },
    { value: 'last_1hour', label: t('dataTransform.timeWindow.last1hour') },
    { value: 'last_6hours', label: t('dataTransform.timeWindow.last6hours') },
    { value: 'last_24hours', label: t('dataTransform.timeWindow.last24hours') },
    { value: 'today', label: t('dataTransform.timeWindow.today') },
    { value: 'yesterday', label: t('dataTransform.timeWindow.yesterday') },
    { value: 'this_week', label: t('dataTransform.timeWindow.thisWeek') },
  ]
}

// All available aggregation options
const ALL_AGGREGATE_OPTIONS: Array<{ value: TelemetryAggregate; labelKey: string }> = [
  { value: 'latest', labelKey: 'dataTransform.aggregate.latest' },
  { value: 'first', labelKey: 'dataTransform.aggregate.first' },
  { value: 'avg', labelKey: 'dataTransform.aggregate.avg' },
  { value: 'min', labelKey: 'dataTransform.aggregate.min' },
  { value: 'max', labelKey: 'dataTransform.aggregate.max' },
  { value: 'sum', labelKey: 'dataTransform.aggregate.sum' },
  { value: 'count', labelKey: 'dataTransform.aggregate.count' },
  { value: 'delta', labelKey: 'dataTransform.aggregate.delta' },
  { value: 'rate', labelKey: 'dataTransform.aggregate.rate' },
  { value: 'raw', labelKey: 'dataTransform.aggregate.raw' },
]

// Aggregation options for each chart type (only show meaningful options)
const AGGREGATE_OPTIONS_BY_CHART_TYPE: Record<string, TelemetryAggregate[]> = {
  // Time-series charts: show raw points and aggregations
  'line-chart': ['raw', 'latest', 'avg', 'min', 'max', 'sum'],
  'area-chart': ['raw', 'latest', 'avg', 'min', 'max', 'sum'],
  'bar-chart': ['raw', 'avg', 'count', 'latest', 'min', 'max', 'sum'],
  'sparkline': ['raw', 'latest', 'avg', 'min', 'max', 'sum'],
  // Pie chart: part-to-whole, single values only
  'pie-chart': ['latest', 'avg', 'sum', 'count'],
  // Single-value indicators: latest or aggregated values
  'card': ['latest', 'avg', 'min', 'max'],
  'led': ['latest', 'avg', 'min', 'max'],
  'progress': ['latest', 'avg', 'min', 'max'],
  // Image history: raw points for history
  'image-history': ['raw', 'latest'],
  // Default: show all options
  'default': ['latest', 'first', 'avg', 'min', 'max', 'sum', 'count', 'delta', 'rate', 'raw'],
}

// Aggregation method options factory (uses translations)
function getAggregateOptions(
  t: (key: string) => string,
  chartType: string
): Array<{ value: TelemetryAggregate; label: string }> {
  const allowedValues = AGGREGATE_OPTIONS_BY_CHART_TYPE[chartType] ?? AGGREGATE_OPTIONS_BY_CHART_TYPE['default']
  return ALL_AGGREGATE_OPTIONS
    .filter(opt => allowedValues.includes(opt.value))
    .map(opt => ({ value: opt.value, label: t(opt.labelKey) }))
}

// Data point limit options factory (uses translations)
function getDataPointOptions(t: (key: string) => string): Array<{ value: number; label: string }> {
  return [
    { value: 12, label: t('dataTransform.dataPoints.12') },
    { value: 24, label: t('dataTransform.dataPoints.24') },
    { value: 50, label: t('dataTransform.dataPoints.50') },
    { value: 100, label: t('dataTransform.dataPoints.100') },
    { value: 200, label: t('dataTransform.dataPoints.200') },
  ]
}

// Simplified aggregate options factory (uses translations)
function getSimplifiedAggregateOptions(t: (key: string) => string): Array<{ value: TelemetryAggregate; label: string }> {
  return [
    { value: 'latest', label: t('dataTransform.aggregate.latest') },
    { value: 'avg', label: t('dataTransform.aggregate.avg') },
  ]
}

// ============================================================================
// Defaults by Chart Type
// ============================================================================

const DEFAULTS_BY_CHART: Record<string, { aggregate: TelemetryAggregate; limit: number }> = {
  pie: { aggregate: 'latest', limit: 10 },
  bar: { aggregate: 'raw', limit: 50 },
  line: { aggregate: 'raw', limit: 50 },
  area: { aggregate: 'raw', limit: 50 },
  card: { aggregate: 'latest', limit: 1 },
  sparkline: { aggregate: 'raw', limit: 50 },
  led: { aggregate: 'latest', limit: 1 },
  progress: { aggregate: 'latest', limit: 1 },
}

// ============================================================================
// Component
// ============================================================================

export function DataTransformConfig({
  dataSource,
  onChange,
  chartType = 'bar',
  readonly = false,
  simplified = false,
}: DataTransformConfigProps) {
  const { t } = useTranslation('dashboardComponents')

  // Normalize to array for consistent handling
  const sources = useMemo(() => normalizeDataSource(dataSource), [dataSource])
  const hasMultipleSources = sources.length > 1
  const firstSource = sources[0]

  // Determine if this is a single-value component that needs simplified options
  const isSimplified = simplified || ['card', 'led', 'progress'].includes(chartType)

  // Get current values from the first source with defaults
  const currentAggregate = useMemo(() => {
    // For card and progress types, prefer 'latest' over 'raw' as default
    if (chartType === 'card' || chartType === 'progress') {
      return firstSource?.aggregateExt ?? firstSource?.aggregate ?? 'latest'
    }
    return firstSource?.aggregateExt ?? firstSource?.aggregate ?? DEFAULTS_BY_CHART[chartType]?.aggregate ?? 'raw'
  }, [firstSource, chartType])

  const currentTimeWindow = useMemo(() => {
    return firstSource?.timeWindow?.type ?? 'last_1hour'
  }, [firstSource])

  const currentLimit = useMemo(() => {
    return firstSource?.limit ?? DEFAULTS_BY_CHART[chartType]?.limit ?? 50
  }, [firstSource, chartType])

  // Detect if sources have different settings (for visual feedback)
  const hasMixedSettings = useMemo(() => {
    if (sources.length <= 1) {
      return { hasMixedAggregate: false, hasMixedTimeWindow: false, hasMixedLimit: false }
    }

    // Check if all sources have the same aggregate setting
    const firstAggregate = firstSource?.aggregateExt ?? firstSource?.aggregate
    const hasMixedAggregate = sources.some(s => {
      const agg = s?.aggregateExt ?? s?.aggregate
      return agg !== firstAggregate
    })

    // Check if all sources have the same time window
    const firstTimeWindow = firstSource?.timeWindow?.type
    const hasMixedTimeWindow = sources.some(s => s?.timeWindow?.type !== firstTimeWindow)

    // Check if all sources have the same limit
    const firstLimit = firstSource?.limit
    const hasMixedLimit = sources.some(s => s?.limit !== firstLimit)

    return { hasMixedAggregate, hasMixedTimeWindow, hasMixedLimit }
  }, [sources, firstSource])

  // Choose aggregate options based on mode and chart type
  const aggregateOptions = isSimplified
    ? getSimplifiedAggregateOptions(t)
    : getAggregateOptions(t, chartType)

  // Initialize aggregate to correct default for card/progress when not explicitly set
  useEffect(() => {
    const shouldDefaultToLatest = (chartType === 'card' || chartType === 'progress') &&
      !firstSource?.aggregateExt &&
      firstSource?.aggregate === 'raw'

    if (shouldDefaultToLatest) {
      onChange({ aggregateExt: 'latest' })
    }
  }, [chartType, firstSource, onChange])

  // Update handlers
  const handleAggregateChange = (value: string) => {
    onChange({
      aggregateExt: value as TelemetryAggregate,
      // For backward compatibility, also set aggregate if it maps
      ...(value === 'raw' || value === 'avg' || value === 'min' || value === 'max' || value === 'sum'
        ? { aggregate: value }
        : {}),
    })
  }

  const handleTimeWindowChange = (value: string) => {
    const type = value as TimeWindowType
    onChange({
      timeWindow: { type },
      // Also update legacy timeRange for backward compatibility
      timeRange: getTimeWindowHours(type),
    })
  }

  const handleLimitChange = (value: string) => {
    onChange({ limit: parseInt(value) || 50 })
  }

  return (
    <div className="space-y-3">
      {/* Multi-source indicator */}
      {hasMultipleSources && (
        <div className="flex items-center gap-2 px-2.5 py-1.5 bg-blue-500/10 border border-blue-500/20 rounded-md">
          <Layers className="h-3.5 w-3.5 text-blue-500" />
          <span className="text-xs text-blue-600 font-medium">
            {t('dataTransform.appliesToAll', { count: sources.length })}
          </span>
        </div>
      )}

      {/* Time Window - simplified mode only shows now and last_1hour */}
      <Field>
        <div className="flex items-center justify-between">
          <Label>{t('dataTransform.timeRange')}</Label>
          {hasMixedSettings?.hasMixedTimeWindow && (
            <span className="text-[10px] text-amber-600 bg-amber-500/10 px-1.5 py-0.5 rounded">
              {t('dataTransform.mixedValues')}
            </span>
          )}
        </div>
        <Select value={currentTimeWindow} onValueChange={handleTimeWindowChange} disabled={readonly}>
          <SelectTrigger>
            <SelectValue placeholder={t('dataTransform.selectTimeRange')} />
          </SelectTrigger>
          <SelectContent>
            {getTimeWindowOptions(t).filter(o => {
              if (isSimplified) {
                // Single-value components only need current/recent values
                return o.value === 'now' || o.value === 'last_5min' || o.value === 'last_15min' || o.value === 'last_30min' || o.value === 'last_1hour'
              }
              return true
            }).map((option) => (
              <SelectItem key={option.value} value={option.value}>
                {option.label}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
      </Field>

      {/* Aggregation Method */}
      <Field>
        <div className="flex items-center justify-between">
          <Label>{t('dataTransform.aggregation')}</Label>
          {hasMixedSettings?.hasMixedAggregate && (
            <span className="text-[10px] text-amber-600 bg-amber-500/10 px-1.5 py-0.5 rounded">
              {t('dataTransform.mixedValues')}
            </span>
          )}
        </div>
        <Select value={currentAggregate} onValueChange={handleAggregateChange} disabled={readonly}>
          <SelectTrigger>
            <SelectValue placeholder={t('dataTransform.selectAggregation')} />
          </SelectTrigger>
          <SelectContent>
            {aggregateOptions.map((option) => (
              <SelectItem key={option.value} value={option.value}>
                {option.label}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
      </Field>

      {/* Data Points Limit - only for charts with raw aggregate */}
      {!isSimplified && (currentAggregate === 'raw' || chartType === 'bar' || chartType === 'line' || chartType === 'area' || chartType === 'sparkline') && (
        <Field>
          <div className="flex items-center justify-between">
            <Label>{t('dataTransform.dataPointLimit')}</Label>
            {hasMixedSettings?.hasMixedLimit && (
              <span className="text-[10px] text-amber-600 bg-amber-500/10 px-1.5 py-0.5 rounded">
                {t('dataTransform.mixedValues')}
              </span>
            )}
          </div>
          <Select
            value={String(currentLimit)}
            onValueChange={handleLimitChange}
            disabled={readonly}
          >
            <SelectTrigger>
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              {getDataPointOptions(t).map((option) => (
                <SelectItem key={option.value} value={String(option.value)}>
                  {option.label}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        </Field>
      )}
    </div>
  )
}

// ============================================================================
// Helper Functions
// ============================================================================

// Helper function to convert time window type to hours
function getTimeWindowHours(type: TimeWindowType): number {
  const conversions: Record<TimeWindowType, number> = {
    'now': 0,
    'last_5min': 5 / 60,
    'last_15min': 15 / 60,
    'last_30min': 30 / 60,
    'last_1hour': 1,
    'last_6hours': 6,
    'last_24hours': 24,
    'today': 24,
    'yesterday': 24,
    'this_week': 24 * 7,
    'custom': 1,
  }
  return conversions[type] ?? 1
}
