import { ReactNode } from 'react'
import { useTranslation } from 'react-i18next'
import { Button } from '@/components/ui/button'
import { cn } from '@/lib/utils'

export interface BulkAction {
  label: string
  icon?: ReactNode
  variant?: 'default' | 'destructive' | 'outline' | 'ghost' | 'secondary'
  onClick: () => void
  disabled?: boolean
}

export interface BulkActionBarProps {
  selectedCount: number
  actions: BulkAction[]
  onCancel?: () => void
  cancelLabel?: string
  className?: string
}

/**
 * Bulk action bar that appears when items are selected
 *
 * @example
 * <BulkActionBar
 *   selectedCount={selectedIds.size}
 *   actions={[
 *     { label: '确认', icon: <Check className="h-4 w-4" />, onClick: handleAcknowledge },
 *     { label: '删除', icon: <Trash2 className="h-4 w-4" />, onClick: handleDelete, variant: 'outline' },
 *   ]}
 *   onCancel={() => setSelectedIds(new Set())}
 * />
 */
export function BulkActionBar({
  selectedCount,
  actions,
  onCancel,
  cancelLabel,
  className,
}: BulkActionBarProps) {
  const { t } = useTranslation('common')

  if (selectedCount === 0) return null

  return (
    <div className={cn('flex items-center justify-between p-3 mb-4 bg-muted rounded-lg', className)}>
      <span className="text-sm font-medium">
        {t('selected', { count: selectedCount })}
      </span>
      <div className="flex gap-2">
        {actions.map((action, index) => (
          <Button
            key={index}
            variant={action.variant || 'outline'}
            size="sm"
            onClick={action.onClick}
            disabled={action.disabled}
          >
            {action.icon && <span className="mr-2">{action.icon}</span>}
            {action.label}
          </Button>
        ))}
        {onCancel && (
          <Button variant="ghost" size="sm" onClick={onCancel}>
            {cancelLabel || t('cancelSelection')}
          </Button>
        )}
      </div>
    </div>
  )
}
