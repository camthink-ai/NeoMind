/**
 * TopNav - Top navigation bar
 * Desktop: icon buttons with tooltips
 * Mobile: scrollable text tab bar with underline indicator + swipe gestures
 */

import { useStore } from "@/store"
import { cn } from "@/lib/utils"
import { isTauriEnv } from "@/lib/api"
import { textNano, textMini } from "@/design-system/tokens/typography"
import { useTranslation } from "react-i18next"
import { Link, useLocation, useNavigate } from "react-router-dom"
import {
  MessageSquare,
  Cpu,
  Workflow,
  Puzzle,
  Settings,
  LogOut,
  Bell,
  LayoutDashboard,
  BellRing,
  Bot,
  Check,
  CheckCheck,
  AlertTriangle,
  Database,
  Rocket,
  Info,
  Sun,
  Languages,
} from "lucide-react"
import { Button } from "@/components/ui/button"
import { Avatar, AvatarFallback } from "@/components/ui/avatar"
import { BrandLogoWithName } from "@/components/shared/BrandName"
import { Badge } from "@/components/ui/badge"
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
import { useTheme } from "@/components/ui/theme"
import { InstanceSelector } from "./InstanceSelector"
import { SystemHealthButton } from "./SystemHealthButton"
import { InstanceManagerDialog } from "@/components/instances/InstanceManagerDialog"
import { OnboardingDialog } from "@/components/onboarding/OnboardingDialog"
import { useOnboarding } from "@/hooks/useOnboarding"
import { useState, useEffect, useRef, useCallback, useMemo, forwardRef, startTransition } from "react"
import { setTopNavHeight } from "@/hooks/useVisualViewport"
import { useIsMobile } from "@/hooks/useMobile"

type PageType = "dashboard" | "visual-dashboard" | "data" | "devices" | "automation" | "agents" | "messages" | "extensions" | "settings"

interface NavItem {
  id: PageType
  path: string
  icon: React.ComponentType<{ className?: string }>
  labelKey: string
  /** Shorter label for mobile tab bar (falls back to labelKey if not set) */
  mobileLabelKey?: string
}

const navItems: NavItem[] = [
  { id: "dashboard", path: "/chat", labelKey: "nav.dashboard", mobileLabelKey: "navShort.dashboard", icon: MessageSquare },
  { id: "agents", path: "/agents", labelKey: "nav.agents", mobileLabelKey: "navShort.agents", icon: Bot },
  { id: "visual-dashboard", path: "/visual-dashboard", labelKey: "nav.visual-dashboard", mobileLabelKey: "navShort.visual-dashboard", icon: LayoutDashboard },
  { id: "devices", path: "/devices", labelKey: "nav.devices", mobileLabelKey: "navShort.devices", icon: Cpu },
  { id: "automation", path: "/automation", labelKey: "nav.automation", mobileLabelKey: "navShort.automation", icon: Workflow },
  { id: "data", path: "/data", labelKey: "nav.data", mobileLabelKey: "navShort.data", icon: Database },
  { id: "messages", path: "/messages", labelKey: "nav.messages", mobileLabelKey: "navShort.messages", icon: Bell },
  { id: "extensions", path: "/extensions", labelKey: "nav.extensions", mobileLabelKey: "navShort.extensions", icon: Puzzle },
]

