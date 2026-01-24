/**
 * Button Group Component (Unified Styles)
 *
 * Home Assistant inspired button group with segmented controls.
 * Clean design with smooth transitions and visual feedback.
 */

import { Card } from '@/components/ui/card'
import { Skeleton } from '@/components/ui/skeleton'
import { cn } from '@/lib/utils'
import { useDataSource } from '@/hooks/useDataSource'
import { dashboardComponentSize, dashboardCardBase } from '@/design-system/tokens/size'
import type { DataSource } from '@/types/dashboard'

export interface ButtonOption {
  value: string | number
  label: string
  icon?: string
  description?: string
  disabled?: boolean
  variant?: 'default' | 'outline' | 'ghost' | 'destructive'
  color?: string
}

export interface ButtonGroupProps {
  dataSource?: DataSource
  value?: string | number
  onValueChange?: (value: string | number) => void
  options: ButtonOption[]
  label?: string
  orientation?: 'horizontal' | 'vertical'
  size?: 'default' | 'sm' | 'lg' | 'icon'
  variant?: 'default' | 'segmented' | 'pills' | 'grid'
  multiple?: boolean
  showCard?: boolean
  className?: string
}

// Use unified size configuration
const getButtonSizeConfig = (size: 'default' | 'sm' | 'lg' | 'icon') => {
  // Map to unified dashboard size tokens
  const unifiedSize = size === 'icon' ? 'sm' : size === 'default' ? 'md' : size
  const config = dashboardComponentSize[unifiedSize as keyof typeof dashboardComponentSize]
  
  return {
    button: size === 'icon' ? 'h-12 w-12' : size === 'sm' ? 'h-9 px-3' : size === 'lg' ? 'h-11 px-5' : 'h-10 px-4',
    icon: size === 'icon' ? 'text-2xl' : size === 'lg' ? 'text-2xl' : 'text-xl',
    text: config.labelText,
    padding: config.padding,
  }
}

const sizeConfig = {
  default: { button: 'h-10 px-4', icon: 'text-xl', text: 'text-sm', padding: 'p-4' },
  sm: { button: 'h-9 px-3', icon: 'text-lg', text: 'text-xs', padding: 'p-3' },
  lg: { button: 'h-11 px-5', icon: 'text-2xl', text: 'text-base', padding: 'p-5' },
  icon: { button: 'h-12 w-12', icon: 'text-2xl', text: 'text-sm', padding: 'p-3' },
}

