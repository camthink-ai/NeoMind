/**
 * AppSidebar — the desktop app navigation column.
 *
 * Replaces the nav icons that used to live in the TopNav: a persistent left
 * column with grouped entries, collapsible to an icon rail. Mobile keeps its
 * own drawer (MobileNav) — this component renders nothing below md.
 *
 * Layout contract: the sidebar is an in-flow flex column (not fixed/overlay),
 * so the app shell (App.tsx) lays out [AppSidebar][TopBar + main] side by
 * side. Height matches the shell; the header row aligns with the TopBar's
 * h-12 so the top edges read as one continuous chrome band.
 *
 * macOS Tauri overlay titlebar: the traffic lights float over the TOP-LEFT of
 * the window — which is this sidebar's header. The header therefore reserves
 * `--titlebar-inset` (set by TopBar) above the brand row, and doubles as a
 * window drag region alongside the TopBar.
 */

import { useCallback, useLayoutEffect } from "react"
import { useTranslation } from "react-i18next"
import { Link, useLocation, useNavigate } from "react-router-dom"
import { startTransition } from "react"
import { PanelLeftClose, PanelLeftOpen, Settings } from "lucide-react"
import { cn } from "@/lib/utils"
import { isTauriEnv } from "@/lib/api"
import { getCurrentWindow } from "@tauri-apps/api/window"
import { useStore } from "@/store"
import { Button } from "@/components/ui/button"
import { BrandLogoWithName } from "@/components/shared/BrandName"
import {
  Tooltip,
  TooltipContent,
  TooltipProvider,
  TooltipTrigger,
} from "@/components/ui/tooltip"
import {
  navItems,
  isNavItemActive,
  PRIMARY_NAV_IDS,
  SYSTEM_NAV_IDS,
  type NavItem,
} from "./navItems"
import { textMini } from "@/design-system/tokens/typography"

const HEADER_ROW_H = "h-12"

// Tauri window drag — same contract as the TopBar: startDragging on mousedown
// over non-interactive areas (unreliable to use data-tauri-drag-region in
// Tauri 2 overlay mode).
const handleDragMouseDown = (e: React.MouseEvent) => {
  if (!isTauriEnv()) return
  const target = e.target as HTMLElement
  if (target.closest("button, a, input, select, textarea, [role='button'], [role='tab']")) return
  getCurrentWindow().startDragging()
}

