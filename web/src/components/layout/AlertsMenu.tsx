/**
 * AlertsMenu — the notification bell with unread badge and the alerts
 * dropdown (latest 10, severity styling, per-item + mark-all acknowledge).
 * Self-contained: owns its fetch/60s-poll lifecycle from the store.
 *
 * Lives in the AppSidebar footer. `compact` renders the icon-only trigger
 * (collapsed rail, tooltip on the right); otherwise a full-width labeled
 * row matching the sidebar nav-item style.
 */

import { useEffect, useMemo, useState } from "react"
import { useTranslation } from "react-i18next"
import { AlertTriangle, Bell, BellRing, Check, CheckCheck, Info } from "lucide-react"
import { cn } from "@/lib/utils"
import { useStore } from "@/store"
import { Button } from "@/components/ui/button"
import { Badge } from "@/components/ui/badge"
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu"
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from "@/components/ui/tooltip"

export function AlertsMenu({
  compact = false,
  align = "start",
  side = "top",
  tooltipSide = "right",
}: {
  compact?: boolean
  align?: "start" | "center" | "end"
  side?: "top" | "right" | "bottom" | "left"
  tooltipSide?: "top" | "right" | "bottom" | "left"
}) {
  const { t } = useTranslation("common")
  const alerts = useStore((state) => state.alerts)
  const fetchAlerts = useStore((state) => state.fetchAlerts)
  const acknowledgeAlert = useStore((state) => state.acknowledgeAlert)
  const [open, setOpen] = useState(false)

  useEffect(() => {
    fetchAlerts()
    const interval = setInterval(fetchAlerts, 60000)
    return () => clearInterval(interval)
  }, [fetchAlerts])

  const unreadCount = useMemo(
    () =>
      alerts.filter(
        (a) => !a.acknowledged && a.status !== "resolved" && a.status !== "acknowledged"
      ).length,
    [alerts]
  )

  const handleAcknowledgeAlert = async (alertId: string) => {
    await acknowledgeAlert(alertId)
  }

  // Severity config: icon + badge classes + left border accent
  const getSeverityConfig = (severity: string) => {
    switch (severity) {
      case "critical":
      case "emergency":
        return {
          icon: AlertTriangle,
          dot: "bg-error",
          badge: "text-error bg-error-light",
          bar: "bg-error",
        }
      case "warning":
        return {
          icon: AlertTriangle,
          dot: "bg-warning",
          badge: "text-warning bg-warning-light",
          bar: "bg-warning",
        }
      case "info":
      default:
        return {
          icon: Info,
          dot: "bg-info",
          badge: "text-info bg-info-light",
          bar: "bg-info",
        }
    }
  }

  const trigger = (
    <Button
      variant="ghost"
      aria-label={t("alerts.title")}
      className={cn(
        "h-10 font-normal no-press-scale text-muted-foreground hover:text-foreground hover:bg-muted-50",
        compact
          ? "w-10 min-w-0 px-0 justify-center relative"
          : "w-full gap-3 px-3 justify-start"
      )}
    >
      <BellRing className="h-5 w-5 shrink-0" />
      {!compact && (
        <>
          <span className="truncate">{t("alerts.title")}</span>
          {unreadCount > 0 && (
            <span className="ml-auto inline-flex h-5 min-w-5 shrink-0 items-center justify-center rounded-full bg-destructive px-1.5 text-nano font-semibold tabular-nums text-destructive-foreground">
              {unreadCount > 99 ? "99+" : unreadCount}
            </span>
          )}
        </>
      )}
      {compact && unreadCount > 0 && (
        <Badge
          variant="destructive"
          className="absolute -top-0.5 -right-0.5 h-5 min-w-5 px-1 flex items-center justify-center text-xs"
        >
          {unreadCount > 99 ? "99+" : unreadCount}
        </Badge>
      )}
    </Button>
  )

  return (
    <DropdownMenu open={open} onOpenChange={setOpen}>
      {compact ? (
        <Tooltip>
          <TooltipTrigger asChild>
            <DropdownMenuTrigger asChild>{trigger}</DropdownMenuTrigger>
          </TooltipTrigger>
          <TooltipContent side={tooltipSide} className="text-xs px-2 py-1">
            {t("alerts.title")}
          </TooltipContent>
        </Tooltip>
      ) : (
        <DropdownMenuTrigger asChild>{trigger}</DropdownMenuTrigger>
      )}
      <DropdownMenuContent align={align} side={side} className="w-[22rem] max-h-[28rem] overflow-hidden flex flex-col p-0">
        {/* Header — icon + title + unread count + mark-all */}
        <div className="flex items-center justify-between px-4 py-3 border-b shrink-0">
          <div className="flex items-center gap-2">
            <BellRing className="h-4 w-4 text-muted-foreground" />
            <span className="font-semibold text-sm">{t("alerts.title")}</span>
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
              onClick={() =>
                alerts
                  .filter((a) => !a.acknowledged)
                  .forEach((a) => handleAcknowledgeAlert(a.id))
              }
            >
              <CheckCheck className="h-3.5 w-3.5" />
              <span className="hidden sm:inline">
                {t("alerts.markAllRead", { defaultValue: "Mark all read" })}
              </span>
            </Button>
          )}
        </div>

        {/* Body */}
        {alerts.length === 0 ? (
          <div className="flex flex-col items-center justify-center py-10 text-center">
            <div className="flex h-12 w-12 items-center justify-center rounded-xl bg-primary-light text-primary mb-3">
              <Bell className="h-6 w-6" />
            </div>
            <p className="text-sm font-medium">{t("alerts.noAlerts")}</p>
            <p className="text-xs text-muted-foreground mt-1">
              {t("alerts.noAlertsDesc", { defaultValue: "You're all caught up" })}
            </p>
          </div>
        ) : (
          <div className="flex-1 overflow-y-auto">
            {alerts.slice(0, 10).map((alert) => {
              const isUnread =
                !alert.acknowledged && alert.status !== "resolved" && alert.status !== "acknowledged"
              const sev = getSeverityConfig(alert.severity)
              const SevIcon = sev.icon
              return (
                <div
                  key={alert.id}
                  className={cn(
                    "group flex gap-3 px-4 py-2.5 border-b last:border-b-0 transition-colors",
                    isUnread ? "bg-muted-30" : "bg-transparent",
                    "hover:bg-muted-50"
                  )}
                >
                  {/* Severity icon */}
                  <div
                    className={cn(
                      "flex h-7 w-7 shrink-0 items-center justify-center rounded-lg",
                      sev.badge
                    )}
                  >
                    <SevIcon className="h-3.5 w-3.5" />
                  </div>

                  {/* Content */}
                  <div className="flex-1 min-w-0">
                    <div className="flex items-center gap-1.5">
                      <p
                        className={cn(
                          "text-xs truncate flex-1",
                          isUnread ? "font-semibold" : "font-medium"
                        )}
                      >
                        {alert.title}
                      </p>
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
                      title={t("alerts.acknowledge")}
                    >
                      <Check className="h-3.5 w-3.5" />
                    </Button>
                  )}
                </div>
              )
            })}
            {alerts.length > 10 && (
              <div className="px-4 py-2.5 text-center text-xs text-muted-foreground border-t">
                {t("alerts.moreAlerts", { count: alerts.length - 10 })}
              </div>
            )}
          </div>
        )}
      </DropdownMenuContent>
    </DropdownMenu>
  )
}
