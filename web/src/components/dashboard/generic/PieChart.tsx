/**
 * Pie Chart Component
 *
 * Unified with dashboard design system.
 * Supports telemetry data binding for categorical/part-to-whole data.
 *
 * Enhanced with time-series aggregation support:
 * - Aggregate multiple data points into single values
 * - Support for different aggregation methods (latest, avg, sum, etc.)
 * - Time window selection for data scope
 */

import { useMemo, memo } from 'react'
import { useTranslation } from 'react-i18next'
import {
  PieChart as RechartsPieChart,
  Pie,
  Cell,
  Tooltip,
  Legend,
} from 'recharts'
import { Skeleton } from '@/components/ui/skeleton'
import { cn } from '@/lib/utils'
import { DataMapper, type CategoricalMappingConfig } from '@/lib/dataMapping'
import { dashboardCardBase, dashboardComponentSize } from '@/design-system/tokens/size'
import { indicatorFontWeight } from '@/design-system/tokens/indicator'
import { chartColors as designChartColors, chartColorsHex } from '@/design-system/tokens/color'
import type { DataSourceOrList, TelemetryAggregate } from '@/types/dashboard'
import { getSourceId } from '@/types/dashboard'
import { ChartContainer, ChartTooltip, EmptyState, useChartDimensions, useStaggeredData, createMemoRenderer, useChartPipeline } from '../shared'
import { isNameValueData, isNumberArray, isMultiSourceData } from '../shared'
import {
  aggregateData,
} from '@/lib/telemetryTransform'

// Use design system chart colors
const _chartColors = designChartColors

// Fallback colors as hex values for SVG
const fallbackColors = chartColorsHex

/**
 * Transform telemetry points to pie chart data using DataMapper.
 * Handles: [{ name, value }, { label, val }, { category, count }, { timestamp, value }] or raw numbers
 *
 * For categorical data (strings), groups by value and counts occurrences.
 * The aggregate parameter controls behavior:
 * - 'count': Always group by value and count (for distribution charts)
 * - Other: Auto-detect categorical vs numeric data
 */
function transformTelemetryToPieData(
  data: unknown,
  dataMapping?: CategoricalMappingConfig,
  aggregate?: TelemetryAggregate,
  t?: (key: string, params?: Record<string, unknown>) => string
): PieData[] {
  if (!data || !Array.isArray(data)) return []

  // Handle simple number array
  if (data.length > 0 && typeof data[0] === 'number') {
    return (data as number[]).map((value, index) => ({
      name: t ? t('chart.item', { count: index + 1 }) : `Item ${index + 1}`,
      value,
    }))
  }

  // Check if already in PieData format FIRST (has both name and value)
  if (data.length > 0 && typeof data[0] === 'object' && data[0] !== null && 'value' in data[0] && 'name' in data[0]) {
    return data as PieData[]
  }

  // Handle raw telemetry points format: [{ timestamp, value }, ...]
  // Only has 'value' but not 'name' → this is telemetry data
  // For pie chart, we aggregate or show distribution over time
  if (data.length > 0 && typeof data[0] === 'object' && data[0] !== null && 'value' in data[0]) {
    const telemetryPoints = data as Array<{ timestamp?: number; value: unknown }>

    // Check if values are categorical (strings) OR if aggregate is 'count' - group and count for distribution
    const firstValue = telemetryPoints[0]?.value
    const isCategorical = typeof firstValue === 'string'
    const forceCounting = aggregate === 'count'

    if (isCategorical || forceCounting) {
      // Group by value and count occurrences for categorical data or when 'count' aggregate is set
      const counts = new Map<string, number>()
      for (const point of telemetryPoints) {
        // For numeric values with counting, group by rounded value to avoid too many unique values
        let key: string
        if (typeof point.value === 'number') {
          // Round to 2 decimal places for grouping
          key = String(Math.round(point.value * 100) / 100)
        } else {
          key = String(point.value ?? 'unknown')
        }
        counts.set(key, (counts.get(key) ?? 0) + 1)
      }

      // Convert to PieData format, sorted by count descending
      return Array.from(counts.entries())
        .sort((a, b) => b[1] - a[1])
        .map(([name, value]) => ({
          name,
          value,
        }))
    }

    // For numeric telemetry data without count aggregation, create time-based labels
    return telemetryPoints.map((point, index) => {
      let name = t ? t('chart.item', { count: index + 1 }) : `Item ${index + 1}`
      if (point.timestamp) {
        const date = new Date(point.timestamp > 10000000000 ? point.timestamp : point.timestamp * 1000)
        if (!isNaN(date.getTime())) {
          name = date.toLocaleTimeString('en-US', { hour: '2-digit', minute: '2-digit' })
        }
      }
      return {
        name,
        value: typeof point.value === 'number' ? point.value : 0,
      }
    })
  }

  // Use DataMapper for categorical data
  const categoricalPoints = DataMapper.mapToCategorical(data, dataMapping)

  return categoricalPoints.map(p => ({
    name: p.name,
    value: p.value,
    color: p.color,
  }))
}

