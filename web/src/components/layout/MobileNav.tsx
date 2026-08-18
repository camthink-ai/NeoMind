/**
 * MobileNav - Hamburger navigation drawer (mobile-only)
 *
 * Style mirrors SessionSidebar: w-72 drawer with a header (title + X close),
 * ScrollArea list, p-2 rounded-lg items with bg-muted active state. The
 * hamburger trigger lives in each page's MobilePageHeader; drawer open-state
 * is shared via the `useMobileNav` store.
 *
 * Sections (top → bottom):
 *   1. User card — avatar + name + role (tap → preferences)
 *   2. Primary nav
 *   3. System nav
 *   4. Account & System entries — instance, onboarding, theme, language
 *   5. Logout
 *
 * Desktop layout is unchanged.
 */

import { useEffect, useMemo, useState, type ReactNode } from "react"
import { useNavigate, useLocation } from "react-router-dom"
import { useTranslation } from "react-i18next"
import {
  Bell,
  MessageSquare,
  Bot,
  Cpu,
  LayoutDashboard,
  Workflow,
  Database,
  Puzzle,
  Settings,
  X,
  LogOut,
  Rocket,
  Sun,
  Moon,
  Monitor,
  Globe,
  type LucideIcon,
} from "lucide-react"
import { useStore } from "@/store"
import { useMobileNav } from "@/store/mobileNav"
import { cn } from "@/lib/utils"
import { useIsMobile } from "@/hooks/useMobile"
import { useTheme } from "@/components/ui/theme"
import { Button } from "@/components/ui/button"
import { Badge } from "@/components/ui/badge"
import { Avatar, AvatarFallback } from "@/components/ui/avatar"
import {
  Sheet,
  SheetContent,
  SheetTitle,
} from "@/components/ui/sheet"
import { OnboardingDialog } from "@/components/onboarding/OnboardingDialog"
import { useOnboarding } from "@/hooks/useOnboarding"

interface NavEntry {
  id: string
  path: string
  icon: LucideIcon
  labelKey: string
}

const PRIMARY: NavEntry[] = [
  { id: "chat", path: "/chat", icon: MessageSquare, labelKey: "nav.dashboard" },
  { id: "agents", path: "/agents", icon: Bot, labelKey: "nav.agents" },
  { id: "devices", path: "/devices", icon: Cpu, labelKey: "nav.devices" },
  { id: "dashboard", path: "/visual-dashboard", icon: LayoutDashboard, labelKey: "nav.visual-dashboard" },
]

const SYSTEM_ENTRIES: NavEntry[] = [
  { id: "automation", path: "/automation", icon: Workflow, labelKey: "nav.automation" },
  { id: "data", path: "/data", icon: Database, labelKey: "nav.data" },
  { id: "messages", path: "/messages", icon: Bell, labelKey: "nav.messages" },
  { id: "extensions", path: "/extensions", icon: Puzzle, labelKey: "nav.extensions" },
  { id: "settings", path: "/settings", icon: Settings, labelKey: "nav.settings" },
]

function getUserInitials(username: string) {
  return username.slice(0, 2).toUpperCase()
}

