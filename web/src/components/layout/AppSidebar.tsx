/**
 * AppSidebar — the desktop app's entire chrome: navigation + utilities.
 *
 * There is NO top bar — the sidebar is a full-height 60px ICON RAIL holding
 * the brand mark, the nav icons, and the utilities (instance, theme,
 * settings, alerts, onboarding, user avatar at the very bottom). The whole
 * window height belongs to content.
 *
 * Fixed COLLAPSED width (60px, no labels — hover tooltips carry the names,
 * no expand/collapse duality). Fixed width keeps --app-sidebar-width stable
 * for the fixed surfaces that offset past it (chat's keyboard container,
 * PageLayout's footer).
 *
 * macOS Tauri overlay titlebar: the traffic lights float over this header —
 * it reserves `--titlebar-inset` (set here) and is the window drag region.
 */

import { useCallback, useEffect, useLayoutEffect, useState } from "react"
import { useTranslation } from "react-i18next"
import { useLocation, useNavigate } from "react-router-dom"
import { startTransition } from "react"
import { Rocket, Settings, Sun, Languages, Info, LogOut, PanelLeftClose } from "lucide-react"
import { cn } from "@/lib/utils"
import { isTauriEnv } from "@/lib/api"
import { getCurrentWindow } from "@tauri-apps/api/window"
import { useStore } from "@/store"
import { Button } from "@/components/ui/button"
import { Badge } from "@/components/ui/badge"
import { Avatar, AvatarFallback } from "@/components/ui/avatar"
import { BrandLogo, BrandLogoHorizontal } from "@/components/shared/BrandName"
import { useTheme } from "@/components/ui/theme"
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
  type NavItem,
} from "./navItems"
import { InstanceSelector } from "./InstanceSelector"
import { OnboardingDialog } from "@/components/onboarding/OnboardingDialog"
import { useOnboarding } from "@/hooks/useOnboarding"
import { InstanceManagerDialog } from "@/components/instances/InstanceManagerDialog"

// 72px — still clears the macOS traffic lights (~14–66px from the window
// edge) while keeping the rail visually tight.
const SIDEBAR_WIDTH_PX = 72
// Expanded (logo click / collapse toggle): 224px — fits icon + label +
// padding with room for i18n labels and instance names.
const SIDEBAR_EXPANDED_PX = 224

