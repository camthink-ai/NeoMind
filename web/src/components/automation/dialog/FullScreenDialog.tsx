/**
 * FullScreenDialog Component
 *
 * Unified full-screen dialog with glassmorphism effect.
 * Used for complex forms like TransformBuilder, RuleBuilder, AgentEditor.
 */

import { ReactNode, useEffect } from 'react'
import { createPortal } from 'react-dom'
import { X } from 'lucide-react'
import { cn } from '@/lib/utils'
import { useBodyScrollLock } from '@/hooks/useBodyScrollLock'
import { useIsMobile, useSafeAreaInsets } from '@/hooks/useMobile'

export interface FullScreenDialogProps {
  open: boolean
  onOpenChange: (open: boolean) => void
  children: ReactNode
  /** Disable closing by backdrop click */
  disableBackdropClose?: boolean
  /** Additional className for the dialog container */
  className?: string
  /** Z-index for the dialog (default: 100). Use 110 for nested dialogs. */
  zIndex?: number
}

export function FullScreenDialog({
  open,
  onOpenChange,
  children,
  disableBackdropClose = false,
  className,
  zIndex = 100,
}: FullScreenDialogProps) {
  const isMobile = useIsMobile()
  const insets = useSafeAreaInsets()

  // Lock body scroll when dialog is open
  useBodyScrollLock(open, { mobileOnly: true })

  // Handle Escape key to close dialog
  useEffect(() => {
    if (!open) return
    const handleKeyDown = (e: KeyboardEvent) => {
      if (e.key === 'Escape') {
        e.preventDefault()
        onOpenChange(false)
      }
    }
    window.addEventListener('keydown', handleKeyDown)
    return () => window.removeEventListener('keydown', handleKeyDown)
  }, [open, onOpenChange])

  // Get dialog root for portal rendering
  const dialogRoot = typeof document !== 'undefined'
    ? document.getElementById('dialog-root') || document.body
    : null

  if (!dialogRoot) return null

  return createPortal(
    <div
      className={cn(
        "fixed inset-0 flex flex-col",
        // Glassmorphism background - lower opacity to show content behind
        "bg-overlay-light",
        "backdrop-blur-sm",
        !open && "hidden"
      )}
      style={{ zIndex }}
      onClick={() => !disableBackdropClose && onOpenChange(false)}
    >
      {/* Inner container - prevents click propagation */}
      <div
        className={cn(
          "flex flex-col flex-1 overflow-hidden",
          // Full-bleed: opaque bg-background on desktop (page-like, max space
          // for heavy editors); --chrome on mobile via inline style.
          isMobile ? "" : "bg-popover",
          className
        )}
        onClick={(e) => e.stopPropagation()}
        style={{
          // Reserve the macOS title-bar (traffic-light) inset so the
          // header/content isn't covered by the overlay traffic lights.
          paddingTop: "calc(env(safe-area-inset-top, 0px) + var(--titlebar-inset, 0px))",
          ...(isMobile
            ? {
                backgroundColor: "var(--chrome)",
                paddingBottom: "env(safe-area-inset-bottom, 0px)",
              }
            : {}),
        }}
      >
        {children}
      </div>
    </div>,
    dialogRoot
  )
}

// ============================================================================
// Header Component
// ============================================================================

export interface FullScreenDialogHeaderProps {
  title: string
  onClose: () => void
  /** Actions to show on the right side */
  actions?: ReactNode
  /** @deprecated no longer rendered — kept so callers compile */
  icon?: ReactNode
  /** @deprecated */
  iconBg?: string
  /** @deprecated */
  iconColor?: string
  /** @deprecated */
  subtitle?: string
}

export function FullScreenDialogHeader({
  title,
  subtitle,
  onClose,
  actions,
}: FullScreenDialogHeaderProps) {
  return (
    <header className="shrink-0 flex items-center justify-between gap-3 px-4 md:px-6 h-14 border-b border-border">
      <div className="min-w-0 flex-1">
        <h1 className="text-base md:text-lg font-semibold truncate text-foreground">
          {title}
        </h1>
        {subtitle && (
          <p className="text-nano text-muted-foreground truncate">{subtitle}</p>
        )}
      </div>
      <div className="flex items-center gap-2 shrink-0">
        {actions}
        <button
          onClick={onClose}
          className={cn(
            "shrink-0 flex items-center justify-center w-8 h-8 md:w-9 md:h-9",
            "rounded-lg text-muted-foreground hover:text-foreground",
            "bg-black/5 dark:bg-white/5 hover:bg-black/10 dark:hover:bg-white/10",
            "transition-all"
          )}
        >
          <X className="w-5 h-5" />
        </button>
      </div>
    </header>
  )
}

// ============================================================================
// Content Component
// ============================================================================

export interface FullScreenDialogContentProps {
  children: ReactNode
  className?: string
}

export function FullScreenDialogContent({
  children,
  className,
}: FullScreenDialogContentProps) {
  return (
    <div className={cn("flex-1 overflow-hidden flex", className)}>
      {children}
    </div>
  )
}

// ============================================================================
// Footer Component
// ============================================================================

export interface FullScreenDialogFooterProps {
  children: ReactNode
  className?: string
}

export function FullScreenDialogFooter({
  children,
  className,
}: FullScreenDialogFooterProps) {
  return (
    <footer
      className={cn(
        "shrink-0 flex items-center justify-end gap-2 md:gap-3",
        "px-3 md:px-5 lg:px-6 py-3 md:py-4",
        "border-t border-border",
        // No bg: footer inherits the container's bg-popover (white) so it
        // matches the body. A bg-black/[0.02] tint here read as #FBFBFC,
        // a visible mismatch vs the white content (the old "glassmorphism
        // bg-bg-95 container" rationale is stale — the container is now
        // opaque bg-popover). border-t alone separates content from actions.
        className
      )}
    >
      {children}
    </footer>
  )
}

// ============================================================================
// Sidebar Component
// ============================================================================

export interface FullScreenDialogSidebarProps {
  children: ReactNode
  className?: string
  /** Hide on mobile */
  hideOnMobile?: boolean
}

export function FullScreenDialogSidebar({
  children,
  className,
  hideOnMobile = true,
}: FullScreenDialogSidebarProps) {
  const isMobile = useIsMobile()

  if (isMobile && hideOnMobile) return null

  return (
    <aside className={cn(
      "shrink-0 w-[180px] md:w-[220px] border-r border-border",
      // Desktop-only tint (see FullScreenDialogFooter for rationale).
      "md:bg-black/[0.02] md:dark:bg-white/[0.02]",
      className
    )}>
      {children}
    </aside>
  )
}

// ============================================================================
// Main Content Component
// ============================================================================

export interface FullScreenDialogMainProps {
  children: ReactNode
  className?: string
}

export function FullScreenDialogMain({
  children,
  className,
}: FullScreenDialogMainProps) {
  return (
    <main className={cn("flex-1 overflow-y-auto", className)}>
      {children}
    </main>
  )
}
