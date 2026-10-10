/**
 * DecisionTab — the decision layer's runtime configuration surface,
 * following the Preferences tab's layout conventions (section → card →
 * SettingsRow).
 *
 * Backend contract: every toggle here is hot-reloadable via
 * PUT /api/decisions/config (persisted to settings.redb; beats env seeds on
 * the next boot). `engine.int8` and `budgets.sidecar_ms` apply at the next
 * decision-layer refresh — the PUT already triggers one.
 */

import { useCallback, useEffect, useState } from "react"
import { useTranslation } from "react-i18next"
import { Badge } from "@/components/ui/badge"
import { Button } from "@/components/ui/button"
import { Input } from "@/components/ui/input"
import { Switch } from "@/components/ui/switch"
import { RefreshCw } from "lucide-react"
import { toast } from "@/hooks/use-toast"
import { SettingsRow } from "./SettingsRow"
import { decisionApi, DecisionConfig, DecisionStatus } from "@/lib/api/decisions"

function PercentInput({
  value,
  disabled,
  onCommit,
}: {
  value: number
  disabled?: boolean
  onCommit: (v: number) => void
}) {
  const [draft, setDraft] = useState(String(Math.round(value * 100)))
  useEffect(() => setDraft(String(Math.round(value * 100))), [value])
  return (
    <div className="flex items-center gap-1 w-[110px]">
      <Input
        type="number"
        min={0}
        max={100}
        step={5}
        className="text-right tabular-nums"
        value={draft}
        disabled={disabled}
        onChange={(e) => setDraft(e.target.value)}
        onBlur={() => {
          const n = Number(draft)
          if (Number.isFinite(n) && n / 100 !== value) {
            onCommit(Math.min(100, Math.max(0, Math.round(n))) / 100)
          } else {
            setDraft(String(Math.round(value * 100)))
          }
        }}
      />
      <span className="text-xs text-muted-foreground">%</span>
    </div>
  )
}

function MsInput({
  value,
  disabled,
  onCommit,
}: {
  value: number
  disabled?: boolean
  onCommit: (v: number) => void
}) {
  const [draft, setDraft] = useState(String(value))
  useEffect(() => setDraft(String(value)), [value])
  return (
    <div className="flex items-center gap-1 w-[130px]">
      <Input
        type="number"
        min={100}
        max={10000}
        step={100}
        className="text-right tabular-nums"
        value={draft}
        disabled={disabled}
        onChange={(e) => setDraft(e.target.value)}
        onBlur={() => {
          const n = Number(draft)
          if (Number.isFinite(n) && n !== value) {
            onCommit(Math.min(10000, Math.max(100, Math.round(n))))
          } else {
            setDraft(String(value))
          }
        }}
      />
      <span className="text-xs text-muted-foreground">ms</span>
    </div>
  )
}

