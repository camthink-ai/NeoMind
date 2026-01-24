/**
 * Gauge Chart Component
 *
 * shadcn/ui inspired semi-circle gauge.
 * Clean design with smooth needle animation.
 */

import { useRef, useEffect, useState } from 'react'
import { cn } from '@/lib/utils'
import { Card, CardContent } from '@/components/ui/card'
import { Skeleton } from '@/components/ui/skeleton'
import { useDataSource } from '@/hooks/useDataSource'
import type { DataSourceOrList } from '@/types/dashboard'

export interface GaugeZone {
  from: number
  to: number
  color: string
  label?: string
}

export interface GaugeChartProps {
  // Data source configuration
  dataSource?: DataSourceOrList

  // Display configuration
  value?: number
  min?: number
  max?: number
  label?: string
  unit?: string
  size?: number

  // Zones for color coding
  zones?: GaugeZone[]

  // Custom colors
  color?: string

  // Styling
  variant?: 'gauge' | 'semi' | 'arc'
  showValue?: boolean
  className?: string
}

// shadcn/ui inspired zones with proper colors
const DEFAULT_ZONES: GaugeZone[] = [
  { from: 0, to: 0.4, color: 'hsl(var(--chart-1))', label: 'Low' },
  { from: 0.4, to: 0.7, color: 'hsl(var(--chart-3))', label: 'Medium' },
  { from: 0.7, to: 1, color: 'hsl(var(--destructive))', label: 'High' },
]

/**
 * Safely convert to number with clamping
 */
function safeToNumber(value: unknown, min: number, max: number): number {
  if (typeof value === 'number') return Math.max(min, Math.min(max, value))
  if (typeof value === 'string') {
    const num = parseFloat(value)
    if (!isNaN(num)) return Math.max(min, Math.min(max, num))
  }
  if (typeof value === 'boolean') return value ? max : min
  return min
}

// Responsive size presets
const sizePresets = {
  sm: { strokeWidth: 10, valueText: 'text-lg', labelText: 'text-xs' },
  md: { strokeWidth: 12, valueText: 'text-2xl', labelText: 'text-xs' },
  lg: { strokeWidth: 14, valueText: 'text-3xl', labelText: 'text-sm' },
  xl: { strokeWidth: 16, valueText: 'text-4xl', labelText: 'text-sm' },
}

