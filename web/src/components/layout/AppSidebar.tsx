/**
 * AppSidebar — the desktop app navigation column.
 *
 * Navigation lives here (not in the TopBar): grouped entries in the body;
 * the footer carries the settings entry and, at the very bottom-left, the
 * user avatar whose dropdown holds theme / language / settings / about /
 * logout. The TopBar keeps the instance selector, onboarding guide and
 * alerts bell. Mobile keeps its own drawer (MobileNav); this column renders
 * nothing below md.
 *
 * Layout contract: in-flow flex column (not fixed/overlay) — the app shell
 * (App.tsx) lays out [AppSidebar][TopBar + main] side by side. The header
 * row aligns with the TopBar's h-12 and both carry border-b, so the top
 * chrome band reads as one continuous surface with a single dividing line.
 *
 * macOS Tauri overlay titlebar: the traffic lights float over the TOP-LEFT
 * of the window — this sidebar's header. The header reserves
 * `--titlebar-inset` (set by TopBar) above the brand row, and doubles as a
 * window drag region alongside the TopBar.
 */

import { useCallback, useLayoutEffect } from "react"
import { useTranslation } from "react-i18next"
import { Link, useLocation, useNavigate } from "react-router-dom"
import { startTransition } from "react"
import { PanelLeftClose, PanelLeftOpen, Settings, Sun, Languages, Info, LogOut } from "lucide-react"
import { cn } from "@/lib/utils"
import { isTauriEnv } from "@/lib/api"
import { getCurrentWindow } from "@tauri-apps/api/window"
import { useStore } from "@/store"
import { Button } from "@/components/ui/button"
import { Badge } from "@/components/ui/badge"
import { Avatar, AvatarFallback } from "@/components/ui/avatar"
import { useTheme } from "@/components/ui/theme"
import { BrandLogoWithName } from "@/components/shared/BrandName"
import {
  Tooltip,
  TooltipContent,
  TooltipProvider,
  TooltipTrigger,
} from "@/components/ui/tooltip"
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
  DropdownMenuSub,
  DropdownMenuSubTrigger,
  DropdownMenuSubContent,
  DropdownMenuPortal,
  DropdownMenuRadioGroup,
  DropdownMenuRadioItem,
} from "@/components/ui/dropdown-menu"
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
  const { t, i18n } = useTranslation("common")
  const location = useLocation()
  const navigate = useNavigate()
  const collapsed = useStore((s) => s.appSidebarCollapsed)
  const toggleCollapsed = useStore((s) => s.toggleAppSidebar)
  const openSettings = useStore((s) => s.openSettings)
  const user = useStore((s) => s.user)
  const logout = useStore((s) => s.logout)
  const { theme, setTheme } = useTheme()

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
          // w-full so every item — and the active row — spans the sidebar
          // uniformly instead of sizing to its label
          "h-10 w-full gap-3 px-3 justify-start font-normal no-press-scale",
          collapsed && "w-10 min-w-0 px-0 justify-center",
          isActive
            ? "bg-muted text-foreground font-medium hover:bg-muted"
            : "text-muted-foreground hover:text-foreground hover:bg-muted-50"
        )}
        aria-current={isActive ? "page" : undefined}
        onClick={() => handleNavigate(item.path)}
      >
        {/* Accent stays restrained (reference palette): neutral active row,
            brand color only tints the active icon */}
        <Icon className={cn("h-5 w-5 shrink-0", isActive && "text-brand")} />
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

  const getUserInitials = (username: string) => username.slice(0, 2).toUpperCase()

  // User entry — bottom-left. Avatar + name (expanded) / avatar only
  // (collapsed); dropdown opens upward.
  const userEntry = user && (
    <DropdownMenu>
      <DropdownMenuTrigger asChild>
        <Button
          variant="ghost"
          aria-label={user.username}
          className={cn(
            "h-10 w-full gap-3 px-2 justify-start font-normal text-muted-foreground hover:text-foreground hover:bg-muted-50 no-press-scale",
            collapsed && "w-10 min-w-0 px-0 justify-center"
          )}
        >
          <Avatar className="h-7 w-7 shrink-0 cursor-pointer rounded-full ring-2 ring-background">
            <AvatarFallback className="bg-primary text-primary-foreground text-xs font-semibold">
              {getUserInitials(user.username)}
            </AvatarFallback>
          </Avatar>
          {!collapsed && (
            <span className="truncate text-sm font-medium text-foreground">
              {user.username}
            </span>
          )}
        </Button>
      </DropdownMenuTrigger>
      <DropdownMenuContent side="top" align="start" className="w-56">
        <div className="px-3 py-2">
          <div className="flex items-center justify-between gap-2">
            <p className="text-sm font-medium truncate">{user.username}</p>
            {user.role && (
              <Badge variant="outline" className="text-xs shrink-0">
                {user.role}
              </Badge>
            )}
          </div>
        </div>
        <DropdownMenuSeparator />
        <DropdownMenuSub>
          <DropdownMenuSubTrigger>
            <Sun className="h-4 w-4 mr-2" />
            {t('theme.title', { defaultValue: 'Theme' })}
          </DropdownMenuSubTrigger>
          <DropdownMenuPortal>
            <DropdownMenuSubContent>
              <DropdownMenuRadioGroup value={theme} onValueChange={(v) => setTheme(v as "light" | "dark" | "system")}>
                <DropdownMenuRadioItem value="light">{t('theme.light')}</DropdownMenuRadioItem>
                <DropdownMenuRadioItem value="dark">{t('theme.dark')}</DropdownMenuRadioItem>
                <DropdownMenuRadioItem value="system">{t('theme.system')}</DropdownMenuRadioItem>
              </DropdownMenuRadioGroup>
            </DropdownMenuSubContent>
          </DropdownMenuPortal>
        </DropdownMenuSub>
        <DropdownMenuSub>
          <DropdownMenuSubTrigger>
            <Languages className="h-4 w-4 mr-2" />
            {t('userMenu.language', { defaultValue: 'Language' })}
          </DropdownMenuSubTrigger>
          <DropdownMenuPortal>
            <DropdownMenuSubContent>
              <DropdownMenuRadioGroup value={i18n.language} onValueChange={(v) => i18n.changeLanguage(v)}>
                <DropdownMenuRadioItem value="zh">中文</DropdownMenuRadioItem>
                <DropdownMenuRadioItem value="en">English</DropdownMenuRadioItem>
              </DropdownMenuRadioGroup>
            </DropdownMenuSubContent>
          </DropdownMenuPortal>
        </DropdownMenuSub>
        <DropdownMenuSeparator />
        <DropdownMenuItem onClick={() => openSettings()}>
          <Settings className="h-4 w-4 mr-2" />
          {t('nav.settings')}
        </DropdownMenuItem>
        <DropdownMenuItem onClick={() => openSettings('about')}>
          <Info className="h-4 w-4 mr-2" />
          {t('userMenu.about')}
        </DropdownMenuItem>
        <DropdownMenuSeparator />
        <DropdownMenuItem onClick={logout} className="text-error focus:text-error">
          <LogOut className="h-4 w-4 mr-2" />
          {t('logout')}
        </DropdownMenuItem>
      </DropdownMenuContent>
    </DropdownMenu>
  )

  return (
    <TooltipProvider delayDuration={500}>
      <aside
        className={cn(
          "flex shrink-0 flex-col bg-[var(--sidebar-bg)]",
          "transition-[width] duration-normal ease-out"
        )}
        style={{ width: collapsed ? 60 : 240 }}
        aria-label={t("nav.primary")}
      >
        {/* Header — brand + collapse toggle. Reserves the macOS traffic-light
            strip (--titlebar-inset) and doubles as a drag region. No border:
            layers separate by background contrast (sidebar rail vs chrome). */}
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
        <nav className={cn("flex flex-col gap-1 px-2 pt-3 pb-2", collapsed && "px-2.5")}>
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

        {/* Footer — user avatar (bottom-left). Settings lives inside the
            avatar menu; no standalone entry. */}
        <div
          className={cn(
            "flex flex-col gap-1 border-t border-border p-2 pb-[calc(0.5rem+env(safe-area-inset-bottom,0px))]",
            collapsed && "px-2.5"
          )}
        >
          {userEntry}
        </div>
      </aside>
    </TooltipProvider>
  )
}