export function DecisionTab() {
  const { t } = useTranslation("settings")
  const [config, setConfig] = useState<DecisionConfig | null>(null)
  const [status, setStatus] = useState<DecisionStatus | null>(null)
  const [loadFailed, setLoadFailed] = useState(false)
  const [busy, setBusy] = useState(false)

  const load = useCallback(() => {
    setLoadFailed(false)
    decisionApi
      .getConfig()
      .then(setConfig)
      .catch(() => setLoadFailed(true))
    decisionApi.getStatus().then(setStatus).catch(() => {})
  }, [])
  useEffect(() => { load() }, [load])

  const apply = useCallback(
    async (update: Parameters<typeof decisionApi.updateConfig>[0]) => {
      setBusy(true)
      try {
        const next = await decisionApi.updateConfig(update)
        setConfig(next)
      } catch {
        toast({ title: t("decision.updateFailed"), variant: "destructive" })
      } finally {
        setBusy(false)
      }
    },
    [t],
  )

  if (!config) {
    return (
      <div>
        <section>
          <div className="rounded-lg bg-card border border-border p-5 text-center text-sm text-muted-foreground">
            {loadFailed ? t("decision.loadFailed") : t("decision.loading")}
          </div>
        </section>
      </div>
    )
  }

  const availableBackends = [
    config.backends.extension.available && t("decision.beExtension"),
    config.backends.native.available && t("decision.beNative"),
    config.backends.env_url.available && t("decision.beEnv"),
  ].filter(Boolean).join(" / ") || t("decision.beNone")

  return (
    <div className="space-y-8">
      {/* Status */}
      <section>
        <h3 className="text-xs font-semibold uppercase tracking-wider text-muted-foreground mb-2">
          {t("decision.statusTitle")}
        </h3>
        <div className="rounded-lg bg-card border border-border p-5 space-y-3">
          <div className="flex flex-col sm:flex-row sm:items-center sm:justify-between gap-2">
            <div className="text-sm">
              <span className="text-muted-foreground">{t("decision.backend")}: </span>
              <span className="font-medium">{config.backend ?? t("decision.backendNone")}</span>
            </div>
            <Badge
              variant={
                config.shadow == null ? "outline" : config.shadow ? "secondary" : "default"
              }
            >
              {config.shadow == null
                ? t("decision.backendNone")
                : config.shadow
                  ? t("decision.shadowOn")
                  : t("decision.activeMode")}
            </Badge>
          </div>
          <SettingsRow label={t("decision.backendAvail")} description={t("decision.backendAvailDesc")}>
            <span className="text-sm text-muted-foreground">{availableBackends}</span>
          </SettingsRow>
          {status?.enabled && (
            <div className="text-xs text-muted-foreground flex flex-wrap gap-x-4 pt-1">
              <span>
                {t("decision.decisions")}: <span className="text-foreground tabular-nums">{status.decides_total ?? 0}</span>
              </span>
              <span>
                {t("decision.cacheHits")}: <span className="text-foreground tabular-nums">{status.cache_hits_total ?? 0}</span>
              </span>
              <span>
                {t("decision.errors")}: <span className="text-foreground tabular-nums">{status.errors_total ?? 0}</span>
              </span>
            </div>
          )}
          {!config.backend && (
            <p className="text-xs text-amber-600 dark:text-amber-400">
              {t("decision.noBackendHint")}
            </p>
          )}
          {config.backend && config.shadow && (
            <p className="text-xs text-muted-foreground">{t("decision.shadowHint")}</p>
          )}
        </div>
      </section>

      {/* Action modes */}
      <section>
        <h3 className="text-xs font-semibold uppercase tracking-wider text-muted-foreground mb-2">
          {t("decision.modesTitle")}
        </h3>
        <div className="rounded-lg bg-card border border-border p-5 space-y-4">
          <SettingsRow
            label={t("decision.shadow")}
            description={t("decision.shadowHintShort")}
          >
            <Switch
              checked={config.shadow ?? false}
              disabled={busy || !config.backend}
              onCheckedChange={(v) => apply({ shadow: v })}
            />
          </SettingsRow>
          <SettingsRow label={t("decision.shortcircuit")} description={t("decision.shortcircuitHint")}>
            <Switch
              checked={config.shortcircuit}
              disabled={busy || config.shadow !== false || !config.backend}
              onCheckedChange={(v) => apply({ shortcircuit: v })}
            />
          </SettingsRow>
          {config.shadow !== false && !!config.backend && (
            <p className="text-xs text-muted-foreground">{t("decision.modesLockedHint")}</p>
          )}
        </div>
      </section>

      {/* Advanced: gates / budgets / engine */}
      <section>
        <h3 className="text-xs font-semibold uppercase tracking-wider text-muted-foreground mb-2">
          {t("decision.advanced")}
        </h3>
        <div className="rounded-lg bg-card border border-border p-5 space-y-4">
          <SettingsRow label={t("decision.gateDomain")} description={t("decision.gateDomainDesc")}>
            <PercentInput
              value={config.gates.domain}
              disabled={busy}
              onCommit={(v) => apply({ gates: { domain: v } })}
            />
          </SettingsRow>
          <SettingsRow label={t("decision.gateOther")} description={t("decision.gateOtherDesc")}>
            <PercentInput
              value={config.gates.other}
              disabled={busy}
              onCommit={(v) => apply({ gates: { other: v } })}
            />
          </SettingsRow>
          <SettingsRow label={t("decision.budgetShortcircuit")} description={t("decision.budgetDesc")}>
            <MsInput
              value={config.budgets.shortcircuit_ms}
              disabled={busy}
              onCommit={(v) => apply({ budgets: { shortcircuit_ms: v } })}
            />
          </SettingsRow>
          <SettingsRow label={t("decision.budgetAsk")} description={t("decision.budgetDesc")}>
            <MsInput
              value={config.budgets.ask_ms}
              disabled={busy}
              onCommit={(v) => apply({ budgets: { ask_ms: v } })}
            />
          </SettingsRow>
          <SettingsRow label={t("decision.budgetSidecar")} description={t("decision.budgetSidecarDesc")}>
            <MsInput
              value={config.budgets.sidecar_ms}
              disabled={busy}
              onCommit={(v) => apply({ budgets: { sidecar_ms: v } })}
            />
          </SettingsRow>
          <SettingsRow label={t("decision.int8")} description={t("decision.int8Hint")}>
            <Switch
              checked={config.engine.int8}
              disabled={busy}
              onCheckedChange={(v) => apply({ engine: { int8: v } })}
            />
          </SettingsRow>
          <div className="pt-2 flex items-center justify-between">
            <p className="text-xs text-muted-foreground">{t("decision.advancedHint")}</p>
            <Button variant="ghost" size="sm" className="h-7 gap-1 text-xs" onClick={load}>
              <RefreshCw className="h-3 w-3" />
              {t("decision.refresh")}
            </Button>
          </div>
        </div>
      </section>
    </div>
  )
}
