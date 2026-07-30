/**
 * UnifiedFormDialog Component
 *
 * A unified dialog with mobile full-screen support, safe area insets,
 * loading states, and consistent interaction patterns.
 *
 * Features:
 * - Mobile full-screen via portal to #dialog-root
 * - Desktop centered dialog via portal to #dialog-root
 * - Safe area insets integration
 * - Loading overlay pattern
 * - Consistent footer with Cancel/Submit buttons
 * - Custom footer support
 * - Escape key / backdrop click to close
 * - Focus trap on mobile
 */

import { getPortalRoot } from '@/lib/portal'
import { ReactNode, useEffect, useRef, useCallback } from 'react'
import { createPortal } from 'react-dom'
import { useTranslation } from 'react-i18next'
import { X, Loader2, AlertCircle } from 'lucide-react'
import { Button } from '@/components/ui/button'
import { cn } from '@/lib/utils'
import { dialogHeader } from '@/design-system/tokens/size'
import { useIsMobile, useSafeAreaInsets } from '@/hooks/useMobile'
import { useMobileBodyScrollLock } from '@/hooks/useBodyScrollLock'

export interface UnifiedFormDialogProps {
  /** Whether the dialog is open */
  open: boolean
  /** Callback when dialog open state changes */
  onOpenChange: (open: boolean) => void
  /** Dialog title */
  title: string
  /** Optional description below title */
  description?: string
  /** Optional icon displayed next to title */
  icon?: ReactNode
  /** Dialog width on desktop */
  width?: 'sm' | 'md' | 'lg' | 'xl' | '2xl' | '3xl'
  /** Whether to use full-screen on mobile (default: true) */
  fullScreenOnMobile?: boolean
  /** Whether the form content is loading */
  loading?: boolean
  /** Whether the form is currently being submitted */
  isSubmitting?: boolean
  /** Submit error to display */
  submitError?: string | null
  /** Async submit handler */
  onSubmit?: () => Promise<void>
  /** Custom submit button label */
  submitLabel?: string
  /** Custom cancel button label */
  cancelLabel?: string
  /** Whether to show the cancel button (default: true) */
  showCancelButton?: boolean
  /** Whether to disable the submit button */
  submitDisabled?: boolean
  /** Form content */
  children: ReactNode
  /** Additional class name for the dialog container */
  className?: string
  /** Additional class name for the content scroll area */
  contentClassName?: string
  /** Custom footer content (replaces default buttons) */
  footer?: ReactNode
  /** Whether to hide the footer entirely */
  hideFooter?: boolean
  /** Whether to prevent closing during submission */
  preventCloseOnSubmit?: boolean
}

const widthClasses = {
  sm: 'max-w-md',
  md: 'max-w-lg',
  lg: 'max-w-xl',
  xl: 'max-w-2xl',
  '2xl': 'max-w-3xl',
  '3xl': 'max-w-5xl',
}

