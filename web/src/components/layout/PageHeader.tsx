import { ReactNode } from 'react'
import { cn } from '@/lib/utils'

export interface PageHeaderProps {
  title: string
  description?: string
  icon?: ReactNode
  actions?: ReactNode
  variant?: 'default' | 'bordered'
  className?: string
}

const variantStyles = {
  default: '',
  bordered: 'border-b pb-6 -mt-4 -mx-4 px-4 md:-mx-6 md:px-6',
}

/**
 * Standard page header component
 *
 * Provides consistent header styling across all pages.
 *
 * @example
 * <PageHeader
 *   title="Commands"
 *   description="View and manage command history"
 *   actions={<Button>Refresh</Button>}
 *   variant="bordered"
 * />
 */
export function PageHeader({
  title,
  description,
  icon,
  actions,
  variant = 'default',
  className,
}: PageHeaderProps) {
  return (
    <div className={cn(variantStyles[variant], className)}>
      {/* Title section with actions on same row for desktop */}
      <div className="flex items-start gap-3 sm:gap-4">
        {icon && (
          // Neutral surface — the icon node carries its own accent color, so
          // the tile works for any hue (the old `primary-light` gradient
          // class didn't exist in the token set and rendered nothing).
          <div className="shrink-0 rounded-xl bg-muted p-2">
            {icon}
          </div>
        )}
        <div className="min-w-0 flex-1">
          {/* semibold, not bold — a 700-weight 24px title against 14px
              medium tab labels read heavy; 600 keeps the hierarchy with a
              lighter step. */}
          <h1 className="truncate text-xl font-semibold tracking-tight sm:text-2xl">{title}</h1>
          {description && (
            <p className="mt-1 line-clamp-2 text-sm text-muted-foreground sm:mt-2">{description}</p>
          )}
        </div>
        {/* Actions - on same row for desktop, below for mobile */}
        {actions && (
          <div className="hidden sm:flex shrink-0 items-center gap-2 flex-wrap justify-end">
            {actions}
          </div>
        )}
      </div>
      {/* Mobile actions - below title */}
      {actions && (
        <div className="mt-3 flex shrink-0 flex-wrap gap-2 sm:hidden">
          {actions}
        </div>
      )}
    </div>
  )
}
