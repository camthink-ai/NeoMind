/**
 * LanAccessSection — desktop-only "allow LAN devices to connect" control.
 *
 * The embedded server binds 0.0.0.0 by default — edge devices connect
 * out of the box; disabling LAN here rebinds HTTP + the MQTT broker to
 * 127.0.0.1 after an app restart. The choice is sticky per install.
 * Renders nothing outside the Tauri desktop app.
 */

import { useCallback, useEffect, useState } from "react"
import { useTranslation } from "react-i18next"
import { invoke } from "@tauri-apps/api/core"
import { Wifi, WifiOff, RotateCw, X } from "lucide-react"
import { Switch } from "@/components/ui/switch"
import { Button } from "@/components/ui/button"
import { isTauriEnv } from "@/lib/api"
import { cn } from "@/lib/utils"

interface LanAccessState {
  desired: boolean
  effective: boolean
  restartRequired: boolean
  compatNoticePending: boolean
}

export function LanAccessSection({ compact = false }: { compact?: boolean }) {
  const { t } = useTranslation("settings")
  const [state, setState] = useState<LanAccessState | null>(null)

  useEffect(() => {
    if (!isTauriEnv()) return
    invoke<LanAccessState>("get_lan_access")
      .then(setState)
      .catch((err) => console.warn("[LanAccess] state unavailable:", err))
  }, [])

  const toggle = useCallback(async (enabled: boolean) => {
    try {
      const next = await invoke<LanAccessState>("set_lan_access", { enabled })
      setState(next)
    } catch (err) {
      console.warn("[LanAccess] set failed:", err)
    }
  }, [])

  const dismissNotice = useCallback(async () => {
    try {
      await invoke("dismiss_lan_notice")
    } catch { /* best-effort */ }
    setState((s) => (s ? { ...s, compatNoticePending: false } : s))
  }, [])

  if (!isTauriEnv() || !state) return null

  return (
    <div className={cn("overflow-hidden rounded-xl border border-border bg-surface", compact && "mx-3")}>
      {state.compatNoticePending && (
        <div className="flex items-start gap-2 border-b border-border bg-accent-orange-light/40 px-3 py-2.5">
          <Wifi className="mt-0.5 h-4 w-4 shrink-0 text-accent-orange" />
          <div className="min-w-0 flex-1 text-xs leading-relaxed text-foreground">
            {t("lan.compatNotice")}
          </div>
          <button
            type="button"
            aria-label={t("lan.dismiss")}
            className="rounded p-0.5 text-muted-foreground hover:text-foreground"
            onClick={() => void dismissNotice()}
          >
            <X className="h-3.5 w-3.5" />
          </button>
        </div>
      )}
      <div className="flex items-center gap-3 px-3 py-2.5">
        <div className={cn(
          "flex h-8 w-8 items-center justify-center rounded-lg",
          state.effective ? "bg-success-light text-success" : "bg-muted text-muted-foreground",
        )}>
          {state.effective ? <Wifi className="h-4 w-4" /> : <WifiOff className="h-4 w-4" />}
        </div>
        <div className="min-w-0 flex-1">
          <div className="text-sm font-medium text-foreground">{t("lan.title")}</div>
          <div className="text-xs text-muted-foreground">{t("lan.description")}</div>
        </div>
        <Switch
          checked={state.desired}
          onCheckedChange={(v) => void toggle(v)}
          aria-label={t("lan.title")}
        />
      </div>
      {state.restartRequired && (
        <div className="flex items-center gap-2 border-t border-border px-3 py-2">
          <span className="min-w-0 flex-1 text-xs text-accent-orange">
            {t("lan.restartRequired")}
          </span>
          <Button
            size="sm"
            variant="outline"
            className="h-7 gap-1.5 text-xs"
            onClick={() => {
              void invoke("relaunch_app").catch(() => { /* updater relaunch fallback */ })
            }}
          >
            <RotateCw className="h-3 w-3" />
            {t("lan.restartNow")}
          </Button>
        </div>
      )}
    </div>
  )
}
