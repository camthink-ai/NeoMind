/**
 * Mobile Edit Bar Component
 *
 * A mobile-optimized bottom toolbar for dashboard component editing.
 * Provides large touch targets (44px+) for easy interaction on mobile devices.
 *
 * Features:
 * - Fixed bottom positioning with safe area support
 * - Large touch-friendly buttons
 * - Smooth animations for show/hide
 * - Context menu for multiple actions
 */

import { memo, useCallback, useEffect, useState } from 'react'
import { cn } from '@/lib/utils'
import { useTranslation } from 'react-i18next'
import {
  Settings2,
  Copy,
  Trash2,
  X,
  Check,
  Move,
} from 'lucide-react'
import { useSafeAreaInsets } from '@/hooks/useMobile'

export interface MobileEditBarProps {
  /** Whether the edit bar is visible */
  isOpen: boolean
  /** Callback when close button is pressed */
  onClose: () => void
  /** Callback when settings button is pressed */
  onSettings: () => void
  /** Callback when copy button is pressed */
  onCopy: () => void
  /** Callback when delete button is pressed */
  onDelete: () => void
  /** Optional component name/title to display */
  componentName?: string
  /** Additional CSS classes */
  className?: string
}

/**
 * Primary action button with large touch target
 */
const ActionButton = memo(function ActionButton({
  icon: Icon,
  label,
  onPress,
  variant = 'default',
  className,
}: {
  icon: React.ComponentType<{ className?: string }>
  label: string
  onPress: () => void
  variant?: 'default' | 'destructive'
  className?: string
}) {
  const handlePress = useCallback((e: React.MouseEvent | React.TouchEvent) => {
    e.preventDefault()
    e.stopPropagation()
    onPress()
  }, [onPress])

  return (
    <button
      onClick={handlePress}
      onTouchEnd={handlePress}
      className={cn(
        'flex flex-col items-center justify-center gap-1',
        'min-w-[72px] min-h-[64px]',
        'p-3 rounded-xl',
        'transition-all duration-normal',
        'active:scale-95',
        'cursor-pointer',
        'touch-action-manipulation',
        variant === 'destructive'
          ? 'bg-muted text-error active:bg-muted'
          : 'bg-secondary text-secondary-foreground active:bg-secondary-hover',
        className
      )}
      style={{ touchAction: 'manipulation' }}
    >
      <Icon className="w-6 h-6" />
      <span className="text-xs font-medium">{label}</span>
    </button>
  )
})


/**
 * Mobile Edit Bar Component
 *
 * Displays a bottom toolbar with editing actions when a component is selected.
 * Automatically adjusts to safe areas on notched devices.
 */
export const MobileEditBar = memo(function MobileEditBar({
  isOpen,
  onClose,
  onSettings,
  onCopy,
  onDelete,
  componentName,
  className,
}: MobileEditBarProps) {
  const { t } = useTranslation('dashboardComponents')
  const insets = useSafeAreaInsets()
  const [showConfirm, setShowConfirm] = useState(false)

  // Hide MobilePrimaryNav while the edit sheet owns the bottom edge.
  useEffect(() => {
    if (!isOpen) return
    document.body.classList.add('mobile-edit-chrome-open')
    return () => {
      document.body.classList.remove('mobile-edit-chrome-open')
    }
  }, [isOpen])

  const handleDeletePress = useCallback(() => {
    if (showConfirm) {
      onDelete()
      setShowConfirm(false)
    } else {
      setShowConfirm(true)
      // Auto-hide confirm after 3 seconds
      setTimeout(() => setShowConfirm(false), 3000)
    }
  }, [showConfirm, onDelete])

  const handleSettings = useCallback(() => {
    setShowConfirm(false)
    onSettings()
  }, [onSettings])

  const handleCopy = useCallback(() => {
    setShowConfirm(false)
    onCopy()
  }, [onCopy])

  // Don't render if not open (except during transitions)
  if (!isOpen) return null

  return (
    <>
      {/* Backdrop */}
      <div
        className={cn(
          'fixed inset-0 bg-overlay-light backdrop-blur-sm z-40',
          'transition-opacity duration-normal',
          isOpen ? 'opacity-100' : 'opacity-0 pointer-events-none'
        )}
        onClick={onClose}
      />

      {/* Edit Bar */}
      <div
        className={cn(
          'fixed left-4 right-4 bottom-4 z-50',
          'bg-bg-95 backdrop-blur-md',
          'rounded-xl shadow-xl',
          'border border-border',
          'transition-all duration-slow ease-out',
          // Safe area padding
          'pb-[calc(1rem+env(safe-area-inset-bottom,0px))]',
          isOpen
            ? 'translate-y-0 opacity-100 scale-100'
            : 'translate-y-full opacity-0 scale-95',
          className
        )}
        style={{
          paddingBottom: `calc(1rem + ${insets.bottom}px)`,
        }}
      >
        {/* Header with component name and close button */}
        <div className="flex items-center justify-between px-4 pt-4 pb-2 ">
          <div className="flex items-center gap-2">
            <Move className="w-4 h-4 text-muted-foreground" />
            <span className="text-sm font-medium text-foreground">
              {componentName || t('mobileEditBar.component')}
            </span>
          </div>
          <button
            onClick={onClose}
            onTouchEnd={(e) => {
              e.preventDefault()
              onClose()
            }}
            className="h-9 w-9 flex items-center justify-center rounded-lg hover:bg-muted active:scale-95 transition-all"
            style={{ touchAction: 'manipulation' }}
          >
            <X className="w-4 h-4" />
          </button>
        </div>

        {/* Action buttons */}
        <div className="flex items-center justify-around gap-2 px-2 pt-2">
          <ActionButton
            icon={Settings2}
            label={t('mobileEditBar.settings')}
            onPress={handleSettings}
          />
          <ActionButton
            icon={Copy}
            label={t('mobileEditBar.copy')}
            onPress={handleCopy}
          />
          <ActionButton
            icon={showConfirm ? Check : Trash2}
            label={showConfirm ? t('mobileEditBar.confirm') : t('mobileEditBar.delete')}
            onPress={handleDeletePress}
            variant={showConfirm ? 'default' : 'destructive'}
            className={showConfirm ? 'bg-success-light text-success active:bg-success-light' : ''}
          />
        </div>
      </div>
    </>
  )
})
