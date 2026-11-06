/**
 * MobilePageHeader - Per-page sticky top bar (mobile-only)
 *
 * Layout: [☰ drawer] [leftExtra?] [title — centered] [actions?]
 * The center holds ONLY the title. No subtitle, no extra content.
 *
 * The bar is h-12 (48px) + safe-area-top, sticky to the top of the scroll
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
  /** Center title (required). */
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
        // 3-column grid: [left 1fr][title auto][right 1fr]. Equal side
        // columns make the title visually centered regardless of how many
        // action buttons either side has — flex-1 + text-center was offset
        // when the action stack was wider than the hamburger.
        //
        // bg via inline style using --chrome (solid white in light mode,
        // solid elevated gray in dark mode) — this is the dedicated token
        // for stable chrome layers per index.css. Using bg-background would
        // tint the header gray after the background token was darkened.
        "safe-top sticky top-0 z-30 grid h-14 grid-cols-[1fr_auto_1fr] items-center gap-1 px-3",
        className,
      )}
      style={{ backgroundColor: 'var(--chrome)' }}
    >
      {/* Left slot: hamburger + leftExtra (e.g. back chevron) */}
      <div className="flex min-w-0 items-center gap-1 justify-self-start">
        {showMenu && (
          <Button
            variant="ghost"
            size="icon"
            // 44×44 = Apple HIG / Material minimum touch target. The previous
            // h-9 w-9 (36px) was below the recommended minimum and small
            // finger drift would miss the tap, making it feel like the menu
            // "didn't open" on first try.
            className="-ml-1 h-11 w-11 shrink-0"
            onClick={() => setOpen(true)}
            aria-label={t("system.menu")}
          >
            <Menu className="h-6 w-6" />
          </Button>
        )}
        {leftExtra}
      </div>
      {/* Center title — clamp width so very long titles truncate instead of
          pushing the side columns and breaking centering. */}
      <span className="min-w-0 max-w-[65vw] truncate text-center text-[17px] font-semibold tracking-tight text-foreground">
        {title}
      </span>
      {/* Right slot: actions */}
      <div className="flex min-w-0 items-center gap-1 justify-self-end">
        {actions}
      </div>
    </div>
  )
}
