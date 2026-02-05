/**
 * FullScreenHeader Component
 *
 * Unified header for full-screen automation dialogs.
 * Features: back button, icon + title, action buttons (test, save, close)
 */

import { ReactNode } from 'react'
import { useTranslation } from 'react-i18next'
import { Button } from '@/components/ui/button'
import { ArrowLeft, X, Save, Loader2, Play } from 'lucide-react'
import { cn } from '@/lib/utils'

export interface FullScreenHeaderProps {
  title: string
  subtitle: string
  icon: ReactNode
  iconBg?: string
  onClose: () => void
  onSave: () => void
  onTest?: () => void
  saving?: boolean
  canSave?: boolean
  canTest?: boolean
  showTest?: boolean
  extraActions?: ReactNode
}

export function FullScreenHeader({
  title,
  subtitle,
  icon,
  iconBg = 'bg-primary/10',
  onClose,
  onSave,
  onTest,
  saving = false,
  canSave = true,
  canTest = true,
  showTest = false,
  extraActions,
}: FullScreenHeaderProps) {
  const { t } = useTranslation(['common'])
  return (
    <header className="flex items-center justify-between px-4 md:px-6 py-4 border-b bg-background shrink-0">
      {/* Left: Back button + Title */}
      <div className="flex items-center gap-3 md:gap-4 min-w-0">
        <Button
          variant="ghost"
          size="icon"
          onClick={onClose}
          className="shrink-0"
        >
          <ArrowLeft className="h-5 w-5" />
        </Button>
        <div className="flex items-center gap-3 min-w-0">
          <div className={cn('w-9 h-9 md:w-10 md:h-10 rounded-xl flex items-center justify-center shrink-0', iconBg)}>
            {icon}
          </div>
          <div className="min-w-0">
            <h1 className="text-base md:text-lg font-semibold truncate">
              {title}
            </h1>
            <p className="text-xs md:text-sm text-muted-foreground truncate">
              {subtitle}
            </p>
          </div>
        </div>
      </div>

      {/* Right: Action buttons */}
      <div className="flex items-center gap-2 shrink-0">
        {extraActions}
        {showTest && onTest && (
          <Button
            variant="outline"
            onClick={onTest}
            disabled={!canTest || saving}
          >
            <Play className="h-4 w-4 mr-1.5" />
            <span className="hidden sm:inline">{t('common:test')}</span>
            <span className="sm:hidden">{t('common:test')}</span>
          </Button>
        )}
        <Button onClick={onSave} disabled={!canSave || saving}>
          {saving ? (
            <Loader2 className="h-4 w-4 mr-1.5 animate-spin" />
          ) : (
            <Save className="h-4 w-4 mr-1.5" />
          )}
          {saving ? t('common:saving') : t('common:save')}
        </Button>
        <Button
          variant="ghost"
          size="icon"
          onClick={onClose}
          className="ml-1 md:ml-2"
        >
          <X className="h-5 w-5" />
        </Button>
      </div>
    </header>
  )
}
