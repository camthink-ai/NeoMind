/**
 * LED Indicator Component (Unified Styles)
 *
 * Fills 100% of container using unified dashboard styles.
 * Size prop controls relative scale.
 */

import { cn } from '@/lib/utils'
import { useDataSource } from '@/hooks/useDataSource'
import { Skeleton } from '@/components/ui/skeleton'
import { dashboardComponentSize, dashboardCardBase } from '@/design-system/tokens/size'
import type { DataSource } from '@/types/dashboard'

export type LEDState = 'on' | 'off' | 'blinking' | 'error' | 'warning'

export interface LEDIndicatorProps {
  dataSource?: DataSource
  state?: LEDState
  label?: string
  size?: 'sm' | 'md' | 'lg'
  color?: string
  errorColor?: string
  warningColor?: string
  showCard?: boolean
  variant?: 'dot' | 'pill' | 'badge' | 'card'
  showGlow?: boolean
  className?: string
}

// Size configuration
const getIconSize = (size: 'sm' | 'md' | 'lg') => {
  const config = dashboardComponentSize[size]
  return {
    dot: size === 'sm' ? 'h-2 w-2' : size === 'md' ? 'h-2.5 w-2.5' : 'h-3 w-3',
    dotWithGlow: size === 'sm' ? 'h-3 w-3' : size === 'md' ? 'h-3.5 w-3.5' : 'h-4 w-4',
    ...config,
  }
}

// State colors with glow effects
const getStateColors = (state: LEDState, customColor?: string, errorColor?: string, warningColor?: string) => {
  if (customColor && state === 'on') {
    return {
      bg: customColor,
      glow: `0 0 12px ${customColor}80, 0 0 24px ${customColor}40`,
      text: '',
      border: '',
      ring: `ring-${customColor}/30`,
    }
  }

  const colors = {
    on: {
      bg: 'bg-emerald-500',
      glow: '0 0 12px rgba(16, 185, 129, 0.5), 0 0 24px rgba(16, 185, 129, 0.25)',
      text: 'text-emerald-600 dark:text-emerald-400',
      border: 'border-emerald-500/30',
      ring: 'ring-emerald-500/20',
    },
    off: {
      bg: 'bg-zinc-400 dark:bg-zinc-600',
      glow: 'none',
      text: 'text-zinc-500 dark:text-zinc-500',
      border: 'border-zinc-300 dark:border-zinc-700',
      ring: 'transparent',
    },
    blinking: {
      bg: 'bg-emerald-500',
      glow: '0 0 12px rgba(16, 185, 129, 0.5), 0 0 24px rgba(16, 185, 129, 0.25)',
      text: 'text-emerald-600 dark:text-emerald-400',
      border: 'border-emerald-500/30',
      ring: 'ring-emerald-500/20',
    },
    error: {
      bg: errorColor ? errorColor : 'bg-rose-500',
      glow: errorColor ? `0 0 12px ${errorColor}80` : '0 0 12px rgba(244, 63, 94, 0.5), 0 0 24px rgba(244, 63, 94, 0.25)',
      text: errorColor ? '' : 'text-rose-600 dark:text-rose-400',
      border: errorColor ? '' : 'border-rose-500/30',
      ring: errorColor ? '' : 'ring-rose-500/20',
    },
    warning: {
      bg: warningColor ? warningColor : 'bg-amber-500',
      glow: warningColor ? `0 0 12px ${warningColor}80` : '0 0 12px rgba(245, 158, 11, 0.5), 0 0 24px rgba(245, 158, 11, 0.25)',
      text: warningColor ? '' : 'text-amber-600 dark:text-amber-400',
      border: warningColor ? '' : 'border-amber-500/30',
      ring: warningColor ? '' : 'ring-amber-500/20',
    },
  }

  return colors[state] || colors.off
}

const stateLabels = {
  on: '开启',
  off: '关闭',
  blinking: '运行中',
  error: '错误',
  warning: '警告',
}

