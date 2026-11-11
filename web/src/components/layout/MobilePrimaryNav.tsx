import { useLocation, useNavigate } from "react-router-dom"
import { useTranslation } from "react-i18next"
import { cn } from "@/lib/utils"
import { useIsMobile } from "@/hooks/useMobile"

const items = [
  { id: "chat", path: "/chat", label: "nav.dashboard" },
  { id: "system", path: "/system", label: "system.pageTitle" },
] as const

function activeItem(pathname: string) {
  if (pathname === "/" || pathname.startsWith("/chat")) return "chat"
  return "system"
}

/** Chat is the immersive home surface: the composer owns the bottom edge, so
 * the capsule would only compete with it for space. Section navigation on
 * Chat lives in the page header's menu drawer instead. */
function isChatRoute(pathname: string) {
  return pathname === "/" || pathname === "/chat" || pathname.startsWith("/chat/")
}

/** Chat and System are the only global levels. Business destinations live in
 * the System hub, so the bottom capsule does not mix global and local routes. */
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
      className="mobile-primary-nav fixed left-1/2 z-40 w-52 -translate-x-1/2 overflow-hidden rounded-[1.65rem] border border-border shadow-lg backdrop-blur-xl transition-[opacity,transform] duration-200"
      style={{
        bottom: "calc(0.5rem + env(safe-area-inset-bottom, 0px))",
        backgroundColor: "color-mix(in srgb, var(--chrome) 94%, transparent)",
      }}
    >
      {/* h-[3.75rem] + p-1.5 keeps each segment ≥44px tall (DESIGN_SPEC touch floor). */}
      <div className="grid h-[3.75rem] grid-cols-2 gap-1 p-1.5">
        {items.map(({ id, path, label }) => {
          const selected = active === id
          return (
            <button
              key={id}
              type="button"
              data-testid={`mobile-primary-${id}`}
              aria-current={selected ? "page" : undefined}
              onClick={() => navigate(path)}
              className={cn(
                "flex min-h-11 min-w-0 items-center justify-center rounded-[1.15rem] px-2 text-xs font-medium transition-[background-color,color,transform] active:scale-[0.97]",
                selected
                  ? "bg-foreground text-background"
                  : "text-muted-foreground active:bg-muted active:text-foreground",
              )}
            >
              <span className="max-w-full truncate">{t(label)}</span>
            </button>
          )
        })}
      </div>
    </nav>
  )
}
