/**
 * TopNav - Top navigation bar with icon-only buttons
 * Clean, compact design with module navigation
 * Responsive: hamburger menu on mobile, full icons on desktop
 */

import { useStore } from "@/store"
import { cn } from "@/lib/utils"
import { useTranslation } from "react-i18next"
import { Link, useLocation } from "react-router-dom"
import {
  MessageSquare,
  Cpu,
  Workflow,
  Puzzle,
  Settings,
  Sparkles,
  Wifi,
  WifiOff,
  LogOut,
  Bell,
  Menu,
  X,
  LayoutDashboard,
  BellRing,
  Bot,
  Check,
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
} from "@/components/ui/dropdown-menu"
import {
  Sheet,
  SheetContent,
  SheetTrigger,
  SheetClose,
} from "@/components/ui/sheet"
import { ThemeToggle } from "./ThemeToggle"
import { useState, useEffect } from "react"

type PageType = "dashboard" | "visual-dashboard" | "devices" | "automation" | "agents" | "messages" | "extensions" | "settings"

interface NavItem {
  id: PageType
  path: string
  icon: React.ComponentType<{ className?: string }>
  labelKey: string
}

const navItems: NavItem[] = [
  { id: "dashboard", path: "/chat", labelKey: "nav.dashboard", icon: MessageSquare },
  { id: "agents", path: "/agents", labelKey: "nav.agents", icon: Bot },
  { id: "visual-dashboard", path: "/visual-dashboard", labelKey: "nav.visual-dashboard", icon: LayoutDashboard },
  { id: "devices", path: "/devices", labelKey: "nav.devices", icon: Cpu },
  { id: "automation", path: "/automation", labelKey: "nav.automation", icon: Workflow },
  { id: "messages", path: "/messages", labelKey: "nav.messages", icon: Bell },
  { id: "extensions", path: "/extensions", labelKey: "nav.extensions", icon: Puzzle },
  { id: "settings", path: "/settings", labelKey: "nav.settings", icon: Settings },
]

