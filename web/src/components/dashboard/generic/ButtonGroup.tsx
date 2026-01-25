/**
 * Button Group Component (Unified Styles)
 *
 * shadcn/ui style button group for multi-option device control.
 * Clean design with segmented, pills, and grid variants.
 * Supports fan speeds, scenes, modes, etc.
 */

import { Card } from '@/components/ui/card'
import { Skeleton } from '@/components/ui/skeleton'
import { cn } from '@/lib/utils'
import { useDataSource } from '@/hooks/useDataSource'
import { dashboardCardBase, dashboardComponentSize } from '@/design-system/tokens/size'
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
  size?: 'sm' | 'md' | 'lg'
  variant?: 'segmented' | 'pills' | 'grid'
  multiple?: boolean
  showCard?: boolean
  className?: string
}

// Use unified size configuration
const getButtonSizeConfig = (size: 'sm' | 'md' | 'lg') => {
  const config = dashboardComponentSize[size]
  return {
    button: size === 'sm' ? 'h-8 px-2' : size === 'md' ? 'h-9 px-3' : 'h-10 px-4',
    icon: size === 'sm' ? 'text-base' : size === 'md' ? 'text-lg' : 'text-xl',
    text: config.labelText,
    padding: config.padding,
  }
}

export function ButtonGroup({
  dataSource,
  value: propValue,
  onValueChange,
  options,
  label,
  orientation = 'horizontal',
  size = 'md',
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

  const config = getButtonSizeConfig(size)

  // Render individual button
  const renderButton = (option: ButtonOption, isActive: boolean, isFirst: boolean, isLast: boolean) => {
    const IconDisplay = option.icon ? (
      <span className={cn(config.icon, 'opacity-70')}>{option.icon}</span>
    ) : null

    // Segmented style
    if (variant === 'segmented') {
      return (
        <button
          key={option.value}
          onClick={() => !option.disabled && !loading && !sending && handleClick(option.value)}
          disabled={option.disabled || loading || sending}
          className={cn(
            'flex-1 flex items-center justify-center gap-2 font-medium transition-all duration-200',
            config.button,
            config.text,
            // Border radius based on position
            isFirst && 'rounded-l-md rounded-r-none',
            isLast && 'rounded-r-md rounded-l-none',
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
            'inline-flex items-center gap-2 px-3 py-1.5 rounded-full font-medium transition-all duration-200',
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

    // Grid style
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
        {IconDisplay || <span className={cn(config.icon, 'opacity-30')}>•</span>}
        <span className={cn('font-medium', config.text)}>{option.label}</span>
      </button>
    )
  }

  // Check if option is active
  const isActive = (option: ButtonOption) => {
    return multiple
      ? Array.isArray(value)
        ? value.includes(option.value)
        : false
      : value === option.value
  }

  // Segmented control with card
  if (variant === 'segmented') {
    const content = (
      <div className={cn('w-full space-y-3', className)}>
        {label && <p className={cn('font-medium text-foreground/90', config.text)}>{label}</p>}

        {loading ? (
          <Skeleton className="h-10 w-full" />
        ) : (
          <div className="flex rounded-md overflow-hidden border border-border/50">
            {options.map((option, index) => (
              <button
                key={option.value}
                onClick={() => !option.disabled && !loading && !sending && handleClick(option.value)}
                disabled={option.disabled || loading || sending}
                className={cn(
                  'flex-1 flex items-center justify-center gap-2 py-2.5 transition-all duration-200',
                  config.text,
                  isActive(option)
                    ? 'bg-primary text-primary-foreground font-semibold'
                    : 'bg-muted/30 text-muted-foreground hover:bg-muted/50',
                  (option.disabled || loading || sending) && 'opacity-50 cursor-not-allowed',
                  index > 0 && 'border-l border-border/50'
                )}
              >
                {option.icon && <span className={cn(config.icon, 'opacity-70')}>{option.icon}</span>}
                <span>{option.label}</span>
              </button>
            ))}
          </div>
        )}

        {/* Description */}
        {options.find((o) => o.description) && value !== undefined && !loading && (
          <p className={cn('text-muted-foreground', config.text)}>
            {options.find((o) => o.value === value)?.description}
          </p>
        )}
      </div>
    )

    if (!showCard) return content
    return (
      <Card className={dashboardCardBase}>
        <div className={cn(config.padding)}>{content}</div>
      </Card>
    )
  }

  // Grid variant
  if (variant === 'grid') {
    const content = (
      <div className={cn('w-full space-y-3', className)}>
        {label && <p className={cn('font-medium text-foreground/90', config.text)}>{label}</p>}

        {loading ? (
          <div className="grid grid-cols-2 sm:grid-cols-3 gap-2">
            {options.map((_, i) => (
              <Skeleton key={i} className="h-16 rounded-xl" />
            ))}
          </div>
        ) : (
          <div className="grid grid-cols-2 sm:grid-cols-3 gap-2">
            {options.map((option) => (
              <button
                key={option.value}
                onClick={() => !option.disabled && !loading && !sending && handleClick(option.value)}
                disabled={option.disabled || loading || sending}
                className={cn(
                  'flex flex-col items-center justify-center gap-2 p-3 rounded-xl transition-all duration-200',
                  isActive(option)
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
            ))}
          </div>
        )}
      </div>
    )

    if (!showCard) return content
    return (
      <Card className={dashboardCardBase}>
        <div className={cn(config.padding)}>{content}</div>
      </Card>
    )
  }

  // Pills variant
  const content = (
    <div className={cn('w-full space-y-3', className)}>
      {label && <p className={cn('font-medium text-foreground/90', config.text)}>{label}</p>}

      {loading ? (
        <Skeleton className="h-10 w-full" />
      ) : (
        <div className={cn(
          'flex gap-2',
          orientation === 'vertical' ? 'flex-col' : 'flex-wrap'
        )}>
          {options.map((option) => (
            <button
              key={option.value}
              onClick={() => !option.disabled && !loading && !sending && handleClick(option.value)}
              disabled={option.disabled || loading || sending}
              className={cn(
                'inline-flex items-center gap-2 px-4 py-2 rounded-full font-medium transition-all duration-200',
                config.text,
                isActive(option)
                  ? 'bg-primary text-primary-foreground shadow-md'
                  : 'bg-muted/50 text-muted-foreground hover:bg-muted',
                (option.disabled || loading || sending) && 'opacity-50 cursor-not-allowed'
              )}
            >
              {option.icon && <span className="text-base opacity-70">{option.icon}</span>}
              <span>{option.label}</span>
            </button>
          ))}
        </div>
      )}

      {/* Description */}
      {options.find((o) => o.description) && value !== undefined && !loading && (
        <p className={cn('text-muted-foreground', config.text)}>
          {options.find((o) => o.value === value)?.description}
        </p>
      )}
    </div>
  )

  if (!showCard) return content
  return (
    <Card className={dashboardCardBase}>
      <div className={cn(config.padding)}>{content}</div>
    </Card>
  )
}
