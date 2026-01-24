/**
 * Toggle Switch Component (Unified Styles)
 *
 * Fills 100% of container using unified dashboard styles.
 * Size prop controls relative scale.
 */

import { Check, Power, ToggleRight, ToggleLeft, Lightbulb, Fan, Lock, Unlock, Zap, DoorOpen } from 'lucide-react'
import { cn, getIconForEntity } from '@/lib/utils'
import { useDataSource } from '@/hooks/useDataSource'
import { Skeleton } from '@/components/ui/skeleton'
import { dashboardComponentSize, dashboardCardBase } from '@/design-system/tokens/size'
import type { DataSource } from '@/types/dashboard'

export interface ToggleSwitchProps {
  dataSource?: DataSource
  checked?: boolean
  onCheckedChange?: (checked: boolean) => void
  label?: string
  description?: string
  size?: 'sm' | 'md' | 'lg'
  variant?: 'default' | 'icon' | 'slider' | 'pill'
  trueLabel?: string
  falseLabel?: string
  trueIcon?: string
  falseIcon?: string
  disabled?: boolean
  showCard?: boolean
  className?: string
}

// Use unified size configuration
const getToggleSizeConfig = (size: 'sm' | 'md' | 'lg') => {
  const config = dashboardComponentSize[size]
  return {
    track: size === 'sm' ? 'h-5 w-10' : size === 'md' ? 'h-6 w-12' : 'h-7 w-14',
    thumb: size === 'sm' ? 'w-3 h-3' : size === 'md' ? 'w-3.5 h-3.5' : 'w-4 h-4',
    icon: config.labelText,
    padding: config.padding,
    iconContainer: config.iconContainer,
  }
}

// Get icon for entity type
function getEntityIcon(label?: string, checked = false): React.ComponentType<{ className?: string }> {
  if (!label) return Power
  const lower = label.toLowerCase()
  if (lower.includes('light') || lower.includes('lamp') || lower.includes('bulb')) return Lightbulb
  if (lower.includes('fan')) return Fan
  if (lower.includes('lock')) return checked ? Lock : Unlock
  if (lower.includes('switch') || lower.includes('plug') || lower.includes('power')) return Power
  if (lower.includes('door')) return DoorOpen
  return Power
}

