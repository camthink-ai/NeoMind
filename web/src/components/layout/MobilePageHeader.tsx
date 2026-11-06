/**
 * MobilePageHeader - Per-page sticky top bar (mobile-only)
 *
 * Layout: [☰ drawer] [leftExtra?] [left-aligned title] [actions?]
 * The title grows into the available space and truncates before the actions.
 *
 * The bar is h-16 (64px) + safe-area-top, sticky to the top of the scroll
 * container, with bg-background that extends under the status bar.
 *
 * Desktop layout does not render this component.
 */

import { type ReactNode } from "react"
import { useTranslation } from "react-i18next"
import { Menu } from "lucide-react"
import { cn } from "@/lib/utils"
import { useIsMobile } from "@/hooks/useMobile"
import { useMobileNav } from "@/store/mobileNav"
import { Button } from "@/components/ui/button"
import { useLocation } from "react-router-dom"

export interface MobilePageHeaderProps {
  /** Left-aligned page title (required). */
  title: ReactNode
  /** Right slot for page actions (add, search, refresh). */
  actions?: ReactNode
  /** Hide the hamburger (default: false). Useful when a custom back button replaces it. */
  hideMenu?: boolean
  /** Optional left slot rendered AFTER the hamburger (e.g., back chevron on sub-pages). */
  leftExtra?: ReactNode
  className?: string
}

export function MobilePageHeader({
  title,
  actions,
  hideMenu = false,
  leftExtra,
  className,
}: MobilePageHeaderProps) {
  const isMobile = useIsMobile()
  const { t } = useTranslation("common")
  const setOpen = useMobileNav((s) => s.setOpen)
  const location = useLocation()
  if (!isMobile) return null

  // The global bottom bar is the navigation entry on regular mobile pages.
  // Chat hides that bar to protect the composer, so it keeps the menu button.
  const showMenu = !hideMenu && (location.pathname === "/" || location.pathname.startsWith("/chat"))

  return (
    <div
      className={cn(
        // bg via inline style using --chrome (solid white in light mode,
        // solid elevated gray in dark mode) — this is the dedicated token
        // for stable chrome layers per index.css. Using bg-background would
        // tint the header gray after the background token was darkened.
        "safe-top sticky top-0 z-30 flex h-16 items-center gap-2 px-4",
        className,
      )}
      style={{ backgroundColor: 'var(--chrome)' }}
    >
      {/* Left slot: hamburger + leftExtra (e.g. back chevron) */}
      <div className="flex min-w-0 shrink-0 items-center gap-1">
        {showMenu && (
          <Button
            variant="ghost"
            size="icon"
            // 44×44 = Apple HIG / Material minimum touch target. The previous
            // h-9 w-9 (36px) was below the recommended minimum and small
            // finger drift would miss the tap, making it feel like the menu
            // "didn't open" on first try.
            className="-ml-2 h-11 w-11 shrink-0"
            onClick={() => setOpen(true)}
            aria-label={t("system.menu")}
          >
            <Menu className="h-6 w-6" />
          </Button>
        )}
        {leftExtra}
      </div>
      <h1 className="min-w-0 flex-1 truncate text-left text-2xl font-bold leading-8 tracking-[-0.025em] text-foreground">
        {title}
      </h1>
      {/* Right slot: actions */}
      <div className="flex min-w-0 shrink-0 items-center gap-1">
        {actions}
      </div>
    </div>
  )
}
