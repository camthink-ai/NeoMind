/**
 * Slider Component (Unified Styles)
 *
 * shadcn/ui style slider for numeric device control.
 * Clean design with smooth interactions and visual feedback.
 * Supports brightness, temperature, speed control, etc.
 */

import { useState, useCallback } from 'react'
import { Slider as SliderPrimitive } from '@/components/ui/slider'
import { Button } from '@/components/ui/button'
import { Skeleton } from '@/components/ui/skeleton'
import { Plus, Minus, Sun, Droplets, Wind, Gauge } from 'lucide-react'
import { cn } from '@/lib/utils'
import { useDataSource } from '@/hooks/useDataSource'
import { dashboardCardBase, dashboardComponentSize } from '@/design-system/tokens/size'
import type { DataSource } from '@/types/dashboard'

export interface SliderProps {
  dataSource?: DataSource
  value?: number
  onValueChange?: (value: number) => void
  min?: number
  max?: number
  step?: number
  unit?: string
  label?: string
  description?: string
  icon?: string
  size?: 'sm' | 'md' | 'lg'
  variant?: 'default' | 'compact' | 'inline'
  showButtons?: boolean
  disabled?: boolean
  className?: string
}

// Size configurations using unified tokens
const getSizeConfig = (size: 'sm' | 'md' | 'lg') => {
  const config = dashboardComponentSize[size]
  return {
    padding: config.padding,
    labelText: config.labelText,
    valueText: size === 'sm' ? 'text-lg' : size === 'md' ? 'text-xl' : 'text-2xl',
    iconSize: size === 'sm' ? 'w-8 h-8' : size === 'md' ? 'w-10 h-10' : 'w-12 h-12',
  }
}

// Get icon based on label
function getSliderIcon(label?: string, icon?: string): React.ComponentType<{ className?: string }> {
  if (icon) {
    // Map icon string to component
    const iconMap: Record<string, React.ComponentType<{ className?: string }>> = {
      sun: Sun,
      droplets: Droplets,
      wind: Wind,
      gauge: Gauge,
    }
    return iconMap[icon.toLowerCase()] || Sun
  }

  if (!label) return Gauge
  const lower = label.toLowerCase()
  if (lower.includes('bright') || lower.includes('light')) return Sun
  if (lower.includes('temp') || lower.includes('heat')) return Gauge
  if (lower.includes('humid') || lower.includes('moisture')) return Droplets
  if (lower.includes('speed') || lower.includes('fan')) return Wind
  return Gauge
}

