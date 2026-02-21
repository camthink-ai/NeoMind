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

import { memo, useCallback, useState } from 'react'
import { cn } from '@/lib/utils'
import { useTranslation } from 'react-i18next'
import {
  Settings2,
  Copy,
  Trash2,
  X,
  MoreHorizontal,
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
        'transition-all duration-200',
        'active:scale-95',
        'cursor-pointer',
        'touch-action-manipulation',
        variant === 'destructive'
          ? 'bg-destructive/15 text-destructive active:bg-destructive/25'
          : 'bg-secondary/50 text-secondary-foreground active:bg-secondary/80',
        className
      )}
      style={{ touchAction: 'manipulation' }}
    >
      <Icon className="w-6 h-6" strokeWidth={2} />
      <span className="text-xs font-medium">{label}</span>
    </button>
  )
})

/**
 * Compact mode action button for when space is limited
 */
const CompactButton = memo(function CompactButton({
  icon: Icon,
  label,
  onPress,
  variant = 'default',
}: {
  icon: React.ComponentType<{ className?: string }>
  label: string
  onPress: () => void
  variant?: 'default' | 'destructive'
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
        'flex items-center justify-center',
        'min-w-[56px] min-h-[56px]',
        'rounded-xl',
        'transition-all duration-200',
        'active:scale-95',
        'cursor-pointer',
        'touch-action-manipulation',
        variant === 'destructive'
          ? 'bg-destructive/15 text-destructive active:bg-destructive/25'
          : 'bg-secondary/50 text-secondary-foreground active:bg-secondary/80'
      )}
      aria-label={label}
      style={{ touchAction: 'manipulation' }}
    >
      <Icon className="w-6 h-6" strokeWidth={2} />
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
          'fixed inset-0 bg-black/20 backdrop-blur-sm z-40',
          'transition-opacity duration-200',
          isOpen ? 'opacity-100' : 'opacity-0 pointer-events-none'
        )}
        onClick={onClose}
      />

      {/* Edit Bar */}
      <div
        className={cn(
          'fixed left-4 right-4 bottom-4 z-50',
          'bg-background/95 backdrop-blur-md',
          'rounded-2xl shadow-2xl shadow-black/20',
          'border border-border/50',
          'transition-all duration-300 ease-out',
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
        <div className="flex items-center justify-between px-4 pt-4 pb-2 border-b border-border/50">
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
            className={showConfirm ? 'bg-green-500/15 text-green-600 active:bg-green-500/25' : ''}
          />
        </div>
      </div>
    </>
  )
})

/**
 * Compact version for smaller screens or when more space is needed
 */
export interface CompactMobileEditBarProps {
  isOpen: boolean
  onClose: () => void
  onSettings: () => void
  onCopy: () => void
  onDelete: () => void
  className?: string
}

export const CompactMobileEditBar = memo(function CompactMobileEditBar({
  isOpen,
  onClose,
  onSettings,
  onCopy,
  onDelete,
  className,
}: CompactMobileEditBarProps) {
  const { t } = useTranslation('dashboardComponents')
  const insets = useSafeAreaInsets()

  if (!isOpen) return null

  return (
    <>
      {/* Backdrop */}
      <div
        className="fixed inset-0 bg-black/20 backdrop-blur-sm z-40"
        onClick={onClose}
      />

      {/* Compact Edit Bar */}
      <div
        className={cn(
          'fixed left-1/2 -translate-x-1/2 bottom-4 z-50',
          'flex items-center gap-2',
          'bg-background/95 backdrop-blur-md',
          'rounded-2xl shadow-2xl shadow-black/20',
          'border border-border/50',
          'p-2',
          'transition-all duration-300 ease-out',
          'pb-[calc(0.5rem+env(safe-area-inset-bottom,0px))]',
          isOpen
            ? 'translate-y-0 opacity-100 scale-100'
            : 'translate-y-full opacity-0 scale-95',
          className
        )}
        style={{
          paddingBottom: `calc(0.5rem + ${insets.bottom}px)`,
        }}
      >
        <CompactButton
          icon={Settings2}
          label={t('mobileEditBar.settings')}
          onPress={onSettings}
        />
        <CompactButton
          icon={Copy}
          label={t('mobileEditBar.copy')}
          onPress={onCopy}
        />
        <div className="w-px h-8 bg-border/50" />
        <CompactButton
          icon={Trash2}
          label={t('mobileEditBar.delete')}
          onPress={onDelete}
          variant="destructive"
        />
      </div>
    </>
  )
})

/**
 * Floating edit button for selecting components
 */
export interface FloatingEditButtonProps {
  isOpen: boolean
  onToggle: () => void
  count?: number
  className?: string
}

export const FloatingEditButton = memo(function FloatingEditButton({
  isOpen,
  onToggle,
  count,
  className,
}: FloatingEditButtonProps) {
  const { t } = useTranslation('dashboardComponents')
  const insets = useSafeAreaInsets()

  return (
    <button
      onClick={onToggle}
      className={cn(
        'fixed z-50',
        'flex items-center gap-2',
        'min-h-[56px] px-6',
        'bg-primary text-primary-foreground',
        'rounded-full shadow-lg shadow-primary/25',
        'transition-all duration-300 ease-out',
        'active:scale-95',
        // Position
        'left-1/2 -translate-x-1/2',
        'bottom-4',
        // Safe area
        'mb-[env(safe-area-inset-bottom,0px)]',
        isOpen
          ? 'translate-y-24 opacity-0'
          : 'translate-y-0 opacity-100',
        className
      )}
      style={{
        marginBottom: `${insets.bottom}px`,
      }}
    >
      <MoreHorizontal className="w-5 h-5" />
      <span className="font-medium">
        {count !== undefined && count > 0
          ? t('mobileEditBar.selectedCount', { count })
          : t('mobileEditBar.edit')}
      </span>
    </button>
  )
})
