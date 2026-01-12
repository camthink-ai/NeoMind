import { Button } from '@/components/ui/button'
import { RefreshCw } from 'lucide-react'
import { cn } from '@/lib/utils'

export interface Action {
  label: string
  icon?: React.ReactNode
  variant?: 'default' | 'primary' | 'destructive' | 'outline' | 'ghost' | 'secondary'
  onClick: () => void
  disabled?: boolean
}

export interface ActionBarProps {
  title?: string
  actions?: Action[]
  onRefresh?: () => void
  refreshLoading?: boolean
  className?: string
}

/**
 * Action bar component for page headers with actions
 *
 * @example
 * <ActionBar
 *   title="设备管理"
 *   onRefresh={handleRefresh}
 *   refreshLoading={isLoading}
 *   actions={[
 *     { label: '添加设备', onClick: handleAdd, variant: 'primary' },
 *     { label: '批量删除', onClick: handleDelete, variant: 'danger' },
 *   ]}
 * />
 */
export function ActionBar({
  title,
  actions = [],
  onRefresh,
  refreshLoading = false,
  className,
}: ActionBarProps) {
  return (
    <div className={cn('flex items-center justify-between gap-4 mb-6', className)}>
      {title && <h2 className="text-xl font-semibold">{title}</h2>}
      <div className="flex items-center gap-2 ml-auto">
        {actions.map((action, index) => (
          <Button
            key={index}
            variant={action.variant === 'primary' ? 'default' : action.variant || 'outline'}
            size="sm"
            onClick={action.onClick}
            disabled={action.disabled}
          >
            {action.icon}
            {action.label}
          </Button>
        ))}
        {onRefresh && (
          <Button
            variant="outline"
            size="icon"
            onClick={onRefresh}
            disabled={refreshLoading}
          >
            <RefreshCw className={cn('h-4 w-4', refreshLoading && 'animate-spin')} />
          </Button>
        )}
      </div>
    </div>
  )
}

/**
 * Simple action bar with just refresh button
 */
export interface ActionBarSimpleProps {
  onRefresh: () => void
  refreshLoading?: boolean
  title?: string
  className?: string
}

export function ActionBarSimple({
  onRefresh,
  refreshLoading = false,
  title,
  className,
}: ActionBarSimpleProps) {
  return (
    <div className={cn('flex items-center justify-between mb-6', className)}>
      {title && <h2 className="text-xl font-semibold">{title}</h2>}
      <Button
        variant="outline"
        size="icon"
        onClick={onRefresh}
        disabled={refreshLoading}
        className={cn(!title && 'ml-auto')}
      >
        <RefreshCw className={cn('h-4 w-4', refreshLoading && 'animate-spin')} />
      </Button>
    </div>
  )
}