export function Slider({
  dataSource,
  value: propValue = 50,
  onValueChange,
  min = 0,
  max = 100,
  step = 1,
  unit,
  label,
  description,
  icon,
  size = 'md',
  variant = 'default',
  showButtons = true,
  disabled = false,
  className,
}: SliderProps) {
  const [localValue, setLocalValue] = useState(propValue)
  const [isDragging, setIsDragging] = useState(false)

  const { data, loading, sendCommand, sending } = useDataSource<number>(dataSource, {
    fallback: propValue,
  })
  const value = data ?? localValue
  const isCommandSource = dataSource?.type === 'command'

  const config = getSizeConfig(size)
  const IconComponent = getSliderIcon(label, icon)

  // Calculate percentage for progress
  const percentage = ((value - min) / (max - min)) * 100

  // Handle value change
  const handleChange = useCallback(async (newValue: number) => {
    setLocalValue(newValue)
    if (isCommandSource && sendCommand) {
      await sendCommand(newValue)
    }
    onValueChange?.(newValue)
  }, [isCommandSource, sendCommand, onValueChange])

  // Increment/decrement handlers
  const handleIncrement = useCallback(() => {
    const newValue = Math.min(value + step, max)
    handleChange(newValue)
  }, [value, step, max, handleChange])

  const handleDecrement = useCallback(() => {
    const newValue = Math.max(value - step, min)
    handleChange(newValue)
  }, [value, step, min, handleChange])

  // Compact variant (horizontal bar)
  if (variant === 'compact') {
    return (
      <div className={cn(
        dashboardCardBase,
        'flex-row items-center gap-3',
        config.padding,
        className
      )}>
        {/* Icon */}
        <div className={cn(
          'flex items-center justify-center rounded-full shrink-0',
          'bg-primary/10 text-primary',
          config.iconSize
        )}>
          {loading ? (
            <Skeleton className={cn(config.iconSize, 'rounded-full')} />
          ) : (
            <IconComponent className={cn('w-1/2 h-1/2', isDragging && 'scale-110 transition-transform')} />
          )}
        </div>

        {/* Label and value */}
        <div className="flex-1 min-w-0">
          {label && (
            <p className={cn('font-medium text-foreground/90 truncate', config.labelText)}>
              {label}
            </p>
          )}
          {description && (
            <p className={cn('text-muted-foreground text-xs truncate', config.labelText)}>
              {description}
            </p>
          )}
        </div>

        {/* Value display */}
        <div className="flex items-center gap-2">
          {showButtons && !disabled && !loading && !sending && (
            <Button
              variant="ghost"
              size="icon"
              className="h-7 w-7 shrink-0"
              onClick={handleDecrement}
              disabled={value <= min || sending}
            >
              <Minus className="h-3 w-3" />
            </Button>
          )}
          <span className={cn(
            'font-semibold tabular-nums min-w-[3ch] text-center',
            config.valueText
          )}>
            {loading ? <Skeleton className="h-6 w-12 mx-auto" /> : Math.round(value)}
          </span>
          {unit && <span className="text-sm text-muted-foreground">{unit}</span>}
          {showButtons && !disabled && !loading && !sending && (
            <Button
              variant="ghost"
              size="icon"
              className="h-7 w-7 shrink-0"
              onClick={handleIncrement}
              disabled={value >= max || sending}
            >
              <Plus className="h-3 w-3" />
            </Button>
          )}
        </div>
      </div>
    )
  }

  // Inline variant (slider only, minimal UI)
  if (variant === 'inline') {
    return (
      <div className={cn('space-y-2', className)}>
        {(label || description) && (
          <div className="flex items-center justify-between">
            <div>
              {label && <p className="text-sm font-medium text-foreground">{label}</p>}
              {description && <p className="text-xs text-muted-foreground">{description}</p>}
            </div>
            <div className="flex items-center gap-1">
              <span className="text-lg font-semibold tabular-nums">{Math.round(value)}</span>
              {unit && <span className="text-xs text-muted-foreground">{unit}</span>}
            </div>
          </div>
        )}
        <div className="relative">
          {loading ? (
            <Skeleton className="h-6 w-full rounded-full" />
          ) : (
            <SliderPrimitive
              value={[value]}
              onValueChange={([v]) => handleChange(v)}
              min={min}
              max={max}
              step={step}
              disabled={disabled || loading || sending}
              className={cn(
                sending && 'opacity-50'
              )}
            />
          )}
        </div>
      </div>
    )
  }

  // Default variant (vertical card with visual feedback)
  return (
    <div className={cn(
      dashboardCardBase,
      'flex-col items-center justify-center',
      config.padding,
      className
    )}>
      {/* Icon with animated background */}
      <div className="relative mb-3">
        <div
          className={cn(
            'absolute inset-0 rounded-full transition-all duration-300',
            'bg-primary/10',
            isDragging && 'bg-primary/20 scale-110'
          )}
          style={{
            transform: `scale(${0.8 + (percentage / 100) * 0.2})`,
          }}
        />
        <div className={cn(
          'relative flex items-center justify-center rounded-full',
          'bg-background shadow-sm',
          config.iconSize
        )}>
          {loading ? (
            <Skeleton className={cn(config.iconSize, 'rounded-full')} />
          ) : (
            <div
              className={cn(
                'w-1/2 h-1/2 transition-all duration-200',
                isDragging && 'scale-110'
              )}
              style={{
                opacity: 0.4 + (percentage / 100) * 0.6,
              }}
            >
              <IconComponent className="w-full h-full" />
            </div>
          )}
        </div>
      </div>

      {/* Value display */}
      {loading ? (
        <Skeleton className="h-8 w-20 mb-1" />
      ) : (
        <div className="flex items-baseline gap-1 mb-1">
          <span className={cn(
            'font-bold tabular-nums',
            config.valueText
          )}>
            {Math.round(value)}
          </span>
          {unit && (
            <span className="text-sm text-muted-foreground">{unit}</span>
          )}
        </div>
      )}

      {/* Label */}
      {label && (
        <p className={cn('font-medium text-foreground/90 text-center mb-2', config.labelText)}>
          {label}
        </p>
      )}

      {/* Description */}
      {description && !loading && (
        <p className={cn('text-muted-foreground/70 text-center text-xs mb-3', config.labelText)}>
          {description}
        </p>
      )}

      {/* Slider */}
      {loading ? (
        <Skeleton className="h-2 w-full rounded-full" />
      ) : (
        <div className="w-full px-2">
          <SliderPrimitive
            value={[value]}
            onValueChange={([v]) => handleChange(v)}
            onPointerDown={() => setIsDragging(true)}
            onPointerUp={() => setIsDragging(false)}
            min={min}
            max={max}
            step={step}
            disabled={disabled || loading || sending}
            className={cn(
              sending && 'opacity-50'
            )}
          />
        </div>
      )}

      {/* Quick action buttons */}
      {showButtons && !disabled && !loading && !sending && (
        <div className="flex items-center gap-2 mt-3">
          <Button
            variant="outline"
            size="icon"
            className="h-7 w-7 rounded-full"
            onClick={handleDecrement}
            disabled={value <= min}
          >
            <Minus className="h-3 w-3" />
          </Button>
          <div className="flex-1 h-1 bg-muted rounded-full overflow-hidden">
            <div
              className="h-full bg-primary transition-all duration-150"
              style={{ width: `${percentage}%` }}
            />
          </div>
          <Button
            variant="outline"
            size="icon"
            className="h-7 w-7 rounded-full"
            onClick={handleIncrement}
            disabled={value >= max}
          >
            <Plus className="h-3 w-3" />
          </Button>
        </div>
      )}
    </div>
  )
}