export function ToggleSwitch({
  dataSource,
  checked: propChecked = false,
  onCheckedChange,
  label,
  description,
  size = 'md',
  variant = 'default',
  trueLabel = 'On',
  falseLabel = 'Off',
  trueIcon,
  falseIcon,
  disabled = false,
  showCard = true,
  className,
}: ToggleSwitchProps) {
  const { data, loading, sendCommand, sending } = useDataSource<boolean>(dataSource, { fallback: propChecked })
  const checked = data ?? propChecked
  const isCommandSource = dataSource?.type === 'command'

  const handleChange = async (newChecked: boolean) => {
    if (isCommandSource && sendCommand) {
      await sendCommand(newChecked)
    }
    onCheckedChange?.(newChecked)
  }

  const config = getToggleSizeConfig(size)
  const OnIcon = trueIcon ? getIconForEntity(trueIcon) : getEntityIcon(label, true)
  const OffIcon = falseIcon ? getIconForEntity(falseIcon) : getEntityIcon(label, false)

  // Icon variant - just the clickable button
  if (variant === 'icon') {
    return (
      <button
        onClick={() => !disabled && !loading && !sending && handleChange(!checked)}
        disabled={disabled || loading || sending}
        className={cn(
          'relative inline-flex items-center justify-center rounded-2xl transition-all duration-200 overflow-hidden',
          'hover:scale-105 active:scale-95 w-full h-full',
          config.padding,
          checked
            ? 'bg-primary/10 text-primary shadow-md'
            : 'bg-muted/30 text-muted-foreground',
          (disabled || loading || sending) && 'opacity-50 cursor-not-allowed',
          className
        )}
      >
        {loading ? (
          <Skeleton className="w-8 h-8 rounded-full" />
        ) : (
          <div className="w-10 h-10 flex items-center justify-center">
            {checked ? <OnIcon className="w-full h-full" /> : <OffIcon className="w-full h-full opacity-60" />}
          </div>
        )}
        {checked && (
          <div className="absolute -bottom-1 -right-1 w-5 h-5 rounded-full border-2 border-background bg-emerald-500 flex items-center justify-center">
            <Check className="h-3 w-3 text-white m-auto" />
          </div>
        )}
      </button>
    )
  }

  // Slider toggle (horizontal)
  if (variant === 'slider') {
    return (
      <div className={cn(dashboardCardBase, 'flex-row items-center justify-center', config.padding, className)}>
        <div className="flex items-center justify-between w-full gap-3">
          <div className="flex items-center gap-2 flex-1 min-w-0">
            <div className="w-8 h-8 flex items-center justify-center flex-shrink-0">
              {checked ? <OnIcon className="w-full h-full opacity-70" /> : <OffIcon className="w-full h-full opacity-40" />}
            </div>
            <div className="min-w-0">
              {label && <p className={cn('font-medium text-foreground/90 truncate', config.icon)}>{label}</p>}
              {description && <p className={cn('text-muted-foreground truncate', config.icon)}>{description}</p>}
            </div>
          </div>
          <button
            onClick={() => !disabled && !loading && !sending && handleChange(!checked)}
            disabled={disabled || loading || sending}
            className={cn(
              'relative inline-flex shrink-0 rounded-full border-2 transition-all duration-200',
              config.track,
              checked
                ? 'bg-primary border-primary shadow-[0_0_12px_currentColor]'
                : 'bg-muted border-muted-foreground/30',
              (disabled || loading || sending) && 'opacity-50 cursor-not-allowed',
            )}
          >
            <span
              className={cn(
                'pointer-events-none inline-block rounded-full bg-background shadow-md transition-transform duration-200 absolute top-1/2 -translate-y-1/2',
                config.thumb
              )}
              style={{ transform: checked ? 'translateX(100%)' : 'translateX(0)' }}
            />
          </button>
        </div>
      </div>
    )
  }

  // Pill toggle
  if (variant === 'pill') {
    return (
      <div className={cn(dashboardCardBase, 'flex-row items-center justify-center', config.padding, className)}>
        <div className="flex items-center justify-between w-full gap-3">
          <div className="flex items-center gap-2 flex-1 min-w-0">
            <div className="w-8 h-8 flex items-center justify-center flex-shrink-0">
              {checked ? <OnIcon className="w-full h-full opacity-70" /> : <OffIcon className="w-full h-full opacity-40" />}
            </div>
            {label && <span className={cn('font-medium text-foreground/90 truncate', config.icon)}>{label}</span>}
          </div>
          <button
            onClick={() => !disabled && !loading && !sending && handleChange(!checked)}
            disabled={disabled || loading || sending}
            className={cn(
              'inline-flex items-center gap-1 px-3 py-1.5 rounded-full text-sm font-medium transition-all duration-200 shrink-0',
              checked
                ? 'bg-primary text-primary-foreground shadow-md'
                : 'bg-muted text-muted-foreground hover:bg-muted/80',
              (disabled || loading || sending) && 'opacity-50 cursor-not-allowed'
            )}
          >
            <span>{checked ? trueLabel : falseLabel}</span>
          </button>
        </div>
      </div>
    )
  }

  // Default vertical card (clickable)
  return (
    <button
      className={cn(
        dashboardCardBase,
        'flex-col items-center justify-center cursor-pointer',
        'hover:shadow-md active:scale-[0.98] transition-all duration-200',
        config.padding,
        (disabled || loading || sending) && 'opacity-50 cursor-not-allowed',
        className
      )}
      onClick={() => !disabled && !loading && !sending && handleChange(!checked)}
    >
      {/* Icon */}
      {loading ? (
        <Skeleton className="w-10 h-10 rounded-full" />
      ) : (
        <div className="w-12 h-12 flex items-center justify-center transition-all duration-200">
          <div className="w-full h-full flex items-center justify-center" style={{ transform: checked ? 'scale(1.1)' : 'scale(1)', opacity: checked ? 1 : 0.6 }}>
            {checked ? <OnIcon className="w-full h-full text-primary" /> : <OffIcon className="w-full h-full text-muted-foreground" />}
          </div>
        </div>
      )}

      {/* Label */}
      {label && (
        <span className={cn('font-medium text-foreground/90 text-center truncate w-full mt-2', config.icon)}>
          {label}
        </span>
      )}

      {/* Status */}
      {loading ? (
        <Skeleton className="h-5 w-12 rounded mt-1" />
      ) : (
        <span
          className={cn(
            'mt-2 px-3 py-1 rounded-full text-center',
            config.icon,
            checked
              ? 'bg-emerald-500/15 text-emerald-600 dark:text-emerald-400'
              : 'bg-muted/50 text-muted-foreground'
          )}
        >
          {checked ? trueLabel : falseLabel}
        </span>
      )}

      {/* Description */}
      {description && !loading && (
        <span className={cn('text-muted-foreground/70 text-center truncate w-full mt-1', config.icon)}>
          {description}
        </span>
      )}
    </button>
  )
}