export function ButtonGroup({
  dataSource,
  value: propValue,
  onValueChange,
  options,
  label,
  orientation = 'horizontal',
  size = 'default',
  variant = 'segmented',
  multiple = false,
  showCard = true,
  className,
}: ButtonGroupProps) {
  const { data, loading, sendCommand, sending } = useDataSource<string | number>(dataSource, {
    fallback: propValue,
  })
  const value = data ?? propValue
  const isCommandSource = dataSource?.type === 'command'

  const handleClick = async (optionValue: string | number) => {
    if (isCommandSource && sendCommand) {
      await sendCommand(optionValue)
    }
    onValueChange?.(optionValue)
  }

  const config = sizeConfig[size]

  // Render individual button
  const renderButton = (option: ButtonOption, isActive: boolean, isFirst: boolean, isLast: boolean) => {
    const IconDisplay = option.icon ? (
      <span className={cn(config.icon, 'opacity-70')}>{option.icon}</span>
    ) : null

    // Segmented style (HA default)
    if (variant === 'segmented') {
      return (
        <button
          key={option.value}
          onClick={() => !option.disabled && !loading && !sending && handleClick(option.value)}
          disabled={option.disabled || loading || sending}
          className={cn(
            'relative flex-1 flex items-center justify-center gap-2 font-medium transition-all duration-200',
            config.button,
            config.text,
            // Border radius based on position
            isFirst && 'rounded-l-lg rounded-r-none',
            isLast && 'rounded-r-lg rounded-l-none',
            !isFirst && !isLast && 'rounded-none',
            // Active/inactive states
            isActive
              ? 'bg-primary text-primary-foreground shadow-sm'
              : 'bg-muted/50 text-muted-foreground hover:bg-muted',
            (option.disabled || loading || sending) && 'opacity-50 cursor-not-allowed'
          )}
        >
          {IconDisplay}
          {option.label && <span>{option.label}</span>}
        </button>
      )
    }

    // Pills style
    if (variant === 'pills') {
      return (
        <button
          key={option.value}
          onClick={() => !option.disabled && !loading && !sending && handleClick(option.value)}
          disabled={option.disabled || loading || sending}
          className={cn(
            'flex items-center gap-2 px-4 py-2 rounded-full font-medium transition-all duration-200',
            config.text,
            isActive
              ? 'bg-primary text-primary-foreground shadow-md'
              : 'bg-muted/50 text-muted-foreground hover:bg-muted',
            (option.disabled || loading || sending) && 'opacity-50 cursor-not-allowed'
          )}
        >
          {IconDisplay}
          <span>{option.label}</span>
        </button>
      )
    }

    // Grid style (for mushroom-like cards)
    if (variant === 'grid') {
      return (
        <button
          key={option.value}
          onClick={() => !option.disabled && !loading && !sending && handleClick(option.value)}
          disabled={option.disabled || loading || sending}
          className={cn(
            'flex flex-col items-center justify-center gap-2 p-3 rounded-xl transition-all duration-200',
            isActive
              ? 'bg-primary/10 text-primary shadow-sm scale-105'
              : 'bg-muted/30 text-muted-foreground hover:bg-muted/50 hover:scale-105',
            (option.disabled || loading || sending) && 'opacity-50 cursor-not-allowed'
          )}
        >
          {IconDisplay || <span className={cn(config.icon, 'opacity-50')}>•</span>}
          <span className={cn('font-medium', config.text)}>{option.label}</span>
        </button>
      )
    }

    // Default button style
    return (
      <button
        key={option.value}
        onClick={() => !option.disabled && !loading && !sending && handleClick(option.value)}
        disabled={option.disabled || loading || sending}
        className={cn(
          'flex items-center gap-2 font-medium transition-all duration-200',
          config.button,
          config.text,
          isActive
            ? 'bg-primary text-primary-foreground shadow-md'
            : 'bg-card border border-border hover:bg-muted',
          'rounded-lg',
          (option.disabled || loading || sending) && 'opacity-50 cursor-not-allowed'
        )}
        style={
          option.color && isActive
            ? ({ backgroundColor: option.color } as React.CSSProperties)
            : undefined
        }
      >
        {IconDisplay}
        <span>{option.label}</span>
      </button>
    )
  }

  // HA-style segmented control container
  const segmentedContent = (
    <div className={cn('w-full space-y-3', className)}>
      {label && <p className={cn('font-medium text-foreground/90', config.text)}>{label}</p>}

      <div
        className={cn(
          'flex',
          orientation === 'horizontal' ? 'flex-row' : 'flex-col',
          variant === 'pills' && 'flex-wrap gap-2',
          variant === 'grid' && 'grid grid-cols-2 sm:grid-cols-3 gap-2'
        )}
      >
        {options.map((option, index) => {
          const isActive = multiple
            ? Array.isArray(value)
              ? value.includes(option.value)
              : false
            : value === option.value

          if (variant === 'segmented') {
            return (
              <div key={option.value} className="flex-1">
                {renderButton(
                  option,
                  isActive,
                  index === 0,
                  index === options.length - 1
                )}
              </div>
            )
          }

          return (
            <div key={option.value}>
              {renderButton(option, isActive, false, false)}
            </div>
          )
        })}
      </div>

      {/* Description */}
      {options.find((o) => o.description) && value !== undefined && (
        <p className={cn('text-muted-foreground', config.text)}>
          {options.find((o) => o.value === value)?.description}
        </p>
      )}
    </div>
  )

  // No card variant
  if (!showCard) {
    return segmentedContent
  }

  // HA-style card with segmented control
  if (variant === 'segmented') {
    return (
      <Card className="dashboardCardBase">
        <div className={cn(config.padding)}>
          {label && <p className={cn('font-medium text-foreground/90 mb-3', config.text)}>{label}</p>}

          {loading ? (
            <Skeleton className="h-10 w-full" />
          ) : (
            <div className="flex rounded-lg overflow-hidden border border-border/50">
              {options.map((option, index) => {
                const isActive = value === option.value
                return (
                  <button
                    key={option.value}
                    onClick={() => !option.disabled && !loading && !sending && handleClick(option.value)}
                    disabled={option.disabled || loading || sending}
                    className={cn(
                      'flex-1 flex items-center justify-center gap-2 py-2.5 transition-all duration-200',
                      config.text,
                      isActive
                        ? 'bg-primary text-primary-foreground font-semibold'
                        : 'bg-muted/30 text-muted-foreground hover:bg-muted/50',
                      (option.disabled || loading || sending) && 'opacity-50 cursor-not-allowed',
                      index > 0 && 'border-l border-border/50'
                    )}
                  >
                    {option.icon && <span className="text-lg opacity-70">{option.icon}</span>}
                    <span>{option.label}</span>
                  </button>
                )
              })}
            </div>
          )}

          {options.find((o) => o.description) && value !== undefined && !loading && (
            <p className={cn('text-muted-foreground mt-2', config.text)}>
              {options.find((o) => o.value === value)?.description}
            </p>
          )}
        </div>
      </Card>
    )
  }

  // HA-style card with grid buttons
  if (variant === 'grid') {
    return (
      <Card className="dashboardCardBase">
        <div className={cn(config.padding)}>
          {label && <p className={cn('font-medium text-foreground/90 mb-3', config.text)}>{label}</p>}

          {loading ? (
            <div className="grid grid-cols-2 sm:grid-cols-3 gap-2">
              {options.map((_, i) => (
                <Skeleton key={i} className="h-16 rounded-xl" />
              ))}
            </div>
          ) : (
            <div className="grid grid-cols-2 sm:grid-cols-3 gap-2">
              {options.map((option) => {
                const isActive = value === option.value
                return (
                  <button
                    key={option.value}
                    onClick={() => !option.disabled && !loading && !sending && handleClick(option.value)}
                    disabled={option.disabled || loading || sending}
                    className={cn(
                      'flex flex-col items-center justify-center gap-2 p-3 rounded-xl transition-all duration-200',
                      isActive
                        ? 'bg-primary/10 text-primary shadow-sm scale-[1.02]'
                        : 'bg-muted/30 text-muted-foreground hover:bg-muted/50 hover:scale-[1.02]',
                      (option.disabled || loading || sending) && 'opacity-50 cursor-not-allowed'
                    )}
                  >
                    {option.icon ? (
                      <span className={cn(config.icon, 'opacity-70')}>{option.icon}</span>
                    ) : (
                      <span className={cn(config.icon, 'opacity-30')}>•</span>
                    )}
                    <span className={cn('font-medium', config.text)}>{option.label}</span>
                  </button>
                )
              })}
            </div>
          )}
        </div>
      </Card>
    )
  }

  // Pills variant with card
  if (variant === 'pills') {
    return (
      <Card className="dashboardCardBase">
        <div className={cn(config.padding)}>
          {label && <p className={cn('font-medium text-foreground/90 mb-3', config.text)}>{label}</p>}

          {loading ? (
            <Skeleton className="h-10 w-full" />
          ) : (
            <div className="flex flex-wrap gap-2">
              {options.map((option) => {
                const isActive = value === option.value
                return (
                  <button
                    key={option.value}
                    onClick={() => !option.disabled && !loading && !sending && handleClick(option.value)}
                    disabled={option.disabled || loading || sending}
                    className={cn(
                      'inline-flex items-center gap-2 px-4 py-2 rounded-full font-medium transition-all duration-200',
                      config.text,
                      isActive
                        ? 'bg-primary text-primary-foreground shadow-md'
                        : 'bg-muted/50 text-muted-foreground hover:bg-muted',
                      (option.disabled || loading || sending) && 'opacity-50 cursor-not-allowed'
                    )}
                  >
                    {option.icon && <span className="text-base opacity-70">{option.icon}</span>}
                    <span>{option.label}</span>
                  </button>
                )
              })}
            </div>
          )}

          {options.find((o) => o.description) && value !== undefined && !loading && (
            <p className={cn('text-muted-foreground mt-2', config.text)}>
              {options.find((o) => o.value === value)?.description}
            </p>
          )}
        </div>
      </Card>
    )
  }

  // Default card
  return (
    <Card className="dashboardCardBase">
      <div className={cn(config.padding)}>{segmentedContent}</div>
    </Card>
  )
}
