/**
 * TopNav - Top navigation bar with icon-only buttons
 * Clean, compact design with module navigation
 * Responsive: hamburger menu on mobile, full icons on desktop
 */

import { useStore } from "@/store"
import { cn } from "@/lib/utils"
import { useTranslation } from "react-i18next"
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
} from "lucide-react"
import { Button } from "@/components/ui/button"
import { Avatar, AvatarFallback } from "@/components/ui/avatar"
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
import { useState } from "react"

type PageType = "dashboard" | "devices" | "automation" | "events" | "plugins" | "settings"

interface NavItem {
  id: PageType
  icon: React.ComponentType<{ className?: string }>
  labelKey: string
}

const navItems: NavItem[] = [
  { id: "dashboard", labelKey: "nav.dashboard", icon: MessageSquare },
  { id: "devices", labelKey: "nav.devices", icon: Cpu },
  { id: "automation", labelKey: "nav.automation", icon: Workflow },
  { id: "events", labelKey: "nav.events", icon: Bell },
  { id: "plugins", labelKey: "nav.plugins", icon: Puzzle },
  { id: "settings", labelKey: "nav.settings", icon: Settings },
]

export function TopNav() {
  const { t, i18n } = useTranslation('common')
  const currentPage = useStore((state) => state.currentPage)
  const setCurrentPage = useStore((state) => state.setCurrentPage)
  const user = useStore((state) => state.user)
  const isConnected = useStore((state) => state.wsConnected)
  const logout = useStore((state) => state.logout)
  const [mobileMenuOpen, setMobileMenuOpen] = useState(false)

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

  const handleNavClick = (pageId: PageType) => {
    setCurrentPage(pageId)
    setMobileMenuOpen(false)
  }

  return (
    <TooltipProvider delayDuration={500}>
      <nav className="h-14 bg-background/95 backdrop-blur flex items-center px-4 shadow-sm z-50 relative">
        {/* Logo */}
        <div className="flex items-center gap-2 mr-6">
          <div className="w-8 h-8 rounded-lg bg-foreground flex items-center justify-center">
            <Sparkles className="h-4 w-4 text-background" />
          </div>
          <span className="font-semibold text-foreground hidden sm:block">NeoTalk</span>
        </div>

        {/* Desktop Navigation Icons - hidden on mobile */}
        <div className="hidden md:flex items-center gap-1">
          {navItems.map((item) => {
            const Icon = item.icon
            const isActive = currentPage === item.id

            return (
              <Tooltip key={item.id}>
                <TooltipTrigger asChild>
                  <Button
                    variant="ghost"
                    size="icon"
                    onClick={() => setCurrentPage(item.id)}
                    className={cn(
                      "w-10 h-10 rounded-xl transition-all",
                      isActive
                        ? "bg-foreground text-background hover:bg-foreground hover:text-background"
                        : "text-muted-foreground hover:text-foreground hover:bg-muted"
                    )}
                  >
                    <Icon className="h-5 w-5" />
                  </Button>
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
                const isActive = currentPage === item.id

                return (
                  <DropdownMenuItem
                    key={item.id}
                    onClick={() => handleNavClick(item.id)}
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
