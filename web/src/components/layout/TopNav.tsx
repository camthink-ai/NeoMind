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
} from "@/components/ui/dropdown-menu"
import { ThemeToggle } from "./ThemeToggle"
import { useState, useEffect } from "react"

type PageType = "dashboard" | "visual-dashboard" | "devices" | "automation" | "agents" | "events" | "plugins" | "settings"

interface NavItem {
  id: PageType
  path: string
  icon: React.ComponentType<{ className?: string }>
  labelKey: string
}

const navItems: NavItem[] = [
  { id: "dashboard", path: "/chat", labelKey: "nav.dashboard", icon: MessageSquare },
  { id: "visual-dashboard", path: "/visual-dashboard", labelKey: "nav.visual-dashboard", icon: LayoutDashboard },
  { id: "devices", path: "/devices", labelKey: "nav.devices", icon: Cpu },
  { id: "automation", path: "/automation", labelKey: "nav.automation", icon: Workflow },
  { id: "agents", path: "/agents", labelKey: "nav.agents", icon: Bot },
  { id: "events", path: "/events", labelKey: "nav.events", icon: Bell },
  { id: "plugins", path: "/plugins", labelKey: "nav.plugins", icon: Puzzle },
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
      <nav className="h-14 bg-background/95 backdrop-blur flex items-center px-4 shadow-sm z-50 relative">
        {/* Logo */}
        <Link to="/chat" className="flex items-center gap-2 mr-6">
          <div className="w-8 h-8 rounded-lg bg-foreground flex items-center justify-center">
            <Sparkles className="h-4 w-4 text-background" />
          </div>
          <span className="font-semibold text-foreground hidden sm:block">NeoTalk</span>
        </Link>

        {/* Desktop Navigation Icons - hidden on mobile */}
        <div className="hidden md:flex items-center gap-1">
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
                        "w-10 h-10 rounded-xl transition-all",
                        isActive
                          ? "bg-foreground text-background hover:bg-foreground hover:text-background"
                          : "text-muted-foreground hover:text-foreground hover:bg-muted"
                      )}
                    >
                      <Icon className="h-5 w-5" />
                    </Button>
                  </Link>
                </TooltipTrigger>
                <TooltipContent side="bottom" className="text-xs">
                  {t(item.labelKey)}
                </TooltipContent>
              </Tooltip>
            )
          })}
        </div>

        {/* Mobile Hamburger Menu */}
        <div className="md:hidden">
          <DropdownMenu open={mobileMenuOpen} onOpenChange={setMobileMenuOpen}>
            <DropdownMenuTrigger asChild>
              <Button
                variant="ghost"
                size="icon"
                className="w-9 h-9 rounded-xl"
              >
                {mobileMenuOpen ? (
                  <X className="h-5 w-5" />
                ) : (
                  <Menu className="h-5 w-5" />
                )}
              </Button>
            </DropdownMenuTrigger>
            <DropdownMenuContent align="start" className="w-48">
              {navItems.map((item) => {
                const Icon = item.icon
                // Check active with prefix match for nested routes (e.g., /devices/types matches /devices)
                const isActive = currentPath === item.path ||
                  (item.path === '/chat' && currentPath === '/') ||
                  currentPath.startsWith(`${item.path}/`)

                return (
                  <DropdownMenuItem
                    key={item.id}
                    asChild
                  >
                    <Link
                      to={item.path}
                      onClick={handleNavClick}
                      className={cn(
                        "gap-2",
                        isActive && "bg-muted"
                      )}
                    >
                      <Icon className={cn(
                        "h-4 w-4",
                        isActive ? "text-foreground" : "text-muted-foreground"
                      )} />
                      <span>{t(item.labelKey)}</span>
                    </Link>
                  </DropdownMenuItem>
                )
              })}
            </DropdownMenuContent>
          </DropdownMenu>
        </div>

        {/* Spacer */}
        <div className="flex-1" />

        {/* Right side: Status + Language + Theme + User */}
        <div className="flex items-center gap-1 sm:gap-2">
          {/* Connection status - icon only on mobile */}
          <Tooltip>
            <TooltipTrigger asChild>
              <div
                className={cn(
                  "flex items-center gap-1.5 px-2 py-1 rounded-lg text-xs font-medium",
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
                className="hidden sm:flex h-8 px-2 rounded-lg text-muted-foreground hover:text-foreground text-xs font-medium"
              >
                {i18n.language === 'zh' ? '中' : 'EN'}
              </Button>
            </TooltipTrigger>
            <TooltipContent side="bottom" className="text-xs sm:block">
              {i18n.language === 'zh' ? 'Switch to English' : '切换到中文'}
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
                    className="w-9 h-9 rounded-xl relative"
                  >
                    <BellRing className="h-4 w-4" />
                    {unreadCount > 0 && (
                      <Badge
                        variant="destructive"
                        className="absolute -top-1 -right-1 h-5 min-w-5 px-1 flex items-center justify-center text-xs"
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
            <DropdownMenuContent align="end" className="w-80 max-h-96 overflow-y-auto">
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
                          "px-3 py-2 border-b last:border-b-0 hover:bg-muted/50 transition-colors",
                          isUnread && "bg-muted/30"
                        )}
                      >
                        <div className="flex items-start gap-2">
                          <Badge
                            variant="outline"
                            className={cn("text-xs shrink-0 mt-0.5", getSeverityColor(alert.severity))}
                          >
                            {alert.severity}
                          </Badge>
                          <div className="flex-1 min-w-0">
                            <div className="flex items-start justify-between gap-2">
                              <p className="text-sm font-medium truncate">{alert.title}</p>
                              {isUnread && (
                                <div className="w-2 h-2 rounded-full bg-blue-500 shrink-0 mt-1" />
                              )}
                            </div>
                            <p className="text-xs text-muted-foreground line-clamp-2 mt-0.5">
                              {alert.message}
                            </p>
                            {alert.source && (
                              <p className="text-xs text-muted-foreground/70 mt-1">
                                {alert.source}
                              </p>
                            )}
                          </div>
                        </div>
                        {isUnread && (
                          <div className="mt-2 flex justify-end">
                            <Button
                              variant="ghost"
                              size="sm"
                              className="h-6 px-2 text-xs"
                              onClick={() => handleAcknowledgeAlert(alert.id)}
                            >
                              {t('alerts.acknowledge')}
                            </Button>
                          </div>
                        )}
                      </div>
                    )
                  })}
                  {alerts.length > 10 && (
                    <div className="px-3 py-2 text-center text-xs text-muted-foreground">
                      {t('alerts.moreAlerts', { count: alerts.length - 10 })}
                    </div>
                  )}
                </div>
              )}
            </DropdownMenuContent>
          </DropdownMenu>

          {/* User avatar with dropdown */}
          {user && (
            <DropdownMenu>
              <DropdownMenuTrigger asChild>
                <Avatar className="h-8 w-8 cursor-pointer">
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
          )}
        </div>
      </nav>
    </TooltipProvider>
  )
}