export interface PieData {
  name: string
  value: number
  color?: string
}

export interface PieChartProps {
  // Data source configuration
  dataSource?: DataSourceOrList  // Support both single and multiple data sources

  // Data
  data?: PieData[]

  // Display options
  title?: string
  height?: number | 'auto'
  showLegend?: boolean
  showTooltip?: boolean
  showLabels?: boolean

  // Style
  variant?: 'pie' | 'donut'
  innerRadius?: number | string
  outerRadius?: number | string

  // === Telemetry options ===
  // Legacy options (kept for backward compatibility)
  limit?: number
  timeRange?: number

  // === New: Data transformation options ===
  // How to aggregate time-series data
  aggregate?: TelemetryAggregate
  // Time window for data
  timeWindow?: 'now' | 'last_5min' | 'last_15min' | 'last_30min' | 'last_1hour' | 'last_6hours' | 'last_24hours'

  // Data mapping configuration
  dataMapping?: CategoricalMappingConfig

  // Styling
  colors?: string[]
  size?: 'sm' | 'md' | 'lg'
  className?: string
}

export const PieChart = memo(function PieChart({
  dataSource,
  data: propData,
  title,
  height = 'auto',
  showLegend = false,
  showTooltip = true,
  showLabels = false,
  variant = 'donut',
  innerRadius = '60%',
  outerRadius = '80%',
  limit = 10,
  timeRange = 1,
  aggregate = 'latest',  // Default to latest value for pie charts
  timeWindow,
  dataMapping,
  colors,
  size = 'md',
  className,
}: PieChartProps) {
  const { t } = useTranslation('dashboardComponents')
  const config = dashboardComponentSize[size]

  // Shared data pipeline — same pattern as LineChart/AreaChart
  const {
    sources, data, loading, effectiveAggregate,
    hasData, showLoading, getSeriesName, getDeviceName,
  } = useChartPipeline<PieData[] | number[] | number[][]>({
    dataSource,
    aggregate,
    limit,
    timeRange,
    fallback: propData ?? [],
    preserveMultiple: true,
  })

  // Normalize data to PieData[] format
  const chartData: PieData[] = useMemo(() => {
    const chartColors = colors || fallbackColors

    // Multi-source data - combine into single pie chart
    if (isMultiSourceData(data, sources.length)) {
      return sources.map((ds, i) => {
        const arr = data[i]
        const seriesLabel = getSeriesName(ds, i)
        if (!Array.isArray(arr)) {
          return {
            name: seriesLabel,
            value: 0,
            color: chartColors[i % chartColors.length],
          }
        }

        // Check if data contains telemetry points with value property
        const firstItem = arr[0] as { value?: unknown; timestamp?: number } | undefined
        const hasTelemetryPoints = typeof firstItem === 'object' && firstItem !== null && 'value' in firstItem

        if (hasTelemetryPoints) {
          // Extract values from telemetry points
          const values = (arr as Array<{ value?: unknown }>).map(item => {
            const val = item.value
            return typeof val === 'number' ? val : parseFloat(String(val)) || 0
          }).filter(v => !isNaN(v))

          if (values.length > 0) {
            // Create timePoints for aggregation (use actual timestamps if available)
            const now = Date.now() / 1000
            const timePoints = (arr as Array<{ value?: unknown; timestamp?: number }>).map((item, idx) => ({
              timestamp: item.timestamp || (now - (values.length - idx) * 60),
              value: values[idx],
            }))

            const aggregatedValue = aggregateData(timePoints, effectiveAggregate)
            return {
              name: seriesLabel,
              value: aggregatedValue ?? 0,
              color: chartColors[i % chartColors.length],
            }
          }
        }

        // Handle simple number array
        const numericValues = (arr as unknown[]).filter(v => typeof v === 'number')
        if (numericValues.length > 0) {
          const now = Date.now() / 1000
          const timePoints = numericValues.map((v, idx) => ({
            timestamp: now - (numericValues.length - idx) * 60,
            value: v as number,
          }))
          const aggregatedValue = aggregateData(timePoints, effectiveAggregate)
          return {
            name: seriesLabel,
            value: aggregatedValue ?? 0,
            color: chartColors[i % chartColors.length],
          }
        }

        return {
          name: seriesLabel,
          value: 0,
          color: chartColors[i % chartColors.length],
        }
      })
    }

    // Handle telemetry data FIRST (when dataSource is provided)
    if (dataSource && isNameValueData(data)) {
      return data as PieData[]
    }

    // Transform telemetry points (handles both numeric and categorical data)
    if (dataSource && Array.isArray(data) && data.length > 0 && !isNameValueData(data)) {
      const transformed = transformTelemetryToPieData(data, dataMapping, effectiveAggregate, t)
      if (transformed.length > 0) {
        return transformed
      }
    }

    // Handle number array from data source - apply aggregation
    if (dataSource && isNumberArray(data)) {
      const values = data as number[]
      const timePoints = values.map((v, idx) => ({
        timestamp: Date.now() / 1000 - (values.length - idx) * 60,
        value: v,
      }))

      const aggregatedValue = aggregateData(timePoints, effectiveAggregate)
      return [{
        name: getSourceId(sources[0]) ? getDeviceName(getSourceId(sources[0])!) : 'Value',
        value: aggregatedValue ?? values[values.length - 1] ?? 0,
      }]
    }

    // If no dataSource, use propData (static data)
    if (!dataSource && propData && Array.isArray(propData) && propData.length > 0) {
      return propData
    }

    // If dataSource is set but data is empty, return empty array (will show EmptyState)
    if (dataSource && !loading && Array.isArray(data) && data.length === 0) {
      return []
    }

    // Return default sample data for preview only (no dataSource)
    return [
      { name: t('chart.categoryA'), value: 30 },
      { name: t('chart.categoryB'), value: 45 },
      { name: t('chart.categoryC'), value: 25 },
    ]
  }, [data, propData, dataSource, sources, dataMapping, effectiveAggregate, loading, t, colors, getSeriesName, getDeviceName])

  if (showLoading) {
    return (
      <div className={cn(dashboardCardBase, 'h-full flex flex-col', config.padding, className)}>
        {title && (
          <div className={cn('mb-3', indicatorFontWeight.title, config.titleText)}>{title}</div>
        )}
        <Skeleton className={cn('w-full', 'flex-1 min-h-0')} />
      </div>
    )
  }

  // Only show empty state when loading is fully complete
  if (!loading && chartData.length === 0) {
    return <EmptyState size={size} className={className} message={title ? `${title} - No Data Available` : undefined} />
  }

  const effectiveColors = colors || fallbackColors

  return (
    <div className={cn(dashboardCardBase, config.padding, className)}>
      {title && (
        <div className={cn('mb-3', indicatorFontWeight.title, config.titleText)}>{title}</div>
      )}
      <ChartContainer>
        <PieChartWithDimensions chartData={chartData} showTooltip={showTooltip} showLegend={showLegend} showLabels={showLabels} variant={variant} innerRadius={innerRadius} outerRadius={outerRadius} effectiveColors={effectiveColors} />
      </ChartContainer>
    </div>
  )
})

