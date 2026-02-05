import { ReactNode } from 'react'
import { useTranslation } from 'react-i18next'
import { Button } from '@/components/ui/button'
import { ArrowLeft } from 'lucide-react'
import { cn } from '@/lib/utils'

export interface SubPageHeaderProps {
  title: string
  description?: string
  icon?: ReactNode
  onBack: () => void
  backLabel?: string
  actions?: ReactNode
  className?: string
}

/**
 * Sub-page header with back button
 *
 * Used for sub-pages that need a back navigation button.
 * Provides consistent header styling for detail/edit views.
 *
 * @example
 * <SubPageHeader
 *   title="Device Details"
 *   description="View and manage device settings"
 *   icon={<Server className="h-6 w-6" />}
 *   onBack={() => navigate(-1)}
 *   backLabel="Back"
 * />
 */
export function SubPageHeader({
  title,
  description,
  icon,
  onBack,
  backLabel,
  actions,
  className,
}: SubPageHeaderProps) {
  const { t } = useTranslation('common')
  const defaultBackLabel = backLabel || t('back')
  return (
    <div className={cn('flex items-center gap-4 mb-6', className)}>
      <Button variant="ghost" size="sm" onClick={onBack} className="gap-1">
        <ArrowLeft className="h-4 w-4" />
        {defaultBackLabel}
      </Button>
      <div className="flex-1 flex items-center justify-between">
        <div>
          <h2 className="text-2xl font-bold flex items-center gap-2">
            {icon}
            {title}
          </h2>
          {description && (
            <p className="text-sm text-muted-foreground mt-1">{description}</p>
          )}
        </div>
        {actions && <div className="flex items-center gap-2">{actions}</div>}
      </div>
    </div>
  )
}
