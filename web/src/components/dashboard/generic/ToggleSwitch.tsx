/**
 * Toggle Switch Component
 *
 * Clickable power button following LED Indicator layout.
 * Horizontal: LED/icon on left, label on right. Click anywhere to toggle.
 */

import { Power, Lightbulb, Fan, Lock } from 'lucide-react'
import { cn } from '@/lib/utils'
import { useDataSource } from '@/hooks/useDataSource'
import { Skeleton } from '@/components/ui/skeleton'
import { dashboardCardBase, dashboardComponentSize } from '@/design-system/tokens/size'
import { indicatorFontWeight } from '@/design-system/tokens/indicator'
import type { DataSource } from '@/types/dashboard'

export interface ToggleSwitchProps {
  dataSource?: DataSource
  checked?: boolean
  onCheckedChange?: (checked: boolean) => void
  label?: string
  size?: 'sm' | 'md' | 'lg'
  disabled?: boolean
  className?: string
}

// Get icon based on label
function getIconForLabel(label?: string): React.ComponentType<{ className?: string }> {
  if (!label) return Power
  const lower = label.toLowerCase()
  if (lower.includes('light') || lower.includes('lamp')) return Lightbulb
  if (lower.includes('fan')) return Fan
  if (lower.includes('lock')) return Lock
  return Power
}

export function ToggleSwitch({
  dataSource,
  checked: propChecked = false,
  onCheckedChange,
  label,
  size = 'md',
  disabled = false,
  className,
}: ToggleSwitchProps) {
  const { data, loading, sendCommand, sending } = useDataSource<boolean>(dataSource, {
    fallback: propChecked,
  })
  const checked = data ?? propChecked
  const isCommandSource = dataSource?.type === 'command'

  const handleClick = async () => {
    if (disabled || loading || sending) return

    const newChecked = !checked
    if (isCommandSource && sendCommand) {
      await sendCommand(newChecked)
    }
    onCheckedChange?.(newChecked)
  }

  const config = dashboardComponentSize[size]
  const Icon = getIconForLabel(label)

  // Loading state
  if (loading) {
    return (
      <div className={cn(dashboardCardBase, 'flex-row items-center', config.contentGap, config.padding, className)}>
        <Skeleton className={cn(config.iconContainer, 'rounded-full')} />
        <Skeleton className={cn('h-4 w-20 rounded')} />
      </div>
    )
  }

  return (
    <button
      onClick={handleClick}
      disabled={disabled || loading || sending}
      className={cn(
        dashboardCardBase,
        'flex-row items-center',
        config.contentGap,
        config.padding,
        'transition-all duration-200',
        'hover:bg-accent/50',
        (disabled || sending) && 'opacity-50 cursor-not-allowed',
        className
      )}
    >
      {/* Icon Section - left side */}
      <div className={cn(
        'flex items-center justify-center shrink-0 rounded-full transition-all duration-300',
        config.iconContainer,
        checked
          ? 'bg-primary text-primary-foreground shadow-md'
          : 'bg-muted/50 text-muted-foreground'
      )}>
        <Icon className={cn(config.iconSize, checked ? 'opacity-100' : 'opacity-50')} />
      </div>

      {/* Label section - right side */}
      <div className="flex flex-col min-w-0 flex-1 text-left">
        {label ? (
          <span className={cn(indicatorFontWeight.title, 'text-foreground truncate', config.titleText)}>
            {label}
          </span>
        ) : (
          <span className={cn(indicatorFontWeight.title, 'text-foreground', config.titleText)}>
            {checked ? '已开启' : '已关闭'}
          </span>
        )}
        {label && (
          <span className={cn(indicatorFontWeight.label, 'text-muted-foreground', config.labelText)}>
            {checked ? '已开启' : '已关闭'}
          </span>
        )}
      </div>

      {/* Sending indicator */}
      {sending && (
        <span className="absolute top-3 right-3 w-2 h-2 rounded-full bg-amber-500 animate-pulse" />
      )}
    </button>
  )
}