export const TopNav = forwardRef<HTMLDivElement>((props, ref) => {
  const innerRef = useRef<HTMLDivElement>(null)
  const tabBarRef = useRef<HTMLDivElement>(null)
  const [indicatorStyle, setIndicatorStyle] = useState({ left: 0, width: 0 })

  const isMobile = useIsMobile()
  // macOS Tauri overlay title bar: reserve a top strip for the traffic lights
  // (their own row, not pushing the logo right); the top bar doubles as the
  // drag region.
  const isMacTauri = isTauriEnv() && /Mac/i.test(navigator.platform || navigator.userAgent)
  const navigate = useNavigate()

  // Set the nav height in CSS variable after mount and on resize
  useEffect(() => {
    const updateNavHeight = () => {
      if (innerRef.current) {
        const height = innerRef.current.getBoundingClientRect().height
        setTopNavHeight(height)
      }
    }

    updateNavHeight()
    window.addEventListener('resize', updateNavHeight)
    return () => window.removeEventListener('resize', updateNavHeight)
  }, [])

  const { t, i18n } = useTranslation('common')
  const location = useLocation()
  const user = useStore((state) => state.user)
  const logout = useStore((state) => state.logout)
  const openSettings = useStore((state) => state.openSettings)
  const alerts = useStore((state) => state.alerts)
  const fetchAlerts = useStore((state) => state.fetchAlerts)
  const acknowledgeAlert = useStore((state) => state.acknowledgeAlert)
  const [alertDropdownOpen, setAlertDropdownOpen] = useState(false)
  const [instanceManagerOpen, setInstanceManagerOpen] = useState(false)
  const [onboardingOpen, setOnboardingOpen] = useState(false)

  // Onboarding status for the Rocket button badge
  const { status: onboardingStatus, dismiss: dismissOnboarding, fetchStatus: fetchOnboardingStatus } = useOnboarding()

  // Fetch onboarding status on mount
  useEffect(() => {
    fetchOnboardingStatus()
  }, [fetchOnboardingStatus])

  // Fetch alerts on mount and periodically (60s, reduced from 30s)
  useEffect(() => {
    fetchAlerts()
    const interval = setInterval(fetchAlerts, 60000)
    return () => clearInterval(interval)
  }, [fetchAlerts])

  // Count unacknowledged alerts - memoized
  const unreadCount = useMemo(
    () => alerts.filter(a => !a.acknowledged && a.status !== 'resolved' && a.status !== 'acknowledged').length,
    [alerts]
  )

  // Get current path without trailing slash for comparison
  const currentPath = location.pathname.endsWith('/') && location.pathname !== '/'
    ? location.pathname.slice(0, -1)
    : location.pathname

  const getUserInitials = (username: string) => {
    return username.slice(0, 2).toUpperCase()
  }

  const { theme, setTheme } = useTheme()

  const handleLogout = () => {
    logout()
  }

  const handleAcknowledgeAlert = async (alertId: string) => {
    await acknowledgeAlert(alertId)
  }

  // Severity config: icon + badge classes + left border accent
  const getSeverityConfig = (severity: string) => {
    switch (severity) {
      case 'critical':
      case 'emergency':
        return {
          icon: AlertTriangle,
          dot: 'bg-error',
          badge: 'text-error bg-error-light',
          bar: 'bg-error',
        }
      case 'warning':
        return {
          icon: AlertTriangle,
          dot: 'bg-warning',
          badge: 'text-warning bg-warning-light',
          bar: 'bg-warning',
        }
      case 'info':
      default:
        return {
          icon: Info,
          dot: 'bg-info',
          badge: 'text-info bg-info-light',
          bar: 'bg-info',
        }
    }
  }

  // Check if a nav item is currently active
  const isItemActive = useCallback((item: NavItem) => {
    return currentPath === item.path ||
      (item.path === '/chat' && currentPath === '/') ||
      currentPath.startsWith(`${item.path}/`)
  }, [currentPath])

  // Update underline indicator position and scroll active tab into view
  useEffect(() => {
    if (!isMobile || !tabBarRef.current) return

    const activeTab = tabBarRef.current.querySelector('[data-active="true"]') as HTMLElement | null
    if (activeTab) {
      setIndicatorStyle({
        left: activeTab.offsetLeft,
        width: activeTab.offsetWidth,
      })
      activeTab.scrollIntoView({ behavior: 'smooth', inline: 'center', block: 'nearest' })
    }
  }, [currentPath, isMobile])

  return (
    <TooltipProvider delayDuration={500}>
      <nav
        ref={innerRef}
        className="fixed top-0 left-0 right-0 z-20 bg-[var(--chrome)] border-b border-border flex flex-col"
        style={{ paddingTop: isMacTauri ? "calc(env(safe-area-inset-top, 0px) + 28px)" : "env(safe-area-inset-top, 0px)" }}
      >
        {/* Main bar */}
        <div
          className="flex items-center px-4 sm:px-6 h-14"
          data-tauri-drag-region={isTauriEnv() || undefined}
        >
          {/* Logo */}
          <Link to="/chat" className="flex shrink-0 items-center justify-center mr-4 md:mr-6">
            <BrandLogoWithName />
          </Link>

          {/* Desktop Navigation Icons */}
          <div className="hidden md:flex items-center gap-1.5">
            {navItems.map((item) => {
              const Icon = item.icon
              const isActive = isItemActive(item)

              return (
                <Tooltip key={item.id}>
                  <TooltipTrigger asChild>
                    <Button
                      variant="ghost"
                      size="icon"
                      className={cn(
                        "w-10 h-10 rounded-lg transition-all no-press-scale",
                        isActive
                          ? "text-brand hover:text-brand hover:bg-transparent"
                          : "text-muted-foreground hover:text-foreground hover:bg-muted-50"
                      )}
                      onClick={() => startTransition(() => navigate(item.path))}
                    >
                      <Icon className={cn("h-5 w-5", isActive && "dark:brand-icon-stroke")} />
                    </Button>
                  </TooltipTrigger>
                  <TooltipContent side="bottom" className="text-xs px-2 py-1">
                    {t(item.labelKey)}
                  </TooltipContent>
                </Tooltip>
              )
            })}
          </div>

          {/* Spacer */}
          <div className="flex-1 max-md:max-w-4" />

          {/* Right side: Instance + Health + Guide + Alerts + Preferences + User */}
          <div className="ml-auto flex shrink-0 items-center gap-1.5 sm:gap-2.5">
            {/* Instance selector (identity anchor) */}
            <InstanceSelector onManageInstances={() => setInstanceManagerOpen(true)} />

            {/* System health indicator */}
            <SystemHealthButton />

            {/* Onboarding guide */}
            <Tooltip>
              <TooltipTrigger asChild>
                <Button
                  variant="ghost"
                  size="icon"
                  className="w-10 h-10 rounded-lg relative bg-[var(--chrome)]"
                  onClick={() => setOnboardingOpen(true)}
                  aria-label={t('onboarding.title')}
                >
                  <Rocket className="h-4 w-4" />
                  {onboardingStatus && !onboardingStatus.dismissed && (
                    (!onboardingStatus.steps.llm.completed || !onboardingStatus.steps.device.completed)
                  ) && (
                    <span className="absolute top-1.5 right-1.5 w-2 h-2 rounded-full bg-primary" />
                  )}
                </Button>
              </TooltipTrigger>
              <TooltipContent side="bottom" className="text-xs px-2 py-1">
                {t('onboarding.title')}
              </TooltipContent>
            </Tooltip>

            {/* Alerts notification */}
            <DropdownMenu open={alertDropdownOpen} onOpenChange={setAlertDropdownOpen}>
              <Tooltip>
                <TooltipTrigger asChild>
                  <DropdownMenuTrigger asChild>
                    <Button
                      variant="ghost"
                      size="icon"
                      className="w-10 h-10 rounded-lg relative bg-[var(--chrome)]"
                    >
                      <BellRing className="h-4 w-4" />
                      {unreadCount > 0 && (
                        <Badge
                          variant="destructive"
                          className="absolute -top-0.5 -right-0.5 h-5 min-w-5 px-1 flex items-center justify-center text-xs"
                        >
                          {unreadCount > 99 ? '99+' : unreadCount}
                        </Badge>
                      )}
                    </Button>
                  </DropdownMenuTrigger>
                </TooltipTrigger>
                <TooltipContent side="bottom" className="text-xs">
                  {t('alerts.title')}
                </TooltipContent>
              </Tooltip>
              <DropdownMenuContent align="end" className="w-[22rem] max-h-[28rem] overflow-hidden flex flex-col p-0">
                {/* Header — icon + title + unread count + mark-all */}
                <div className="flex items-center justify-between px-4 py-3 border-b shrink-0">
                  <div className="flex items-center gap-2">
                    <BellRing className="h-4 w-4 text-muted-foreground" />
                    <span className="font-semibold text-sm">{t('alerts.title')}</span>
                    {unreadCount > 0 && (
                      <span className="inline-flex items-center justify-center h-5 min-w-5 px-1.5 rounded-full bg-destructive text-destructive-foreground text-[10px] font-semibold tabular-nums">
                        {unreadCount}
                      </span>
                    )}
                  </div>
                  {unreadCount > 0 && (
                    <Button
                      variant="ghost"
                      size="xs"
                      className="gap-1 text-muted-foreground hover:text-foreground"
                      onClick={() => alerts.filter(a => !a.acknowledged).forEach(a => handleAcknowledgeAlert(a.id))}
                    >
                      <CheckCheck className="h-3.5 w-3.5" />
                      <span className="hidden sm:inline">{t('alerts.markAllRead', { defaultValue: 'Mark all read' })}</span>
                    </Button>
                  )}
                </div>

                {/* Body */}
                {alerts.length === 0 ? (
                  <div className="flex flex-col items-center justify-center py-10 text-center">
                    <div className="flex h-12 w-12 items-center justify-center rounded-xl bg-primary-light text-primary mb-3">
                      <Bell className="h-6 w-6" />
                    </div>
                    <p className="text-sm font-medium">{t('alerts.noAlerts')}</p>
                    <p className="text-xs text-muted-foreground mt-1">{t('alerts.noAlertsDesc', { defaultValue: 'You\'re all caught up' })}</p>
                  </div>
                ) : (
                  <div className="flex-1 overflow-y-auto">
                    {alerts.slice(0, 10).map((alert) => {
                      const isUnread = !alert.acknowledged && alert.status !== 'resolved' && alert.status !== 'acknowledged'
                      const sev = getSeverityConfig(alert.severity)
                      const SevIcon = sev.icon
                      return (
                        <div
                          key={alert.id}
                          className={cn(
                            "group flex gap-3 px-4 py-2.5 border-b last:border-b-0 transition-colors",
                            isUnread ? "bg-muted-30" : "bg-transparent",
                            "hover:bg-muted-50",
                          )}
                        >
                          {/* Severity icon */}
                          <div className={cn("flex h-7 w-7 shrink-0 items-center justify-center rounded-lg", sev.badge)}>
                            <SevIcon className="h-3.5 w-3.5" />
                          </div>

                          {/* Content */}
                          <div className="flex-1 min-w-0">
                            <div className="flex items-center gap-1.5">
                              <p className={cn("text-xs truncate flex-1", isUnread ? "font-semibold" : "font-medium")}>{alert.title}</p>
                              {isUnread && (
                                <div className={cn("w-1.5 h-1.5 rounded-full shrink-0", sev.dot)} />
                              )}
                            </div>
                            <p className="text-xs text-muted-foreground truncate mt-0.5" title={alert.message}>
                              {alert.message}
                            </p>
                          </div>

                          {/* Acknowledge button */}
                          {isUnread && (
                            <Button
                              variant="ghost"
                              size="icon-sm"
                              className="h-6 w-6 shrink-0 opacity-0 group-hover:opacity-100 transition-opacity"
                              onClick={() => handleAcknowledgeAlert(alert.id)}
                              title={t('alerts.acknowledge')}
                            >
                              <Check className="h-3.5 w-3.5" />
                            </Button>
                          )}
                        </div>
                      )
                    })}
                    {alerts.length > 10 && (
                      <div className="px-4 py-2.5 text-center text-xs text-muted-foreground border-t">
                        {t('alerts.moreAlerts', { count: alerts.length - 10 })}
                      </div>
                    )}
                  </div>
                )}
              </DropdownMenuContent>
            </DropdownMenu>

            {/* User avatar with dropdown (theme / language / settings live in here) */}
            {user && (
              <DropdownMenu>
                <DropdownMenuTrigger asChild>
                  <Avatar className="h-10 w-10 cursor-pointer rounded-full ring-2 ring-background">
                    <AvatarFallback className="bg-primary text-primary-foreground text-xs font-semibold">
                      {getUserInitials(user.username)}
                    </AvatarFallback>
                  </Avatar>
                </DropdownMenuTrigger>
                <DropdownMenuContent align="end" className="w-56">
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
                  <DropdownMenuItem onClick={handleLogout} className="text-error focus:text-error">
                    <LogOut className="h-4 w-4 mr-2" />
                    {t('logout')}
                  </DropdownMenuItem>
                </DropdownMenuContent>
              </DropdownMenu>
            )}
          </div>
        </div>

        {/* Mobile: Scrollable text tab bar with underline indicator */}
        {isMobile && (
          <div
            className="relative border-b border-glass-border"
          >
            <div
              ref={tabBarRef}
              className="relative flex overflow-x-auto scrollbar-none px-3"
            >
              {navItems.map((item) => {
                const isActive = isItemActive(item)
                return (
                  <button
                    key={item.id}
                    data-active={isActive || undefined}
                    className={cn(
                      "flex-shrink-0 px-4 py-2.5 text-base whitespace-nowrap transition-all select-none",
                      isActive
                        ? "text-brand font-bold"
                        : "text-muted-foreground active:text-foreground"
                    )}
                    onClick={() => startTransition(() => navigate(item.path))}
                  >
                    {t(item.mobileLabelKey || item.labelKey)}
                  </button>
                )
              })}
              {/* Animated underline indicator */}
              <div
                className="absolute bottom-0 h-[3px] bg-brand transition-all duration-250 ease-out rounded-full"
                style={{ left: indicatorStyle.left, width: indicatorStyle.width }}
              />
            </div>
          </div>
        )}
      </nav>

      {/* Instance Manager Dialog */}
      <InstanceManagerDialog
        open={instanceManagerOpen}
        onOpenChange={setInstanceManagerOpen}
      />

      {/* Onboarding Dialog */}
      <OnboardingDialog
        open={onboardingOpen}
        onOpenChange={setOnboardingOpen}
        status={onboardingStatus}
        onDismiss={dismissOnboarding}
      />
    </TooltipProvider>
  )
})

TopNav.displayName = 'TopNav'