export function UnifiedFormDialog({
  open,
  onOpenChange,
  title,
  description,
  icon,
  width = 'md',
  fullScreenOnMobile = true,
  loading = false,
  isSubmitting = false,
  submitError,
  onSubmit,
  submitLabel,
  cancelLabel,
  showCancelButton = true,
  submitDisabled = false,
  children,
  className,
  contentClassName,
  footer,
  hideFooter = false,
  preventCloseOnSubmit = true,
}: UnifiedFormDialogProps) {
  const { t } = useTranslation('common')
  const isMobile = useIsMobile()
  const insets = useSafeAreaInsets()
  const contentRef = useRef<HTMLDivElement>(null)

  // Default labels
  const submitText = submitLabel || t('save', 'Save')
  const cancelText = cancelLabel || t('cancel', 'Cancel')

  // Lock body scroll on mobile
  useMobileBodyScrollLock(isMobile && fullScreenOnMobile && open)

  // Handle submit
  const handleSubmit = useCallback(async () => {
    if (isSubmitting || !onSubmit) return
    try {
      await onSubmit()
    } catch {
      // Error is handled via submitError prop
    }
  }, [isSubmitting, onSubmit])

  // Handle close
  const handleClose = useCallback(() => {
    if (isSubmitting && preventCloseOnSubmit) return
    onOpenChange(false)
  }, [isSubmitting, preventCloseOnSubmit, onOpenChange])

  // Focus trap for mobile full-screen dialog
  useEffect(() => {
    if (!open || !isMobile || !fullScreenOnMobile) return

    const container = contentRef.current?.parentElement?.parentElement
    if (!container) return

    const focusableSelector = 'button, [href], input, select, textarea, [tabindex]:not([tabindex="-1"])'

    const raf = requestAnimationFrame(() => {
      const firstFocusable = container.querySelector(focusableSelector) as HTMLElement
      firstFocusable?.focus()
    })

    const handleKeyDown = (e: KeyboardEvent) => {
      if (e.key !== 'Tab') return

      const focusableElements = container.querySelectorAll(focusableSelector)
      if (focusableElements.length === 0) return

      const first = focusableElements[0] as HTMLElement
      const last = focusableElements[focusableElements.length - 1] as HTMLElement

      if (e.shiftKey) {
        if (document.activeElement === first) {
          e.preventDefault()
          last.focus()
        }
      } else {
        if (document.activeElement === last) {
          e.preventDefault()
          first.focus()
        }
      }
    }

    container.addEventListener('keydown', handleKeyDown)
    return () => {
      cancelAnimationFrame(raf)
      container.removeEventListener('keydown', handleKeyDown)
    }
  }, [open, isMobile, fullScreenOnMobile])

  // Scroll to top when dialog opens
  useEffect(() => {
    if (open && contentRef.current) {
      contentRef.current.scrollTop = 0
    }
  }, [open])

  // Keyboard handling
  useEffect(() => {
    if (!open) return

    const handleKeyDown = (e: KeyboardEvent) => {
      if (e.key === 'Escape' && !(isSubmitting && preventCloseOnSubmit)) {
        handleClose()
      }
    }

    window.addEventListener('keydown', handleKeyDown)
    return () => window.removeEventListener('keydown', handleKeyDown)
  }, [open, isSubmitting, preventCloseOnSubmit, handleClose])

  const isDisabled = loading || isSubmitting || submitDisabled

  // Default footer buttons
  const defaultFooter = (
    <>
      {showCancelButton && (
        <Button
          variant="outline"
          onClick={handleClose}
          disabled={isSubmitting && preventCloseOnSubmit}
          className="min-w-[80px]"
        >
          {cancelText}
        </Button>
      )}
      {onSubmit && (
        <Button
          onClick={handleSubmit}
          disabled={isDisabled}
          className="min-w-[80px]"
        >
          {isSubmitting && <Loader2 className="mr-2 h-4 w-4 animate-spin" />}
          {submitText}
        </Button>
      )}
    </>
  )

  const footerContent = hideFooter ? null : (footer || defaultFooter)

  // Extract z-index from className for nested dialog support (e.g. above FullScreenDialog z:100)
  const zIndexMatch = className?.match(/z-\[?(\d+)\]?/)
  const overlayZIndex = zIndexMatch ? `z-[${zIndexMatch[1]}]` : 'z-50'
  const dialogZIndex = zIndexMatch ? `z-[${zIndexMatch[1]}]` : 'z-50'

  // Mobile full-screen render
  if (isMobile && fullScreenOnMobile) {
    return createPortal(
      open ? (
        <div
          className={cn("fixed inset-0 animate-in fade-in duration-200", overlayZIndex)}
          // Use opaque --chrome (matches MobilePageHeader) instead of
          // bg-background which has /97% alpha in dark mode and lets the
          // previous layer bleed through, creating a visible color split.
          style={{ backgroundColor: 'var(--chrome)' }}
          // Portal content bubbles through the React tree (not the DOM tree).
          // Without this, every tap inside the dialog — including the close
          // button — reaches ancestor handlers like a table row's onRowClick.
          onClick={(e) => e.stopPropagation()}
        >
          <div className="flex h-full w-full flex-col">
            {/* Header */}
            <div
              className={dialogHeader}
              style={{ paddingTop: `calc(1rem + ${insets.top}px)` }}
            >
              <div className="flex items-center gap-3 min-w-0 flex-1">
                {icon && <div className="text-muted-foreground shrink-0">{icon}</div>}
                <div className="min-w-0 flex-1">
                  <h1 className="text-base font-semibold truncate">{title}</h1>
                  {description && (
                    <p className="text-xs text-muted-foreground truncate">{description}</p>
                  )}
                </div>
              </div>
              <Button
                variant="ghost"
                size="icon"
                onClick={handleClose}
                disabled={isSubmitting && preventCloseOnSubmit}
                className="shrink-0"
              >
                <X className="h-5 w-5" />
              </Button>
            </div>

            {/* Content */}
            <div
              ref={contentRef}
              className={cn("flex-1 overflow-y-auto overflow-x-hidden", contentClassName)}
            >
              <div className="p-4 space-y-4">
                {loading ? (
                  <div className="flex items-center justify-center py-8">
                    <Loader2 className="h-6 w-6 animate-spin text-muted-foreground" />
                  </div>
                ) : (
                  children
                )}
              </div>
            </div>

            {/* Error message */}
            {submitError && (
              <div className="px-4 py-3 bg-muted border-t border-error">
                <div className="flex items-center gap-2 text-sm text-error">
                  <AlertCircle className="h-4 w-4 shrink-0" />
                  <span>{submitError}</span>
                </div>
              </div>
            )}

            {/* Footer */}
            {footerContent && (
              <div
                // No bg class — footer inherits the container's chrome bg
                // (set on the outer mobile portal). Adding bg-background here
                // re-introduces the dark-mode /97% alpha see-through that
                // makes the footer read as a different color from the body
                // of the dialog. The border-t alone provides enough visual
                // separation between content and actions.
                className="flex items-center justify-end gap-3 px-4 py-4 border-t shrink-0"
                style={{ paddingBottom: `calc(1rem + ${insets.bottom}px)` }}
              >
                {footerContent}
              </div>
            )}

            {/* Loading overlay */}
            {loading && (
              <div className="absolute inset-0 flex items-center justify-center bg-bg-80">
                <Loader2 className="h-8 w-8 animate-spin text-muted-foreground" />
              </div>
            )}
          </div>
        </div>
      ) : null, getPortalRoot()
    )
  }

  // Desktop render
  return createPortal(
    <>
      {/* Backdrop */}
      {open && (
        <div
          className={cn("fixed inset-0 bg-overlay-heavy backdrop-blur-sm animate-in fade-in duration-200", overlayZIndex)}
          onClick={(e) => { e.stopPropagation(); if (!isDisabled) handleClose() }}
        />
      )}

      {/* Dialog */}
      {open && (
        <div
          className={cn(
            'fixed left-1/2 top-1/2',
            dialogZIndex,
            'grid w-full gap-0',
            'bg-popover border shadow-lg',
            'duration-200',
            'animate-in fade-in zoom-in-95 slide-in-from-left-1/2 slide-in-from-top-[48%]',
            'rounded-lg sm:rounded-xl',
            'overflow-hidden',
            'max-h-[calc(100dvh-2rem)] sm:max-h-[85dvh]',
            'flex flex-col',
            widthClasses[width],
            '-translate-x-1/2 -translate-y-1/2',
            className
          )}
          style={{ maxHeight: '85vh' }}
          // Portal content bubbles through the React tree. Stop clicks (incl.
          // the header ✕ and footer Close) from reaching ancestor handlers
          // such as a table row's onRowClick.
          onClick={(e) => e.stopPropagation()}
        >
          {/* Header */}
          <div className="flex items-center justify-between gap-2 px-6 py-4 border-b shrink-0">
            <div className="flex items-center gap-2 flex-1 min-w-0">
              {icon && <span className="text-muted-foreground shrink-0">{icon}</span>}
              <h2 className="text-lg font-semibold leading-none truncate">{title}</h2>
            </div>
            <button
              onClick={handleClose}
              disabled={isSubmitting && preventCloseOnSubmit}
              className="inline-flex items-center justify-center rounded-sm opacity-70 ring-offset-background transition-opacity hover:opacity-100 focus:outline-none focus:ring-2 focus:ring-ring focus:ring-offset-2 disabled:pointer-events-none shrink-0"
            >
              <X className="h-4 w-4" />
              <span className="sr-only">{t('close', 'Close')}</span>
            </button>
          </div>

          {/* Content */}
          <div
            ref={contentRef}
            className={cn("flex-1 overflow-y-auto px-6 py-4", contentClassName)}
          >
            {loading ? (
              <div className="flex items-center justify-center py-8">
                <Loader2 className="h-6 w-6 animate-spin text-muted-foreground" />
              </div>
            ) : (
              children
            )}
          </div>

          {/* Error message */}
          {submitError && (
            <div className="px-6 py-3 bg-muted border-t border-error">
              <div className="flex items-center gap-2 text-sm text-error">
                <AlertCircle className="h-4 w-4 shrink-0" />
                <span>{submitError}</span>
              </div>
            </div>
          )}

          {/* Footer — no bg: inherits bg-popover so it matches the header and
              content. A bg-muted-30 wash here read as #FBFBFC, visibly
              different from the white body. border-t alone separates
              content from actions (same approach as the mobile footer). */}
          {footerContent && (
            <div className="flex items-center justify-end gap-2 px-6 py-4 border-t shrink-0">
              {footerContent}
            </div>
          )}
        </div>
      )}
    </>,
    getPortalRoot()
  )
}

export default UnifiedFormDialog
