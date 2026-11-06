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
import { ChevronLeft } from "lucide-react"
import { cn } from "@/lib/utils"
import { useIsMobile } from "@/hooks/useMobile"
import { Button } from "@/components/ui/button"

export interface MobilePageHeaderProps {
  /** Left-aligned page title (required). */
  title: ReactNode
  /** Right slot for page actions (add, search, refresh). */
  actions?: ReactNode
  /** Kept for caller compatibility; global navigation now lives in the bottom capsule. */
  hideMenu?: boolean
  /** Optional left slot rendered AFTER the hamburger (e.g., back chevron on sub-pages). */
  leftExtra?: ReactNode
  className?: string
}

export function MobilePageHeader({
  title,
  actions,
  leftExtra,
  className,
}: MobilePageHeaderProps) {
  const isMobile = useIsMobile()
  const { t } = useTranslation("common")
  const location = useLocation()
  const navigate = useNavigate()
  if (!isMobile) return null

  const systemChildRoots = ["/visual-dashboard", "/automation", "/data", "/messages", "/extensions", "/settings"]
  const showSystemBack = systemChildRoots.some(
    (path) => location.pathname === path || location.pathname.startsWith(`${path}/`),
  )

  return (
    <div
      className={cn(
        // bg via inline style using --chrome (solid white in light mode,
        // solid elevated gray in dark mode) — this is the dedicated token
        // for stable chrome layers per index.css. Using bg-background would
        // tint the header gray after the background token was darkened.
        "safe-top sticky top-0 z-30 flex items-center gap-2 px-4",
        className,
      )}
      style={{
        backgroundColor: 'var(--chrome)',
        height: 'calc(4rem + env(safe-area-inset-top, 0px) + var(--titlebar-inset, 0px))',
      }}
    >
      {/* Optional local navigation, such as a back or history button. */}
      <div className="flex min-w-0 shrink-0 items-center gap-1 [&_button]:rounded-full [&_button]:border [&_button]:border-glass-border [&_button]:bg-surface-glass [&_button]:shadow-sm [&_button]:backdrop-blur-xl">
        {showSystemBack && (
          <Button
            variant="ghost"
            size="icon"
            className="-ml-2 h-11 w-11 shrink-0"
            onClick={() => navigate("/system")}
            aria-label={t("back", { defaultValue: "Back" })}
          >
            <ChevronLeft className="h-6 w-6" />
          </Button>
        )}
        {leftExtra}
      </div>
      <h1 className="min-w-0 flex-1 truncate text-left text-2xl font-bold leading-8 tracking-[-0.025em] text-foreground">
        {title}
      </h1>
      {/* Right slot: actions */}
      <div className="flex min-w-0 shrink-0 items-center gap-1 [&_button]:rounded-full [&_button]:border [&_button]:border-glass-border [&_button]:bg-surface-glass [&_button]:shadow-sm [&_button]:backdrop-blur-xl [&_button:hover]:border-glass-border-hover [&_button:hover]:bg-muted">
        {actions}
      </div>
    </div>
  )
}