export function GaugeChart({
  dataSource,
  value: propValue,
  min = 0,
  max = 100,
  label,
  unit,
  size = 160,
  zones = DEFAULT_ZONES,
  color,
  variant = 'gauge',
  showValue = true,
  className,
}: GaugeChartProps) {
  const containerRef = useRef<HTMLDivElement>(null)
  const [containerWidth, setContainerWidth] = useState(size)

  // Track container size for responsiveness
  useEffect(() => {
    const updateSize = () => {
      if (containerRef.current) {
        const width = containerRef.current.offsetWidth
        setContainerWidth(Math.min(width, size) || size)
      }
    }

    updateSize()

    const resizeObserver = new ResizeObserver(updateSize)
    if (containerRef.current) {
      resizeObserver.observe(containerRef.current)
    }

    return () => resizeObserver.disconnect()
  }, [size])

  // Get data from source with error handling
  const { data, loading, error } = useDataSource<number>(dataSource, {
    fallback: propValue ?? 0,
  })

  const rawValue = error ? propValue : data ?? propValue ?? 0
  const value = safeToNumber(rawValue, min, max)

  // Determine size preset based on container width
  const sizeKey = containerWidth < 140 ? 'sm' : containerWidth < 180 ? 'md' : containerWidth < 220 ? 'lg' : 'xl'
  const preset = sizePresets[sizeKey]

  const range = max - min
  const normalizedValue = (value - min) / range

  // Gauge dimensions - responsive to container
  const width = containerWidth
  const height = variant === 'gauge' ? width / 2 : width / 3
  const strokeWidth = preset.strokeWidth
  const radius = (width - strokeWidth) / 2
  const centerX = width / 2
  const centerY = height - strokeWidth / 2

  // Create zone path
  const createZonePath = (startFraction: number, endFraction: number) => {
    const startAngle = Math.PI + startFraction * Math.PI
    const endAngle = Math.PI + endFraction * Math.PI

    const x1 = centerX + radius * Math.cos(startAngle)
    const y1 = centerY + radius * Math.sin(startAngle)
    const x2 = centerX + radius * Math.cos(endAngle)
    const y2 = centerY + radius * Math.sin(endAngle)

    const largeArcFlag = endFraction - startFraction > 0.5 ? 1 : 0

    return `M ${x1} ${y1} A ${radius} ${radius} 0 ${largeArcFlag} 1 ${x2} ${y2}`
  }

  // Determine color based on zones
  const getZoneColor = () => {
    if (color) return color
    const zone = zones.find(z => normalizedValue >= z.from && normalizedValue <= z.to)
    return zone?.color || zones[zones.length - 1]?.color || 'hsl(var(--primary))'
  }

  const gaugeColor = getZoneColor()

  // Needle path
  const needleAngle = Math.PI + normalizedValue * Math.PI
  const needleLength = radius - strokeWidth
  const needleX = centerX + needleLength * Math.cos(needleAngle)
  const needleY = centerY + needleLength * Math.sin(needleAngle)

  return (
    <Card className={cn('border shadow-sm overflow-hidden flex flex-col h-full', className)}>
      <CardContent ref={containerRef} className="flex-1 min-h-0 p-4 flex flex-col items-center justify-center">
        <style>{`
          :root {
            --chart-1: oklch(0.646 0.222 264.38);
            --chart-3: oklch(0.646 0.222 48.85);
          }
        `}</style>
        <div className="flex flex-col items-center w-full">
          {loading ? (
            <Skeleton className="rounded-full" style={{ width, height }} />
          ) : (
            <>
              <svg width={width} height={height} className="overflow-visible" viewBox={`0 0 ${width} ${height}`} style={{ maxWidth: '100%' }}>
                {/* Zone backgrounds */}
                {zones.map((zone, i) => {
                  const path = createZonePath(zone.from, zone.to)
                  return (
                    <path
                      key={i}
                      d={path}
                      fill="none"
                      stroke={zone.color}
                      strokeWidth={strokeWidth}
                      strokeLinecap="butt"
                      opacity={0.15}
                    />
                  )
                })}

                {/* Value arc */}
                <path
                  d={createZonePath(0, normalizedValue)}
                  fill="none"
                  stroke={gaugeColor}
                  strokeWidth={strokeWidth}
                  strokeLinecap="round"
                  className="transition-all duration-500 ease-out"
                />

                {/* Needle */}
                <line
                  x1={centerX}
                  y1={centerY}
                  x2={needleX}
                  y2={needleY}
                  stroke="hsl(var(--foreground))"
                  strokeWidth="2"
                  strokeLinecap="round"
                  className="transition-all duration-300 ease-out"
                />

                {/* Center circle */}
                <circle
                  cx={centerX}
                  cy={centerY}
                  r={strokeWidth / 2}
                  fill="hsl(var(--background))"
                  stroke="hsl(var(--border))"
                  strokeWidth="1"
                />
              </svg>

              {/* Value display */}
              <div className="text-center -mt-2">
                {showValue && (
                  <span className={cn('font-bold tracking-tight tabular-nums', preset.valueText)}>
                    {value.toFixed(0)}
                  </span>
                )}
                {unit && (
                  <span className="text-sm text-muted-foreground ml-1">{unit}</span>
                )}
                {label && <p className={cn('text-muted-foreground mt-1', preset.labelText)}>{label}</p>}
              </div>
            </>
          )}
        </div>
      </CardContent>
    </Card>
  )
}
