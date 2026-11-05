import { Bot, Cpu, MessageSquare, MoreHorizontal } from "lucide-react"
import { useLocation, useNavigate } from "react-router-dom"
import { useTranslation } from "react-i18next"
import { cn } from "@/lib/utils"
import { useIsMobile } from "@/hooks/useMobile"

const items = [
  { id: "chat", path: "/chat", icon: MessageSquare, label: "nav.dashboard" },
  { id: "devices", path: "/devices", icon: Cpu, label: "nav.devices" },
  { id: "agents", path: "/agents", icon: Bot, label: "nav.agents" },
  { id: "more", path: "/system", icon: MoreHorizontal, label: "system.pageTitle" },
] as const

const moreRoutes = ["/system", "/visual-dashboard", "/automation", "/data", "/messages", "/extensions", "/settings"]

function activeItem(pathname: string) {
  if (pathname === "/" || pathname.startsWith("/chat")) return "chat"
  if (pathname.startsWith("/devices")) return "devices"
  if (pathname.startsWith("/agents")) return "agents"
  if (moreRoutes.some((path) => pathname === path || pathname.startsWith(`${path}/`))) return "more"
  return "more"
}

/** Global mobile destinations. Chat intentionally opts out so its composer is
 * the only control docked to the bottom edge. Business tabs live at the top of
 * their page and never compete with this bar. */
export function MobilePrimaryNav() {
  const isMobile = useIsMobile()
  const location = useLocation()
  const navigate = useNavigate()
  const { t } = useTranslation("common")

  if (!isMobile || location.pathname === "/" || location.pathname.startsWith("/chat")) return null

  const active = activeItem(location.pathname)
  return (
    <nav
      aria-label={t("common:nav.mobilePrimary")}
      className="mobile-primary-nav fixed inset-x-0 bottom-0 z-40 border-t border-border bg-background safe-bottom"
      style={{ backgroundColor: "var(--chrome)" }}
    >
      <div className="grid h-16 grid-cols-4 px-2">
        {items.map(({ id, path, icon: Icon, label }) => {
          const selected = active === id
          return (
            <button
              key={id}
              type="button"
              aria-current={selected ? "page" : undefined}
              onClick={() => navigate(path)}
              className={cn(
                "relative flex min-w-0 flex-col items-center justify-center gap-1 rounded-lg px-1 text-[11px] font-medium transition-colors",
                selected ? "text-foreground" : "text-muted-foreground active:text-foreground",
              )}
            >
              <Icon className="h-5 w-5" strokeWidth={selected ? 2.1 : 1.7} aria-hidden="true" />
              <span className="max-w-full truncate">{t(label)}</span>
              <span
                aria-hidden="true"
                className={cn(
                  "absolute bottom-1 h-0.5 w-4 rounded-full bg-foreground transition-opacity",
                  selected ? "opacity-100" : "opacity-0",
                )}
              />
            </button>
          )
        })}
      </div>
    </nav>
  )
}
