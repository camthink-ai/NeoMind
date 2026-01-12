import { Button } from '@/components/ui/button'
import { cn } from '@/lib/utils'

export interface EmptyStateProps {
  icon?: React.ReactNode
  title: string
  description?: string
  action?: {
    label: string
    onClick: () => void
    variant?: 'default' | 'outline' | 'secondary' | 'ghost' | 'destructive'
  }
  className?: string
}

/**
 * Empty state component for when there's no data to display
 *
 * @example
 * <EmptyState
 *   title="暂无设备"
 *   description="点击下方按钮添加您的第一个设备"
 *   action={{ label: '添加设备', onClick: handleAdd }}
 * />
 */
export function EmptyState({ icon, title, description, action, className }: EmptyStateProps) {
  return (
    <div className={cn('flex flex-col items-center justify-center py-12 px-4 text-center', className)}>
      {icon && (
        <div className="mb-4 flex h-16 w-16 items-center justify-center rounded-full bg-muted/50">
          {icon}
        </div>
      )}
      <h3 className="text-lg font-semibold">{title}</h3>
      {description && (
        <p className="mt-2 text-sm text-muted-foreground max-w-sm">{description}</p>
      )}
      {action && (
        <Button
          onClick={action.onClick}
          variant={action.variant || 'default'}
          className="mt-6"
        >
          {action.label}
        </Button>
      )}
    </div>
  )
}

/**
 * Compact empty state for smaller spaces
 */
export function EmptyStateCompact({
  title,
  description,
}: {
  title: string
  description?: string
}) {
  return (
    <div className="flex flex-col items-center justify-center py-6 text-center">
      <p className="font-medium text-muted-foreground">{title}</p>
      {description && (
        <p className="mt-1 text-sm text-muted-foreground/70">{description}</p>
      )}
    </div>
  )
}
