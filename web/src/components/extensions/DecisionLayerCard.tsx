/**
 * DecisionLayerCard — the LAYA decision layer's status card on the
 * Extensions page.
 *
 * The decision layer is a COMPILED-IN module (decision-native feature +
 * model artifacts under <data>/models/decision/), not an extension-system
 * package — it can never be installed/uninstalled from this page. The card
 * makes it visible where users look for capabilities: mode (shadow records
 * only; guidance injects hints into the LLM), backend, and the live
 * decision/cache/error counters, with a manual refresh.
 */

import { useCallback, useEffect, useState } from "react"
import { useTranslation } from "react-i18next"
import { Cpu, RefreshCw } from "lucide-react"
import { cn } from "@/lib/utils"
import { textMini } from "@/design-system/tokens/typography"
import { decisionsApi, type DecisionStatus } from "@/lib/api/decisions"

export function DecisionLayerCard() {
  const { t } = useTranslation("extensions")
  const [status, setStatus] = useState<DecisionStatus | null>(null)
  const [loading, setLoading] = useState(true)

  const fetchStatus = useCallback(async () => {
    setLoading(true)
    try {
      setStatus(await decisionsApi.getDecisionStatus())
    } catch {
      // Non-fatal — the card degrades to the "off" state.
      setStatus(null)
    } finally {
      setLoading(false)
    }
  }, [])

  useEffect(() => {
    void fetchStatus()
  }, [fetchStatus])

  const enabled = status?.enabled === true
  const modeKey = status?.shadow
    ? "decision.modeShadow"
    : "decision.modeGuidance"
  const unavailable = enabled && status?.available === false

  return (
    <div
      className={cn(
        "rounded-lg border p-4",
        enabled ? "border-border bg-muted/20" : "border-dashed border-border/60",
      )}
      data-testid="decision-layer-card"
    >
      <div className="flex items-center gap-2">
        <Cpu
          className={cn("size-4 shrink-0", enabled ? "text-primary" : "text-muted-foreground")}
          aria-hidden
        />
        <span className="text-sm font-medium">{t("decision.title")}</span>
        <span className={cn(textMini, "rounded bg-muted px-1.5 text-muted-foreground")}>
          {t("decision.builtin")}
        </span>
        <div className="flex-1" />
        <button
          type="button"
          onClick={() => void fetchStatus()}
          className="text-muted-foreground transition-colors hover:text-foreground"
          aria-label={t("decision.refresh")}
          disabled={loading}
        >
          <RefreshCw className={cn("size-3.5", loading && "animate-spin")} aria-hidden />
        </button>
      </div>

      {!enabled ? (
        <p className={cn(textMini, "mt-2 text-muted-foreground")}>
          {status?.reason ? `${t("decision.off")} — ${status.reason}` : t("decision.offHint")}
        </p>
      ) : (
        <>
          <div className="mt-2 flex flex-wrap items-center gap-2">
            <span
              className={cn(
                textMini,
                "rounded px-1.5 py-0.5",
                unavailable
                  ? "bg-warning-light text-warning"
                  : status?.shadow
                    ? "bg-muted text-foreground"
                    : "bg-primary/10 text-primary",
              )}
              data-testid="decision-layer-mode"
            >
              {unavailable ? t("decision.unavailable") : t(modeKey)}
            </span>
            {status?.backend && (
              <span className={cn(textMini, "font-mono text-muted-foreground")}>
                {status.backend}
              </span>
            )}
          </div>
          <div className={cn(textMini, "mt-2 flex gap-4 text-muted-foreground")}>
            <span>
              {t("decision.decides")} {status?.decides_total ?? 0}
            </span>
            <span>
              {t("decision.cache")} {status?.cache_hits_total ?? 0}
            </span>
            <span>
              {t("decision.errors")} {status?.errors_total ?? 0}
            </span>
          </div>
        </>
      )}
    </div>
  )
}