export function MobileNav() {
  const isMobile = useIsMobile()
  const { t, i18n } = useTranslation("common")
  const navigate = useNavigate()
  const openSettings = useStore((state) => state.openSettings)
  const location = useLocation()
  const user = useStore((s) => s.user)
  const logout = useStore((s) => s.logout)
  const alerts = useStore((s) => s.alerts)
  const { open, setOpen } = useMobileNav()

  // When the nav drawer opens, blur any focused input so the soft keyboard
  // starts dismissing immediately. iOS PWA's `interactive-widget=resizes-
  // content` only resizes the layout viewport once the keyboard is fully
  // gone (300ms animation); if the user taps a menu item while the keyboard
  // is still mid-dismiss, the new page renders in the shrunk viewport and
  // ends up with content under the notch. Blurring on drawer-open gives
  // the keyboard the full drawer-appearance animation to dismiss before
  // any navigation can happen.
  useEffect(() => {
    if (!open) return
    if (document.activeElement instanceof HTMLElement) {
      document.activeElement.blur()
    }
  }, [open])

  const [onboardingOpen, setOnboardingOpen] = useState(false)
  const { status: onboardingStatus, dismiss: dismissOnboarding, fetchStatus: fetchOnboardingStatus } = useOnboarding()

  const { theme, setTheme } = useTheme()

  const unreadCount = useMemo(
    () =>
      alerts.filter((a) => !a.acknowledged && a.status !== "resolved" && a.status !== "acknowledged").length,
    [alerts],
  )

  if (!isMobile) return null

  const current = location.pathname.endsWith("/") && location.pathname !== "/" ? location.pathname.slice(0, -1) : location.pathname
  const isActive = (path: string) => {
    if (path === "/chat") return current === "/" || current === "/chat" || current.startsWith("/chat/")
    return current === path || current.startsWith(`${path}/`)
  }

  const go = (path: string) => {
    setOpen(false)
    // NOTE: previously wrapped in startTransition(), which marked the route
    // change as low-priority. Combined with the immediate setOpen(false),
    // the drawer would close but the page could lag behind by a tick —
    // users reported "taps don't navigate" because the visual close didn't
    // line up with the (deferred) route change, prompting a second tap that
    // interrupted the first. Navigation from a menu item is an explicit
    // user action and should fire at full priority.
    navigate(path)
  }

  const onboardingIncomplete =
    !!onboardingStatus &&
    !onboardingStatus.dismissed &&
    (!onboardingStatus.steps.llm.completed || !onboardingStatus.steps.device.completed)

  const renderItem = (entry: NavEntry) => {
    const Icon = entry.icon
    const active = isActive(entry.path)
    return (
      <button
        key={entry.id}
        type="button"
        onClick={() => {
          if (entry.id === "settings") {
            setOpen(false)
            openSettings()
          } else {
            go(entry.path)
          }
        }}
        // p-3 (12px) + 20px content = ~44px, Apple HIG minimum touch target.
        // Previously p-2 → 36px which missed taps when the finger drifted even
        // a few pixels, especially near the rounded-lg corners.
        className={cn(
          "group relative flex w-full items-center gap-3 rounded-lg p-3 text-left transition-colors",
          active ? "" : "hover:bg-muted-50 active:bg-muted",
        )}
      >
        <Icon
          className={cn("h-5 w-5 shrink-0", active ? "text-foreground" : "text-muted-foreground")}
        />
        <span
          className={cn(
            "flex-1 truncate text-sm",
            active ? "font-medium text-foreground" : "text-muted-foreground",
          )}
        >
          {t(entry.labelKey)}
        </span>
        {entry.id === "messages" && unreadCount > 0 && (
          <Badge variant="destructive" className="h-5 min-w-5 justify-center px-1 text-xs">
            {unreadCount > 99 ? "99+" : unreadCount}
          </Badge>
        )}
      </button>
    )
  }

  // Compact icon-button row used for the Account section's quick toggles.
  const renderIconButton = (
    label: string,
    icon: ReactNode,
    onClick: () => void,
    opts: { active?: boolean; badge?: boolean } = {},
  ) => (
    <Button
      key={label}
      variant="ghost"
      size="icon"
      onClick={onClick}
      className={cn(
        "h-9 w-9 shrink-0 rounded-lg",
        opts.active ? "bg-muted text-foreground" : "text-muted-foreground hover:text-foreground hover:bg-muted-50",
      )}
      aria-label={label}
      title={label}
    >
      {icon}
      {opts.badge && <span className="absolute right-1.5 top-1.5 h-1.5 w-1.5 rounded-full bg-primary" />}
    </Button>
  )

  return (
    <Sheet open={open} onOpenChange={setOpen}>
      <SheetContent
        side="left"
        className="mobile-nav-sheet flex w-72 flex-col gap-0 p-0"
        // Use --chrome (opaque in both light & dark) instead of the default
        // bg-background (dark mode has /97% alpha → slightly see-through,
        // which lets the SheetOverlay tint bleed through and makes the drawer
        // read as a darker/different layer than the page). --chrome matches
        // MobilePageHeader so the drawer visually belongs to the same chrome
        // layer as the top bar — no color split on mobile.
        style={{ backgroundColor: "var(--chrome)" }}
      >
        <SheetTitle className="sr-only">{t("system.menu")}</SheetTitle>

        {/* Header */}
        <div className="safe-top shrink-0">
          <div className="flex h-12 items-center justify-between border-b border-border px-3">
            <h2 className="text-sm font-semibold text-foreground">{t("system.menu")}</h2>
            <Button
              variant="ghost"
              size="icon"
              onClick={() => setOpen(false)}
              className="h-8 w-8 rounded-lg"
              aria-label={t("system.back")}
            >
              <X className="h-4 w-4" />
            </Button>
          </div>
        </div>

        {/* Nav list — native overflow-y-auto instead of Radix ScrollArea.
            Radix ScrollArea's pointer-event handling on iOS swallows tap
            events that land during momentum-scroll settle, which made menu
            items feel unresponsive ("tap doesn't navigate"). Native scroll
            has no such interception. */}
        <div className="flex-1 min-h-0 overflow-y-auto overflow-x-hidden">
          <div className="space-y-1 px-2 pb-2 pt-1">
            {PRIMARY.map(renderItem)}

            {SYSTEM_ENTRIES.map(renderItem)}

            {/* Quick toggles + guides */}

            {/* Theme quick toggle row — label on left, three icon buttons on right */}
            <div className="flex items-center gap-1 rounded-lg p-2">
              <Sun className="h-4 w-4 shrink-0 text-muted-foreground" />
              <span className="flex-1 truncate text-sm text-muted-foreground">
                {t("theme.label", "Theme")}
              </span>
              <div className="flex shrink-0 items-center gap-0.5">
                {renderIconButton(
                  t("theme.light", "Light"),
                  <Sun className="h-4 w-4" />,
                  () => setTheme("light"),
                  { active: theme === "light" },
                )}
                {renderIconButton(
                  t("theme.dark", "Dark"),
                  <Moon className="h-4 w-4" />,
                  () => setTheme("dark"),
                  { active: theme === "dark" },
                )}
                {renderIconButton(
                  t("theme.system", "System"),
                  <Monitor className="h-4 w-4" />,
                  () => setTheme("system"),
                  { active: theme === "system" },
                )}
              </div>
            </div>

            {/* Language toggle row — same shape as other nav rows */}
            <button
              type="button"
              onClick={() => {
                const next = i18n.language === "zh" ? "en" : "zh"
                i18n.changeLanguage(next)
              }}
              className="group relative flex w-full items-center gap-2 rounded-lg p-2 text-left transition-all hover:bg-muted-50"
            >
              <Globe className="h-4 w-4 shrink-0 text-muted-foreground" />
              <span className="flex-1 truncate text-sm text-muted-foreground">
                {t("system.language")}
              </span>
              <span className="shrink-0 text-xs font-medium text-muted-foreground">
                {i18n.language === "zh" ? "中文" : "English"}
              </span>
            </button>

            {/* Onboarding guide */}
            <button
              type="button"
              onClick={() => {
                setOpen(false)
                setOnboardingOpen(true)
              }}
              className={cn(
                "group relative flex w-full items-center gap-2 rounded-lg p-2 text-left transition-all hover:bg-muted-50",
              )}
            >
              <Rocket className="h-4 w-4 shrink-0 text-muted-foreground" />
              <span className="flex-1 truncate text-sm text-muted-foreground">
                {t("onboarding.title")}
              </span>
              {onboardingIncomplete && (
                <span className="h-1.5 w-1.5 shrink-0 rounded-full bg-primary" />
              )}
            </button>
          </div>
        </div>

        {/* User card + Logout — anchored at the bottom of the drawer,
            outside the scroll container so they never ride up next to the
            menu items on short lists. The drawer is a flex-col, so this
            shrink-0 footer always sits above safe-bottom. */}
        <div className="shrink-0 border-t border-border px-2 pt-2">
          {user && (
            <div className="mb-1 flex w-full items-center gap-2 rounded-lg p-2">
              <button
                type="button"
                onClick={() => {
                  setOpen(false)
                  openSettings("preferences")
                }}
                className="flex min-w-0 flex-1 items-center gap-3 rounded-lg p-1 text-left transition-all hover:bg-muted-50"
              >
                <Avatar className="h-9 w-9 shrink-0 rounded-lg">
                  <AvatarFallback className="bg-muted text-xs font-medium text-foreground">
                    {getUserInitials(user.username)}
                  </AvatarFallback>
                </Avatar>
                <div className="min-w-0 flex-1">
                  <p className="truncate text-sm font-medium text-foreground">{user.username}</p>
                  {user.role && (
                    <p className="truncate text-xs text-muted-foreground">{user.role}</p>
                  )}
                </div>
              </button>
              {/* Logout — to the right of the username */}
              <Button
                variant="ghost"
                size="icon"
                onClick={() => {
                  setOpen(false)
                  logout()
                }}
                className="h-9 w-9 shrink-0 rounded-lg text-error hover:text-error hover:bg-error-light"
                aria-label={t("logout")}
              >
                <LogOut className="h-4 w-4" />
              </Button>
            </div>
          )}
        </div>

        {/* Safe-bottom spacer */}
        <div className="safe-bottom shrink-0" />
      </SheetContent>

      {/* Dialogs opened from inside the drawer. Rendered outside SheetContent
          so their portals aren't clipped by the drawer's stacking context. */}
      <OnboardingDialog
        open={onboardingOpen}
        onOpenChange={(o) => {
          setOnboardingOpen(o)
          if (!o) fetchOnboardingStatus()
        }}
        status={onboardingStatus}
        onDismiss={dismissOnboarding}
      />
    </Sheet>
  )
}
