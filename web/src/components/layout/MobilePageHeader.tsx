/**
 * MobilePageHeader - Per-page sticky top bar (mobile-only)
 *
 * Layout: [leftExtra?] [left-aligned title] [actions?]
 * The title grows into the available space and truncates before the actions.
 *
 * The bar is h-16 (64px) + safe-area-top, sticky to the top of the scroll
 * container, with bg-background that extends under the status bar.
 *
 * Desktop layout does not render this component.
 */

import { type ReactNode } from "react"
import { useTranslation } from "react-i18next"
import { useLocation, useNavigate } from "react-router-dom"
import { ChevronLeft, Menu } from "lucide-react"
import { cn } from "@/lib/utils"
import { useIsMobile } from "@/hooks/useMobile"
import { useMobileNav } from "@/store/mobileNav"
import { Button } from "@/components/ui/button"

export interface MobilePageHeaderProps {
  /** Page title (required). */
  title: ReactNode
  /** Right slot for page actions (add, search, refresh). */
  actions?: ReactNode
  /** Kept for caller compatibility; global navigation now lives in the bottom capsule. */
  hideMenu?: boolean
  /** Renders a hamburger that opens the MobileNav drawer. Used on pages
   * WITHOUT the bottom capsule (Chat) — it is their only section exit. */
  menu?: boolean
  /** Optional left slot rendered AFTER the hamburger (e.g., back chevron on sub-pages). */
  leftExtra?: ReactNode
  /** Title alignment. Chat uses center so the session name sits between equal side actions. */
  titleAlign?: "start" | "center"
  /**
   * Fully transparent overlay chrome (Chat). Business pages keep the opaque
   * sticky bar — DESIGN_SPEC: opaque navigation avoids WebKit paint flashes.
   */
  scrim?: boolean
  className?: string
}

export function MobilePageHeader({
  title,
  actions,
  menu,
  leftExtra,
  titleAlign = "start",
  scrim = false,
  className,
}: MobilePageHeaderProps) {
  const isMobile = useIsMobile()
  const { t } = useTranslation("common")
  const location = useLocation()
  const navigate = useNavigate()
  const setDrawerOpen = useMobileNav((s) => s.setOpen)
  if (!isMobile) return null

  const systemChildRoots = ["/devices", "/agents", "/visual-dashboard", "/automation", "/data", "/messages", "/extensions", "/settings"]
  const showSystemBack = systemChildRoots.some(
    (path) => location.pathname === path || location.pathname.startsWith(`${path}/`),
  )

  const hasLeading = Boolean(menu || leftExtra || showSystemBack)
  const centered = titleAlign === "center"

  return (
    <div
      className={cn(
        "safe-top z-30 px-4",
        // Opaque sticky is the default chrome contract. Scrim = light fade mask
        // so conversation can read through while titles stay legible.
        scrim ? "relative" : "sticky top-0 bg-background",
        centered ? "grid grid-cols-[2.5rem_minmax(0,1fr)_2.5rem] items-center gap-2" : "flex items-center gap-2",
        className,
      )}
      style={{
        height: 'calc(4rem + env(safe-area-inset-top, 0px) + var(--titlebar-inset, 0px))',
        ...(scrim
          ? {
              backgroundImage:
                'linear-gradient(to bottom, var(--background) 0%, color-mix(in srgb, var(--background) 70%, transparent) 45%, transparent 100%)',
            }
          : {}),
      }}
    >
      {/* Local navigation (menu, back) — plain ghost by default; callers may
          wrap leftExtra in their own glass chip (Chat). */}
      <div className="flex min-w-0 shrink-0 items-center justify-self-start">
        {menu && (
          <Button
            variant="ghost"
            size="icon"
            className="-ml-1.5 h-10 w-10 shrink-0"
            onClick={() => setDrawerOpen(true)}
            aria-label={t("system.menu")}
            data-testid="mobile-page-menu"
          >
            <Menu className="h-5 w-5" />
          </Button>
        )}
        {showSystemBack && !leftExtra && (
          <Button
            variant="ghost"
            size="icon"
            className="-ml-1.5 h-10 w-10 shrink-0"
            onClick={() => navigate("/system")}
            aria-label={t("back", { defaultValue: "Back" })}
          >
            <ChevronLeft className="h-5 w-5" />
          </Button>
        )}
        {leftExtra}
      </div>
      <h1 className={cn(
        "min-w-0 truncate tracking-[-0.02em] text-foreground",
        centered
          // Cap inside the middle grid cell so long titles never crowd the
          // side capsules (grid already keeps 2.5rem + gap-2 gutters).
          ? "mx-auto w-full max-w-[min(100%,12.5rem)] justify-self-center px-1 text-center"
          : "flex-1 text-left",
        hasLeading ? "text-lg font-semibold leading-6" : "text-2xl font-bold leading-8",
      )}>
        {title}
      </h1>
      {/* Right slot: actions — glass capsule chrome */}
      <div className={cn(
        "flex min-w-0 shrink-0 items-center gap-1 [&_button]:rounded-full [&_button]:border [&_button]:border-glass-border [&_button]:bg-surface-glass [&_button]:shadow-sm [&_button]:backdrop-blur-xl [&_button:hover]:border-glass-border-hover [&_button:hover]:bg-muted",
        centered && "justify-self-end",
      )}>
        {actions}
      </div>
    </div>
  )
}
