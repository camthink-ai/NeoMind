import { useState } from "react"
import { useTranslation } from "react-i18next"
import { useStore } from "@/store"
import { LogOut, User } from "lucide-react"
import { Button } from "@/components/ui/button"
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu"
import { Avatar, AvatarFallback } from "@/components/ui/avatar"
import { LanguageSwitcher } from "@/components/ui/LanguageSwitcher"

export function Header() {
  const { t } = useTranslation(['common', 'navigation'])
  const { currentPage, user, logout } = useStore()
  const [isLoggingOut, setIsLoggingOut] = useState(false)

  const handleLogout = async () => {
    setIsLoggingOut(true)
    try {
      await logout()
    } finally {
      setIsLoggingOut(false)
    }
  }

  const pageTitles: Record<string, string> = {
    dashboard: t('navigation:dashboard'),
    devices: t('navigation:devices'),
    alerts: t('navigation:alerts'),
    automation: t('navigation:automation'),
    commands: t('navigation:commands'),
    decisions: t('navigation:decisions'),
    settings: t('navigation:settings'),
    plugins: t('navigation:plugins'),
  }

  const getUserInitials = (username: string) => {
    return username.slice(0, 2).toUpperCase()
  }

  return (
    <header className="flex h-14 items-center justify-between border-b px-4 md:px-6">
      <div className="flex items-center gap-2">
        <div className="md:hidden w-8" />
        <h1 className="text-base font-semibold">
          {pageTitles[currentPage]}
        </h1>
      </div>

      <div className="flex items-center gap-2">
        {/* Language Switcher */}
        <LanguageSwitcher />

        {/* User Avatar with Dropdown */}
        {user && (
          <DropdownMenu>
            <DropdownMenuTrigger asChild>
              <Button variant="ghost" className="h-8 w-8 rounded-full p-0">
                <Avatar className="h-8 w-8">
                  <AvatarFallback className="bg-primary text-primary-foreground text-xs font-medium">
                    {getUserInitials(user.username)}
                  </AvatarFallback>
                </Avatar>
              </Button>
            </DropdownMenuTrigger>
            <DropdownMenuContent align="end" className="w-48">
              <div className="flex items-center gap-2 px-2 py-1.5">
                <div className="h-8 w-8 rounded-full bg-primary/10 flex items-center justify-center">
                  <User className="h-4 w-4 text-primary" />
                </div>
                <div className="flex flex-col">
                  <span className="text-sm font-medium">{user.username}</span>
                  <span className="text-xs text-muted-foreground">管理员</span>
                </div>
              </div>
              <DropdownMenuSeparator />
              <DropdownMenuItem
                onClick={handleLogout}
                disabled={isLoggingOut}
                className="text-destructive focus:text-destructive"
              >
                <LogOut className="h-4 w-4 mr-2" />
                {isLoggingOut ? t('common:loggingOut') : t('common:logout')}
              </DropdownMenuItem>
            </DropdownMenuContent>
          </DropdownMenu>
        )}
      </div>
    </header>
  )
}