// Tauri window drag — startDragging on mousedown over non-interactive areas
// (data-tauri-drag-region is unreliable in Tauri 2 overlay mode).
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
  const openSettings = useStore((s) => s.openSettings)
  const user = useStore((s) => s.user)
  const logout = useStore((s) => s.logout)
  const { theme, setTheme } = useTheme()
  const [instanceManagerOpen, setInstanceManagerOpen] = useState(false)
  const [onboardingOpen, setOnboardingOpen] = useState(false)
  // Logo click expands/collapses the rail (labels appear when expanded).
  // Always starts COLLAPSED — expansion is an explicit per-session choice,
  // never restored from a previous session.
  const [expanded, setExpanded] = useState(false)
  const { status: onboardingStatus, dismiss: dismissOnboarding, fetchStatus: fetchOnboardingStatus } = useOnboarding()
  useEffect(() => {
    fetchOnboardingStatus()
  }, [fetchOnboardingStatus])
  const onboardingIncomplete =
    !!onboardingStatus &&
    !onboardingStatus.dismissed &&
    (!onboardingStatus.steps.llm.completed || !onboardingStatus.steps.device.completed)

  // macOS Tauri overlay titlebar: expose the traffic-light inset so this
  // header and full-screen overlays (settings, onboarding) reserve it.
  const isMacTauri = isTauriEnv() && /Mac/i.test(navigator.platform || navigator.userAgent)
  useEffect(() => {
    // 32px — generous clearance for the macOS traffic lights (close/min/
    // zoom) floating over the rail's top-left, plus room for future custom
    // window controls. Full-screen overlays reserve the same strip.
    document.documentElement.style.setProperty("--titlebar-inset", isMacTauri ? "32px" : "0px")
  }, [isMacTauri])

  const handleNavigate = useCallback(
    (path: string) => startTransition(() => navigate(path)),
    [navigate]
  )

  // Fixed-width rail exported for fixed full-bleed surfaces (ChatPage's
  // keyboard-aware container, PageLayout's footer). 0 on unmount (mobile).
  // Width follows the expanded state so all offset surfaces shift together.
  useLayoutEffect(() => {
    const w = expanded ? SIDEBAR_EXPANDED_PX : SIDEBAR_WIDTH_PX
    document.documentElement.style.setProperty("--app-sidebar-width", `${w}px`)
    return () => {
      document.documentElement.style.setProperty("--app-sidebar-width", "0px")
    }
  }, [expanded])

  const renderItem = (item: NavItem) => {
    const Icon = item.icon
    const isActive = isNavItemActive(item, location.pathname)
    const label = t(item.labelKey)

    if (expanded) {
      // Expanded: icon + label, full-width row, no tooltip needed.
      return (
        <div key={item.id} className="w-full px-2 flex items-center">
          <Button
            variant="ghost"
            aria-label={label}
            aria-current={isActive ? "page" : undefined}
            className={cn(
              "w-full h-10 px-3 justify-start font-normal no-press-scale",
              isActive
                ? "bg-muted text-foreground hover:bg-muted"
                : "text-muted-foreground hover:text-foreground hover:bg-muted-50"
            )}
            onClick={() => handleNavigate(item.path)}
          >
            <Icon className="h-5 w-5 shrink-0" />
            <span className="ml-3 truncate text-sm">{label}</span>
          </Button>
        </div>
      )
    }

    return (
      <Tooltip key={item.id}>
        <TooltipTrigger asChild>
          <Button
            variant="ghost"
            aria-label={label}
            aria-current={isActive ? "page" : undefined}
            className={cn(
              "h-10 w-10 px-0 justify-center font-normal no-press-scale",
              isActive
                ? "bg-muted text-foreground hover:bg-muted"
                : "text-muted-foreground hover:text-foreground hover:bg-muted-50"
            )}
            onClick={() => handleNavigate(item.path)}
          >
            <Icon className="h-5 w-5 shrink-0" />
          </Button>
        </TooltipTrigger>
        <TooltipContent side="right" className="text-xs px-2 py-1">
          {label}
        </TooltipContent>
      </Tooltip>
    )
  }

  const getUserInitials = (username: string) => username.slice(0, 2).toUpperCase()

  // Footer row — full-width icon+label button when expanded, tooltip icon
  // when collapsed.
  const footerRow = (label: string, icon: React.ReactNode, onClick?: () => void, badge?: boolean, rowKey?: string) =>
    expanded ? (
      <div key={rowKey} className="w-full px-2 flex items-center">
        <Button
          variant="ghost"
          aria-label={label}
          className="w-full h-10 px-3 justify-start font-normal no-press-scale text-muted-foreground hover:text-foreground hover:bg-muted-50"
          onClick={onClick}
        >
          {icon}
          <span className="ml-3 truncate text-sm">{label}</span>
          {badge && <span className="ml-2 h-1.5 w-1.5 rounded-full bg-primary shrink-0" />}
        </Button>
      </div>
    ) : (
      <Tooltip key={rowKey}>
        <TooltipTrigger asChild>
          <Button
            variant="ghost"
            aria-label={label}
            className="relative h-10 w-10 px-0 justify-center text-muted-foreground hover:text-foreground no-press-scale"
            onClick={onClick}
          >
            {icon}
            {badge && <span className="absolute top-1 right-1 h-1.5 w-1.5 rounded-full bg-primary" />}
          </Button>
        </TooltipTrigger>
        <TooltipContent side="right" className="text-xs px-2 py-1">{label}</TooltipContent>
      </Tooltip>
    )

  // User entry — avatar at the very bottom; dropdown opens upward.
  // Expanded: full-width row with username; collapsed: icon-only.
  const userEntry = user && (
    <DropdownMenu>
      <DropdownMenuTrigger asChild>
        <Button
          variant="ghost"
          aria-label={user.username}
          className={cn(
            "h-10 font-normal no-press-scale",
            expanded ? "w-full px-3 justify-start gap-3" : "w-10 px-0 justify-center"
          )}
        >
          <Avatar className="h-7 w-7 shrink-0 cursor-pointer rounded-full ring-2 ring-background">
            <AvatarFallback className="bg-primary text-primary-foreground text-xs font-semibold">
              {getUserInitials(user.username)}
            </AvatarFallback>
          </Avatar>
          {expanded && (
            <span className="truncate text-sm text-muted-foreground">{user.username}</span>
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
        className="flex shrink-0 flex-col items-center overflow-hidden bg-[var(--sidebar-bg)] border-r border-border transition-[width] duration-200 ease-out"
        style={{
          width: expanded ? SIDEBAR_EXPANDED_PX : SIDEBAR_WIDTH_PX,
          // Top strip: reserves the macOS traffic-light inset + safe area
          // (no brand mark anymore — the nav starts right below it) and
          // serves as the window drag region
          paddingTop: "calc(env(safe-area-inset-top, 0px) + var(--titlebar-inset, 0px))",
        }}
        onMouseDown={handleDragMouseDown}
      >
        {/* Brand mark — top of the rail. Clicking it expands/collapses the
            rail (labels on/off) instead of navigating (nav has /chat). */}
        <div className={cn("flex w-full items-center pt-5 pb-2", expanded ? "justify-between pl-2 pr-1" : "justify-center")}>
          <button
            type="button"
            onClick={() => setExpanded((e) => !e)}
            aria-label={t("nav.toggleSidebar", { defaultValue: "Toggle navigation" })}
            aria-expanded={expanded}
            className={cn("flex items-center justify-center rounded-lg hover:bg-muted-50 transition-colors no-press-scale", expanded ? "h-8 px-1" : "px-2")}
          >
            {expanded ? (
              <BrandLogoHorizontal className="h-8" />
            ) : (
              <BrandLogo className="h-7 w-7 rounded-lg" />
            )}
          </button>
          {/* Explicit collapse affordance — the logo also toggles, but a
              visible button makes collapsing discoverable. */}
          {expanded && (
            <button
              type="button"
              onClick={() => setExpanded(false)}
              aria-label={t("nav.collapseSidebar", { defaultValue: "Collapse sidebar" })}
              className="flex h-8 w-8 items-center justify-center rounded-lg text-muted-foreground hover:bg-muted-50 hover:text-foreground transition-colors no-press-scale"
            >
              <PanelLeftClose className="h-4 w-4" />
            </button>
          )}
        </div>

        {/* Nav — icon rail, tooltips carry the names */}
        <nav className="flex flex-col items-center gap-1 pt-2 pb-2">
          {navItems.map(renderItem)}
        </nav>

        <div className="flex-1" />

        {/* Footer — instance / onboarding / settings / user avatar.
            Theme, language and alerts live top-right (GlobalControls).
            Uniform row height + gap in BOTH states so the rows stay at the
            same Y when the rail expands — only labels appear/disappear. */}
        <div className={cn(
          "flex w-full flex-col items-center gap-1 pb-[calc(0.75rem+env(safe-area-inset-bottom,0px))]"
        )}>
          {/* Instance selector — name pill when expanded, icon when collapsed */}
          {expanded ? (
            <div className="w-full px-2">
              <InstanceSelector onManageInstances={() => setInstanceManagerOpen(true)} />
            </div>
          ) : (
            <Tooltip>
              <TooltipTrigger asChild>
                <InstanceSelector compact onManageInstances={() => setInstanceManagerOpen(true)} />
              </TooltipTrigger>
              <TooltipContent side="right" className="text-xs px-2 py-1">
                {t("instances.title", { defaultValue: "Instances" })}
              </TooltipContent>
            </Tooltip>
          )}
          {footerRow(
            t("onboarding.title"),
            <Rocket className="h-5 w-5" />,
            () => setOnboardingOpen(true),
            onboardingIncomplete,
            "onboarding"
          )}
          {footerRow(
            t("nav.settings"),
            <Settings className="h-5 w-5" />,
            () => openSettings(),
            false,
            "settings"
          )}
          {userEntry}
        </div>
      </aside>

      {/* Dialogs owned by the rail entries */}
      <InstanceManagerDialog
        open={instanceManagerOpen}
        onOpenChange={setInstanceManagerOpen}
      />
      <OnboardingDialog
        open={onboardingOpen}
        onOpenChange={setOnboardingOpen}
        status={onboardingStatus}
        onDismiss={dismissOnboarding}
      />
    </TooltipProvider>
  )
}