const PieChartRenderer = createMemoRenderer(({ chartData, showTooltip, showLegend, showLabels, variant, innerRadius, outerRadius, effectiveColors, width, height }: {
  chartData: PieData[]
  showTooltip: boolean
  showLegend: boolean
  showLabels: boolean
  variant: string
  innerRadius: number | string
  outerRadius: number | string
  effectiveColors: readonly string[]
  width: number
  height: number
}) => (
  <RechartsPieChart width={width} height={height} margin={{ top: 0, right: 0, bottom: 0, left: 0 }}>
    <Pie
      data={chartData}
      cx="50%"
      cy="50%"
      labelLine={false}
      label={showLabels ? (entry) => `${entry.name}` : false}
      innerRadius={variant === 'donut' ? innerRadius : 0}
      outerRadius={outerRadius}
      dataKey="value"
      isAnimationActive
      animationDuration={600}
    >
      {chartData.map((entry, index) => (
        <Cell key={`cell-${index}`} fill={entry.color || effectiveColors[index % effectiveColors.length]} stroke="none" />
      ))}
    </Pie>
    {showTooltip && <Tooltip content={<ChartTooltip />} />}
    {showLegend && <Legend />}
  </RechartsPieChart>
))

function PieChartWithDimensions({ chartData, showTooltip, showLegend, showLabels, variant, innerRadius, outerRadius, effectiveColors }: {
  chartData: PieData[]
  showTooltip: boolean
  showLegend: boolean
  showLabels: boolean
  variant: string
  innerRadius: number | string
  outerRadius: number | string
  effectiveColors: readonly string[]
}) {
  const { ref, width, height, turn } = useChartDimensions()
  const staggeredChartData = useStaggeredData(chartData, turn)
  return (
    <div ref={ref} style={{ width: '100%', height: '100%' }}>
      {width > 0 && height > 0 && (
        <PieChartRenderer chartData={staggeredChartData} showTooltip={showTooltip} showLegend={showLegend} showLabels={showLabels} variant={variant} innerRadius={innerRadius} outerRadius={outerRadius} effectiveColors={effectiveColors} width={width} height={height} />
      )}
    </div>
  )
}
