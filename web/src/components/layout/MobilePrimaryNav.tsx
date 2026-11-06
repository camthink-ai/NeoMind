import { Bot, Cpu, LayoutDashboard, MessageSquare, Settings2 } from "lucide-react"
import { useLocation, useNavigate } from "react-router-dom"
import { useTranslation } from "react-i18next"
import { cn } from "@/lib/utils"
import { useIsMobile } from "@/hooks/useMobile"

const items = [
  { id: "chat", path: "/chat", icon: MessageSquare, label: "nav.dashboard" },
  { id: "devices", path: "/devices", icon: Cpu, label: "nav.devices" },
  { id: "agents", path: "/agents", icon: Bot, label: "nav.agents" },
  { id: "dashboard", path: "/visual-dashboard", icon: LayoutDashboard, label: "nav.dashboardShort" },
  { id: "system", path: "/system", icon: Settings2, label: "system.pageTitle" },
] as const

const systemRoutes = ["/system", "/automation", "/data", "/messages", "/extensions", "/settings"]

function activeItem(pathname: string) {
  if (pathname === "/" || pathname.startsWith("/chat")) return "chat"
  if (pathname.startsWith("/devices")) return "devices"
  if (pathname.startsWith("/agents")) return "agents"
  if (pathname.startsWith("/visual-dashboard")) return "dashboard"
  if (systemRoutes.some((path) => pathname === path || pathname.startsWith(`${path}/`))) return "system"
  return "system"
}

/** Chat is the immersive home surface: the composer owns the bottom edge, so
 * the capsule would only compete with it for space. Section navigation on
 * Chat lives in the page header's menu drawer instead. */
function isChatRoute(pathname: string) {
  return pathname === "/" || pathname === "/chat" || pathname.startsWith("/chat/")
}

/** Global mobile destinations for every page EXCEPT Chat — the five direct
 * destinations stay visible on all top-level pages, so changing sections
 * never requires opening an intermediate drawer. */
export function MobilePrimaryNav() {
  const isMobile = useIsMobile()
  const location = useLocation()
  const navigate = useNavigate()
  const { t } = useTranslation("common")

  if (!isMobile || isChatRoute(location.pathname)) return null

  const active = activeItem(location.pathname)
  return (
    <nav
      aria-label={t("common:nav.mobilePrimary")}
      className="mobile-primary-nav fixed inset-x-3 z-40 overflow-hidden rounded-[1.65rem] border border-border/80 shadow-[0_8px_28px_rgba(0,0,0,0.12)] backdrop-blur-xl transition-[opacity,transform] duration-200"
      style={{
        bottom: "calc(0.5rem + env(safe-area-inset-bottom, 0px))",
        backgroundColor: "color-mix(in srgb, var(--chrome) 94%, transparent)",
      }}
    >
      <div className="grid h-[3.75rem] grid-cols-5 gap-0.5 p-1.5">
        {items.map(({ id, path, icon: Icon, label }) => {
          const selected = active === id
          return (
            <button
              key={id}
              type="button"
              data-testid={`mobile-primary-${id}`}
              aria-current={selected ? "page" : undefined}
              onClick={() => navigate(path)}
              className={cn(
                "flex min-w-0 flex-col items-center justify-center gap-0.5 rounded-[1.15rem] px-1 text-[11px] font-medium transition-[background-color,color,transform] active:scale-[0.97]",
                selected
                  ? "bg-foreground text-background"
                  : "text-muted-foreground active:bg-muted active:text-foreground",
              )}
            >
              <Icon className="h-5 w-5" strokeWidth={selected ? 2.1 : 1.7} aria-hidden="true" />
              <span className="max-w-full truncate">{t(label)}</span>
            </button>
          )
        })}
      </div>
    </nav>
  )
}
