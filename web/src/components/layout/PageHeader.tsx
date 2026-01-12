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
  bordered: 'border-b pb-4 -mt-4 -mx-4 px-4 md:-mx-6 md:px-6',
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
    <div className={cn('flex items-center justify-between', variantStyles[variant], className)}>
      <div className="flex items-center gap-3">
        {icon && (
          <div className="p-2 rounded-xl bg-gradient-to-br from-blue-500/10 to-purple-500/10">
            {icon}
          </div>
        )}
        <div>
          <h1 className="text-2xl md:text-3xl font-bold tracking-tight">{title}</h1>
          {description && (
            <p className="text-sm text-muted-foreground mt-1">{description}</p>
          )}
        </div>
      </div>
      {actions && <div className="flex items-center gap-2">{actions}</div>}
    </div>
  )
}