export function TopNav() {
  const { t, i18n } = useTranslation('common')
  const location = useLocation()
  const user = useStore((state) => state.user)
  const isConnected = useStore((state) => state.wsConnected)
  const logout = useStore((state) => state.logout)
  const alerts = useStore((state) => state.alerts)
  const fetchAlerts = useStore((state) => state.fetchAlerts)
  const acknowledgeAlert = useStore((state) => state.acknowledgeAlert)
  const [mobileMenuOpen, setMobileMenuOpen] = useState(false)
  const [alertDropdownOpen, setAlertDropdownOpen] = useState(false)

  // Fetch alerts on mount and periodically
  useEffect(() => {
    fetchAlerts()
    const interval = setInterval(fetchAlerts, 30000) // Refresh every 30s
    return () => clearInterval(interval)
  }, [fetchAlerts])

  // Count unacknowledged alerts
  const unreadCount = alerts.filter(a => !a.acknowledged && a.status !== 'resolved' && a.status !== 'acknowledged').length

  // Get current path without trailing slash for comparison
  const currentPath = location.pathname.endsWith('/') && location.pathname !== '/'
    ? location.pathname.slice(0, -1)
    : location.pathname

  const getUserInitials = (username: string) => {
    return username.slice(0, 2).toUpperCase()
  }

  const toggleLanguage = () => {
    const newLang = i18n.language === 'zh' ? 'en' : 'zh'
    i18n.changeLanguage(newLang)
  }

  const handleLogout = () => {
    logout()
  }

  const handleNavClick = () => {
    setMobileMenuOpen(false)
  }

  const handleAcknowledgeAlert = async (alertId: string) => {
    await acknowledgeAlert(alertId)
  }

  const getSeverityColor = (severity: string) => {
    switch (severity) {
      case 'critical':
      case 'emergency':
        return 'text-red-600 bg-red-50 border-red-200 dark:text-red-400 dark:bg-red-950/30 dark:border-red-800'
      case 'warning':
        return 'text-amber-600 bg-amber-50 border-amber-200 dark:text-amber-400 dark:bg-amber-950/30 dark:border-amber-800'
      case 'info':
      default:
        return 'text-blue-600 bg-blue-50 border-blue-200 dark:text-blue-400 dark:bg-blue-950/30 dark:border-blue-800'
    }
  }

  return (
    <TooltipProvider delayDuration={500}>
      <nav className="fixed top-0 left-0 right-0 min-h-16 bg-background/95 backdrop-blur flex items-center px-4 sm:px-6 shadow-sm z-50" style={{paddingTop: 'env(safe-area-inset-top, 0px)'}}>
        {/* Logo - vertically centered in nav */}
        <Link to="/chat" className="flex shrink-0 items-center justify-center mr-4 md:mr-6">
          <BrandLogoWithName />
        </Link>

        {/* Desktop Navigation Icons - hidden on mobile */}
        <div className="hidden md:flex items-center gap-1.5">
          {navItems.map((item) => {
            const Icon = item.icon
            // Check active with prefix match for nested routes (e.g., /devices/types matches /devices)
            const isActive = currentPath === item.path ||
              (item.path === '/chat' && currentPath === '/') ||
              currentPath.startsWith(`${item.path}/`)

            return (
              <Tooltip key={item.id}>
                <TooltipTrigger asChild>
                  <Link to={item.path}>
                    <Button
                      variant="ghost"
                      size="icon"
                      className={cn(
                        "w-11 h-11 rounded-lg transition-all",
                        isActive
                          ? "bg-foreground text-background hover:bg-foreground hover:text-background"
                          : "text-muted-foreground hover:text-foreground hover:bg-muted/50"
                      )}
                    >
                      <Icon className="h-5 w-5" />
                    </Button>
                  </Link>
                </TooltipTrigger>
                <TooltipContent side="bottom" className="text-xs px-2 py-1">
                  {t(item.labelKey)}
                </TooltipContent>
              </Tooltip>
            )
          })}
        </div>

        {/* Mobile Hamburger Menu - Left Side Drawer */}
        <div className="md:hidden">
          <Sheet open={mobileMenuOpen} onOpenChange={setMobileMenuOpen}>
            <SheetTrigger asChild>
              <Button
                variant="ghost"
                size="icon"
                className="w-10 h-10 rounded-lg"
                aria-label="Open navigation menu"
              >
                <Menu className="h-5 w-5" />
              </Button>
            </SheetTrigger>
            <SheetContent side="left" className="w-64 p-0">
              <div className="flex flex-col h-full">
                {/* Header */}
                <div className="px-6 py-4 border-b">
                  <Link to="/chat" onClick={handleNavClick}>
                    <BrandLogoWithName />
                  </Link>
                </div>

                {/* Navigation Items */}
                <nav className="flex-1 overflow-y-auto py-4 px-2">
                  {navItems.map((item) => {
                    const Icon = item.icon
                    // Check active with prefix match for nested routes (e.g., /devices/types matches /devices)
                    const isActive = currentPath === item.path ||
                      (item.path === '/chat' && currentPath === '/') ||
                      currentPath.startsWith(`${item.path}/`)

                    return (
                      <SheetClose asChild key={item.id}>
                        <Link
                          to={item.path}
                          className={cn(
                            "flex items-center gap-3 px-4 py-3 rounded-lg text-sm font-medium transition-all active:scale-95 active:bg-muted",
                            isActive
                              ? "bg-foreground text-background"
                              : "text-muted-foreground hover:text-foreground hover:bg-muted/50"
                          )}
                        >
                          <Icon className="h-5 w-5" />
                          <span>{t(item.labelKey)}</span>
                        </Link>
                      </SheetClose>
                    )
                  })}
                </nav>

                {/* Footer - User info */}
                {user && (
                  <div className="px-4 py-4 border-t">
                    <div className="flex items-center gap-3 px-2">
                      <Avatar className="h-9 w-9 rounded-lg">
                        <AvatarFallback className="bg-muted text-muted-foreground text-xs font-medium">
                          {getUserInitials(user.username)}
                        </AvatarFallback>
                      </Avatar>
                      <div className="flex-1 min-w-0">
                        <p className="text-sm font-medium truncate">{user.username}</p>
                        <p className="text-xs text-muted-foreground truncate">{t('user')}</p>
                      </div>
                    </div>
                    <Button
                      variant="ghost"
                      onClick={() => {
                        handleLogout()
                        setMobileMenuOpen(false)
                      }}
                      className="w-full mt-3 justify-start gap-2 text-destructive hover:text-destructive hover:bg-destructive/10"
                    >
                      <LogOut className="h-4 w-4" />
                      {t('logout')}
                    </Button>
                  </div>
                )}

                {/* Settings - Language & Theme */}
                <div className="px-4 py-3 border-t">
                  <Button
                    variant="ghost"
                    onClick={() => {
                      toggleLanguage()
                    }}
                    className="w-full justify-start gap-2"
                  >
                    <span className="text-sm font-medium">{i18n.language === 'zh' ? 'English' : '中文'}</span>
                    <span className="text-xs text-muted-foreground ml-auto">{i18n.language === 'zh' ? 'EN' : '中'}</span>
                  </Button>
                </div>
              </div>
            </SheetContent>
          </Sheet>
        </div>

        {/* Spacer - limit on mobile to keep logo and right icons closer */}
        <div className="flex-1 max-md:max-w-8" />

        {/* Right side: Status + Language + Theme + User - ml-auto pushes to right when spacer is limited on mobile */}
        <div className="ml-auto flex shrink-0 items-center gap-1.5 sm:gap-2.5">
          {/* Connection status - icon only on mobile */}
          <Tooltip>
            <TooltipTrigger asChild>
              <div
                className={cn(
                  "flex items-center gap-2 px-2.5 py-1.5 rounded-lg text-xs font-medium transition-colors",
                  isConnected
                    ? "bg-green-500/10 text-green-600 dark:text-green-400 border border-green-500/20"
                    : "text-destructive bg-destructive/10"
                )}
              >
                {isConnected ? (
                  <Wifi className="h-3.5 w-3.5" />
                ) : (
                  <WifiOff className="h-3.5 w-3.5" />
                )}
                <span className="hidden sm:inline">
                  {isConnected ? t('connection.connected') : t('connection.disconnected')}
                </span>
              </div>
            </TooltipTrigger>
            <TooltipContent side="bottom" className="text-xs">
              {isConnected ? t('connection.wsConnected') : t('connection.wsDisconnected')}
            </TooltipContent>
          </Tooltip>

          {/* Language toggle - hidden on mobile */}
          <Tooltip>
            <TooltipTrigger asChild>
              <Button
                variant="ghost"
                size="sm"
                onClick={toggleLanguage}
                className="hidden sm:flex h-10 w-10 rounded-lg text-muted-foreground hover:text-foreground text-xs font-medium"
              >
                {i18n.language === 'zh' ? '中' : 'EN'}
              </Button>
            </TooltipTrigger>
            <TooltipContent side="bottom" className="text-xs sm:block">
              {i18n.language === 'zh' ? t('language.switchToEnglish') : t('language.switchToChinese')}
            </TooltipContent>
          </Tooltip>

          {/* Theme toggle */}
          <ThemeToggle />

          {/* Alerts notification */}
          <DropdownMenu open={alertDropdownOpen} onOpenChange={setAlertDropdownOpen}>
            <Tooltip>
              <TooltipTrigger asChild>
                <DropdownMenuTrigger asChild>
                  <Button
                    variant="ghost"
                    size="icon"
                    className="w-10 h-10 rounded-lg relative"
                  >
                    <BellRing className="h-4 w-4" />
                    {unreadCount > 0 && (
                      <Badge
                        variant="destructive"
                        className="absolute -top-0.5 -right-0.5 h-5 min-w-5 px-1 flex items-center justify-center text-xs"
                      >
                        {unreadCount > 9 ? '9+' : unreadCount}
                      </Badge>
                    )}
                  </Button>
                </DropdownMenuTrigger>
              </TooltipTrigger>
              <TooltipContent side="bottom" className="text-xs">
                {t('alerts.title')}
              </TooltipContent>
            </Tooltip>
            <DropdownMenuContent align="end" className="w-80 max-h-80 overflow-y-auto">
              <div className="px-3 py-2 border-b">
                <div className="flex items-center justify-between">
                  <span className="font-semibold text-sm">{t('alerts.title')}</span>
                  {unreadCount > 0 && (
                    <Badge variant="outline" className="text-xs">
                      {unreadCount} {t('alerts.unread')}
                    </Badge>
                  )}
                </div>
              </div>
              {alerts.length === 0 ? (
                <div className="py-8 text-center text-muted-foreground text-sm">
                  <Bell className="h-8 w-8 mx-auto mb-2 opacity-50" />
                  {t('alerts.noAlerts')}
                </div>
              ) : (
                <div className="py-1">
                  {alerts.slice(0, 10).map((alert) => {
                    const isUnread = !alert.acknowledged && alert.status !== 'resolved' && alert.status !== 'acknowledged'
                    return (
                      <div
                        key={alert.id}
                        className={cn(
                          "px-3 py-1.5 border-b last:border-b-0 hover:bg-muted/50 transition-colors",
                          isUnread && "bg-muted/30"
                        )}
                      >
                        <div className="flex items-center gap-2">
                          {/* Severity badge - fixed size */}
                          <Badge
                            variant="outline"
                            className={cn(
                              "text-[10px] px-1 py-0 shrink-0 h-5 flex items-center",
                              getSeverityColor(alert.severity)
                            )}
                          >
                            {alert.severity}
                          </Badge>
                          {/* Unread indicator */}
                          {isUnread && (
                            <div className="w-1.5 h-1.5 rounded-full bg-blue-500 shrink-0" />
                          )}
                          {/* Title - single line, truncate */}
                          <p className="text-xs font-medium truncate flex-1">{alert.title}</p>
                          {/* Acknowledge button - compact icon */}
                          {isUnread && (
                            <Button
                              variant="ghost"
                              size="icon"
                              className="h-5 w-5 shrink-0"
                              onClick={() => handleAcknowledgeAlert(alert.id)}
                              title={t('alerts.acknowledge')}
                            >
                              <Check className="h-3 w-3" />
                            </Button>
                          )}
                        </div>
                        {/* Message - fixed height with single line clamp */}
                        <p className="text-[11px] text-muted-foreground truncate ml-7 mt-0.5" title={alert.message}>
                          {alert.message}
                        </p>
                      </div>
                    )
                  })}
                  {alerts.length > 10 && (
                    <div className="px-3 py-1.5 text-center text-xs text-muted-foreground">
                      {t('alerts.moreAlerts', { count: alerts.length - 10 })}
                    </div>
                  )}
                </div>
              )}
            </DropdownMenuContent>
          </DropdownMenu>

          {/* User avatar with dropdown - desktop only */}
          {user && (
            <div className="hidden md:block">
              <DropdownMenu>
                <DropdownMenuTrigger asChild>
                  <Avatar className="h-10 w-10 cursor-pointer rounded-lg">
                    <AvatarFallback className="bg-muted text-muted-foreground text-xs font-medium">
                      {getUserInitials(user.username)}
                    </AvatarFallback>
                  </Avatar>
                </DropdownMenuTrigger>
                <DropdownMenuContent align="end" className="w-48">
                  <div className="px-3 py-2">
                    <p className="text-sm font-medium">{user.username}</p>
                  </div>
                  <DropdownMenuSeparator />
                  <DropdownMenuItem onClick={handleLogout} className="text-destructive focus:text-destructive">
                    <LogOut className="h-4 w-4 mr-2" />
                    {t('logout')}
                  </DropdownMenuItem>
                </DropdownMenuContent>
              </DropdownMenu>
            </div>
          )}
        </div>
      </nav>
    </TooltipProvider>
  )
}
