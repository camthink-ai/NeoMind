/**
 * TopBar — slim desktop chrome bar.
 * Navigation lives in the AppSidebar; this bar carries the window drag region
 * (macOS overlay titlebar) and the global actions: instance selector,
 * onboarding, alerts, settings, user menu. In-flow (not fixed) inside the
 * app shell's content column; height exported as --topnav-height.
 */

import { useStore } from "@/store"
import { cn } from "@/lib/utils"
import { isTauriEnv } from "@/lib/api"
import { getCurrentWindow } from "@tauri-apps/api/window"
import { textNano, textMini } from "@/design-system/tokens/typography"
import { useTranslation } from "react-i18next"
import {
  Settings,
  LogOut,
  Bell,
  BellRing,
  Check,
  CheckCheck,
  AlertTriangle,
  Rocket,
  Info,
  Sun,
  Languages,
} from "lucide-react"
import { Button } from "@/components/ui/button"
import { Avatar, AvatarFallback } from "@/components/ui/avatar"
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

import { InstanceManagerDialog } from "@/components/instances/InstanceManagerDialog"
import { OnboardingDialog } from "@/components/onboarding/OnboardingDialog"
import { useOnboarding } from "@/hooks/useOnboarding"
import { useState, useEffect, useRef, useCallback, useMemo, forwardRef } from "react"
import { setTopNavHeight } from "@/hooks/useVisualViewport"

export const TopBar = forwardRef<HTMLDivElement>((props, ref) => {
  const innerRef = useRef<HTMLDivElement>(null)

  // macOS Tauri overlay title bar: reserve a top strip for the traffic lights
  // (their own row, not pushing the logo right); the top bar doubles as the
  // drag region.
  const isMacTauri = isTauriEnv() && /Mac/i.test(navigator.platform || navigator.userAgent)

  // Tauri window drag — explicitly call startDragging() on mousedown instead of
  // relying on data-tauri-drag-region (which is unreliable in Tauri 2 overlay mode).
  // Only fires when clicking non-interactive areas (the drag handler checks target).
  const handleDragMouseDown = useCallback((e: React.MouseEvent) => {
    if (!isTauriEnv()) return
    const target = e.target as HTMLElement
    // Skip if clicking on interactive elements (buttons, links, inputs, etc.)
    if (target.closest("button, a, input, select, textarea, [role='button'], [role='tab']")) return
    getCurrentWindow().startDragging()
  }, [])

  // Expose the macOS title-bar (traffic-light) inset as a CSS var so full-screen
  // overlays (settings, onboarding) reserve the same top space and aren't
  // covered by the traffic lights.
  useEffect(() => {
    document.documentElement.style.setProperty("--titlebar-inset", isMacTauri ? "24px" : "0px")
  }, [isMacTauri])

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

  return (
    <TooltipProvider delayDuration={500}>
      <header
        ref={innerRef}
        className="relative z-20 flex shrink-0 items-center bg-[var(--chrome)] border-b border-border px-4 sm:px-6 h-12"
        style={{ paddingTop: "calc(env(safe-area-inset-top, 0px) + var(--titlebar-inset, 0px))" }}
        onMouseDown={handleDragMouseDown}
      >
        {/* Drag region — the empty left area moves the window (macOS Tauri) */}
        <div className="flex-1 max-w-full" />

        {/* Right side: Instance + Health + Guide + Alerts + Preferences + User */}
        <div className="ml-auto flex shrink-0 items-center gap-1.5 sm:gap-2.5">
          {/* Instance selector (identity anchor) */}
          <InstanceSelector onManageInstances={() => setInstanceManagerOpen(true)} />


            {/* Onboarding guide */}
            <Tooltip>
              <TooltipTrigger asChild>
                <Button
                  variant="ghost"
                  size="icon"
                  className="w-10 h-10 rounded-lg relative chrome-ghost"
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
                      className="w-10 h-10 rounded-lg relative chrome-ghost"
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
                      <span className="inline-flex items-center justify-center h-5 min-w-5 px-1.5 rounded-full bg-destructive text-destructive-foreground text-nano font-semibold tabular-nums">
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

            {/* Settings quick access */}
            <Tooltip>
              <TooltipTrigger asChild>
                <Button
                  variant="ghost"
                  size="icon"
                  className="w-10 h-10 rounded-lg chrome-ghost"
                  onClick={() => openSettings()}
                  aria-label={t('nav.settings')}
                >
                  <Settings className="h-4 w-4" />
                </Button>
              </TooltipTrigger>
              <TooltipContent side="bottom" className="text-xs px-2 py-1">
                {t('nav.settings')}
              </TooltipContent>
            </Tooltip>

            {/* User avatar with dropdown (theme / language / settings live in here) */}
            {user && (
              <DropdownMenu>
                <DropdownMenuTrigger asChild>
                  <Avatar className="h-9 w-9 cursor-pointer rounded-full ring-2 ring-background">
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
      </header>

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

TopBar.displayName = 'TopBar'