export function LEDIndicator({
  dataSource,
  state = 'on',
  label,
  size = 'md',
  color,
  errorColor,
  warningColor,
  showCard = true,
  variant = 'badge',
  showGlow = true,
  className,
}: LEDIndicatorProps) {
  const { data, loading, error } = useDataSource<LEDState>(dataSource, { fallback: state })
  const ledState = error ? 'off' : data ?? state
  const config = getIconSize(size)

  const colorConfig = getStateColors(ledState, color, errorColor, warningColor)
  const isBlinking = ledState === 'blinking'
  const isOn = ledState === 'on' || ledState === 'blinking'

  // Dot variant - inline (used inside other components)
  if (variant === 'dot') {
    return (
      <div className={cn('inline-flex items-center gap-2 overflow-hidden', className)}>
        <div
          className={cn(
            'rounded-full transition-all duration-300 shrink-0',
            isBlinking && 'animate-pulse',
            isOn && showGlow && 'shadow-[0_0_8px_currentColor]',
            colorConfig.bg
          )}
          style={{
            backgroundColor: (ledState === 'on' || ledState === 'blinking') && color ? color : undefined,
            boxShadow: isOn && showGlow ? colorConfig.glow : undefined,
          }}
        />
        {label && <span className={cn('text-muted-foreground', config.labelText)}>{label}</span>}
      </div>
    )
  }

  // Pill variant - horizontal pill (inline)
  if (variant === 'pill') {
    return (
      <div
        className={cn(
          'inline-flex items-center gap-2 rounded-full border transition-all duration-300 overflow-hidden',
          'px-3 py-1.5',
          colorConfig.border,
          isBlinking && 'animate-pulse',
          ledState === 'off' && 'bg-muted/40',
        )}
        style={{
          backgroundColor: isOn ? undefined : undefined,
        }}
      >
        <div
          className={cn('rounded-full shrink-0', config.dot, colorConfig.bg)}
          style={{
            backgroundColor: isOn && color ? color : undefined,
            boxShadow: isOn && showGlow ? colorConfig.glow : undefined,
          }}
        />
        {label && <span className={cn('font-medium text-sm', colorConfig.text || 'text-foreground')}>{label}</span>}
      </div>
    )
  }

  // Badge variant - fills container with icon background
  if (variant === 'badge') {
    if (loading) {
      return (
        <div className={cn(dashboardCardBase, 'items-center', config.padding, className)}>
          <div className="flex items-center gap-3 w-full h-full min-w-0">
            <Skeleton className={cn('rounded-full shrink-0', config.iconContainer)} />
            <div className="flex flex-col min-w-0 flex-1 gap-2">
              <Skeleton className={cn('h-4 w-20 rounded')} />
              <Skeleton className={cn('h-3 w-12 rounded')} />
            </div>
          </div>
        </div>
      )
    }

    // Get background color with proper opacity for both themes
    const getContainerBg = () => {
      if (ledState === 'off') return undefined
      if (color) return color + '20' // Custom color with ~12% opacity
      // Use theme-aware colors
      const stateBgMap = {
        on: 'rgba(16, 185, 129, 0.12)',    // emerald
        blinking: 'rgba(16, 185, 129, 0.12)',
        error: errorColor || 'rgba(244, 63, 94, 0.12)',  // rose
        warning: warningColor || 'rgba(245, 158, 11, 0.12)', // amber
      }
      return stateBgMap[ledState] || stateBgMap.on
    }

    const content = (
      <div className="flex items-center gap-3 w-full h-full min-w-0">
        {/* LED indicator */}
        <div
          className={cn(
            'flex items-center justify-center rounded-full shrink-0 transition-all duration-300',
            config.iconContainer,
            ledState === 'off' ? 'bg-muted/50' : '',
            isBlinking && 'animate-pulse',
          )}
          style={ledState !== 'off' ? { backgroundColor: getContainerBg() } : undefined}
        >
          <div
            className={cn('rounded-full transition-all duration-300', config.dotWithGlow, colorConfig.bg)}
            style={{
              backgroundColor: isOn && color ? color : undefined,
              boxShadow: isOn && showGlow ? colorConfig.glow : undefined,
            }}
          />
        </div>
        <div className="flex flex-col min-w-0 flex-1 overflow-hidden">
          {label && <span className={cn('font-medium text-foreground truncate', config.titleText)}>{label}</span>}
          <span className={cn(colorConfig.text || 'text-muted-foreground', config.labelText)}>
            {stateLabels[ledState]}
          </span>
        </div>
      </div>
    )

    if (showCard) {
      return (
        <div className={cn(dashboardCardBase, 'items-center', config.padding, className)}>
          {content}
        </div>
      )
    }

    return <div className={cn('flex items-center w-full h-full overflow-hidden', config.padding, className)}>{content}</div>
  }

  // Card variant - centered card with larger LED
  if (variant === 'card') {
    if (loading) {
      return (
        <div className={cn(dashboardCardBase, 'flex flex-col items-center justify-center gap-3', config.padding, className)}>
          <Skeleton className={cn('rounded-full', size === 'sm' ? 'w-16 h-16' : size === 'md' ? 'w-20 h-20' : 'w-24 h-24')} />
          <div className="flex flex-col items-center gap-2">
            <Skeleton className={cn('h-5 w-16 rounded')} />
            <Skeleton className={cn('h-4 w-12 rounded')} />
          </div>
        </div>
      )
    }

    // Get background and border colors for both themes
    const getCardStyles = () => {
      if (ledState === 'off') {
        return {
          bg: undefined,
          border: 'hsl(var(--border))',
        }
      }
      if (color) {
        return {
          bg: color + '15', // ~8% opacity
          border: color + '40', // ~25% opacity
        }
      }
      const stateStyleMap = {
        on: { bg: 'rgba(16, 185, 129, 0.08)', border: 'rgba(16, 185, 129, 0.25)' },
        blinking: { bg: 'rgba(16, 185, 129, 0.08)', border: 'rgba(16, 185, 129, 0.25)' },
        error: { bg: errorColor ? errorColor + '15' : 'rgba(244, 63, 94, 0.08)',
                 border: errorColor || 'rgba(244, 63, 94, 0.25)' },
        warning: { bg: warningColor ? warningColor + '15' : 'rgba(245, 158, 11, 0.08)',
                  border: warningColor || 'rgba(245, 158, 11, 0.25)' },
      }
      return stateStyleMap[ledState] || stateStyleMap.on
    }

    const cardStyles = getCardStyles()

    return (
      <div className={cn(dashboardCardBase, 'flex flex-col items-center justify-center gap-3', config.padding, className)}>
        {/* Large LED indicator */}
        <div
          className={cn(
            'flex items-center justify-center rounded-full transition-all duration-300',
            size === 'sm' ? 'w-16 h-16' : size === 'md' ? 'w-20 h-20' : 'w-24 h-24',
            ledState === 'off' && 'bg-muted/40',
            isBlinking && 'animate-pulse',
          )}
          style={ledState !== 'off' ? {
            backgroundColor: cardStyles.bg,
            border: '2px solid',
            borderColor: cardStyles.border,
          } : undefined}
        >
          <div
            className={cn(
              'rounded-full transition-all duration-300',
              size === 'sm' ? 'h-6 w-6' : size === 'md' ? 'h-8 w-8' : 'h-10 w-10',
              colorConfig.bg
            )}
            style={{
              backgroundColor: isOn && color ? color : undefined,
              boxShadow: isOn && showGlow ? colorConfig.glow : undefined,
            }}
          />
        </div>

        {/* Label and state */}
        <div className="flex flex-col items-center text-center">
          {label && <span className={cn('font-semibold text-foreground', config.titleText)}>{label}</span>}
          <span className={cn(colorConfig.text || 'text-muted-foreground', config.labelText)}>
            {stateLabels[ledState]}
          </span>
        </div>
      </div>
    )
  }

  return null
}