export function AppSidebar() {
  const { t } = useTranslation("common")
  const location = useLocation()
  const navigate = useNavigate()
  const collapsed = useStore((s) => s.appSidebarCollapsed)
  const toggleCollapsed = useStore((s) => s.toggleAppSidebar)
  const openSettings = useStore((s) => s.openSettings)

  const handleNavigate = useCallback(
    (path: string) => startTransition(() => navigate(path)),
    [navigate]
  )

  // Export the sidebar width as a CSS var so fixed full-bleed surfaces (e.g.
  // ChatPage's keyboard-aware container) can offset left of it. Unmounts
  // (mobile) reset to 0. Layout effect so the var is set before first paint.
  const widthPx = collapsed ? 60 : 240
  useLayoutEffect(() => {
    document.documentElement.style.setProperty("--app-sidebar-width", `${widthPx}px`)
    return () => {
      document.documentElement.style.setProperty("--app-sidebar-width", "0px")
    }
  }, [widthPx])

  const renderItem = (item: NavItem) => {
    const Icon = item.icon
    const isActive = isNavItemActive(item, location.pathname)
    const label = t(item.labelKey)

    const button = (
      <Button
        variant="ghost"
        aria-label={label}
        className={cn(
          "h-10 gap-3 px-3 justify-start font-normal no-press-scale",
          collapsed && "w-10 min-w-0 px-0 justify-center",
          isActive
            ? "bg-brand-bg text-brand hover:bg-brand-bg hover:text-brand"
            : "text-muted-foreground hover:text-foreground hover:bg-muted-50"
        )}
        aria-current={isActive ? "page" : undefined}
        onClick={() => handleNavigate(item.path)}
      >
        <Icon
          className={cn("h-5 w-5 shrink-0", isActive && "brand-icon-stroke")}
        />
        {!collapsed && <span className="truncate">{label}</span>}
      </Button>
    )

    if (!collapsed) return <div key={item.id}>{button}</div>

    return (
      <Tooltip key={item.id}>
        <TooltipTrigger asChild>{button}</TooltipTrigger>
        <TooltipContent side="right" className="text-xs px-2 py-1">
          {label}
        </TooltipContent>
      </Tooltip>
    )
  }

  const primaryItems = navItems.filter((i) => PRIMARY_NAV_IDS.includes(i.id))
  const systemItems = navItems.filter((i) => SYSTEM_NAV_IDS.includes(i.id))

  return (
    <TooltipProvider delayDuration={500}>
      <aside
        className={cn(
          "flex shrink-0 flex-col bg-[var(--chrome)] border-r border-border",
          "transition-[width] duration-normal ease-out"
        )}
        style={{ width: collapsed ? 60 : 240 }}
        aria-label={t("nav.primary")}
      >
        {/* Header — brand + collapse toggle. Reserves the macOS traffic-light
            strip (--titlebar-inset) and doubles as a drag region. */}
        <div
          className={cn(
            "flex items-center gap-1.5 px-3",
            HEADER_ROW_H,
            collapsed && "px-0 justify-center gap-0"
          )}
          style={{
            paddingTop: "calc(env(safe-area-inset-top, 0px) + var(--titlebar-inset, 0px))",
          }}
          onMouseDown={handleDragMouseDown}
        >
          {!collapsed && (
            <Link
              to="/chat"
              className="flex min-w-0 flex-1 items-center justify-start"
            >
              <BrandLogoWithName />
            </Link>
          )}
          <Tooltip>
            <TooltipTrigger asChild>
              <Button
                variant="ghost"
                size="icon-sm"
                className="text-muted-foreground hover:text-foreground no-press-scale"
                onClick={toggleCollapsed}
                aria-label={t(collapsed ? "nav.expandSidebar" : "nav.collapseSidebar")}
                aria-expanded={!collapsed}
              >
                {collapsed ? (
                  <PanelLeftOpen className="h-4 w-4" />
                ) : (
                  <PanelLeftClose className="h-4 w-4" />
                )}
              </Button>
            </TooltipTrigger>
            <TooltipContent side="right" className="text-xs px-2 py-1">
              {t(collapsed ? "nav.expandSidebar" : "nav.collapseSidebar")}
            </TooltipContent>
          </Tooltip>
        </div>

        {/* Nav groups */}
        <nav className={cn("flex flex-col gap-0.5 px-2 pb-2", collapsed && "px-2.5")}>
          {primaryItems.map(renderItem)}

          <div className={cn("mt-3 mb-1", collapsed && "mx-auto my-3 h-px w-6 bg-border")} />
          {!collapsed && (
            <div className={cn("px-3 pb-1 font-medium uppercase tracking-wide text-muted-foreground/70", textMini)}>
              {t("navShort.system")}
            </div>
          )}
          {systemItems.map(renderItem)}
        </nav>

        <div className="flex-1" />

        {/* Footer — settings entry (dialog, not a route) */}
        <div className={cn("border-t border-border p-2 pb-[calc(0.5rem+env(safe-area-inset-bottom,0px))]", collapsed && "px-2.5")}>
          <Button
            variant="ghost"
            className={cn(
              "h-10 w-full gap-3 px-3 justify-start font-normal text-muted-foreground hover:text-foreground hover:bg-muted-50 no-press-scale",
              collapsed && "w-10 min-w-0 px-0 justify-center"
            )}
            aria-label={t("nav.settings")}
            onClick={() => openSettings()}
          >
            <Settings className="h-5 w-5 shrink-0" />
            {!collapsed && <span className="truncate">{t("nav.settings")}</span>}
          </Button>
        </div>
      </aside>
    </TooltipProvider>
  )
}
