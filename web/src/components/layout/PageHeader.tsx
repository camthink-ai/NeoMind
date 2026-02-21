import { ReactNode } from 'react'
import { cn } from '@/lib/utils'

export interface PageHeaderProps {
  title: string
  description?: string
  icon?: ReactNode
  actions?: ReactNode
  variant?: 'default' | 'gradient' | 'bordered'
  className?: string
}

const variantStyles = {
  default: '',
  gradient: 'bg-gradient-to-br from-blue-500/10 to-purple-500/10 rounded-xl p-4 -m-4 mb-2 md:p-6 md:-m-6',
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
          <div className="shrink-0 rounded-xl bg-gradient-to-br from-blue-500/10 to-purple-500/10 p-2">
            {icon}
          </div>
        )}
        <div className="min-w-0 flex-1">
          <h1 className="truncate text-xl font-bold tracking-tight sm:text-2xl md:text-3xl">{title}</h1>
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
