/**
 * DecisionTab — the decision layer's runtime configuration surface.
 *
 * Backend contract: every toggle here is hot-reloadable via
 * PUT /api/decisions/config (persisted to settings.redb; beats env seeds on
 * the next boot). `engine.int8` and `budgets.sidecar_ms` apply at the next
 * decision-layer refresh — the PUT already triggers one.
 */

import { useCallback, useEffect, useState } from "react"
import { useTranslation } from "react-i18next"
import { Activity, ChevronDown, Loader2, ShieldCheck, Zap } from "lucide-react"
import { Button } from "@/components/ui/button"
import { Switch } from "@/components/ui/switch"
import { Collapsible, CollapsibleContent, CollapsibleTrigger } from "@/components/ui/collapsible"
import { Badge } from "@/components/ui/badge"
import { toast } from "@/hooks/use-toast"
import { decisionApi, DecisionConfig } from "@/lib/api/decisions"

function ToggleRow({
  label,
  hint,
  checked,
  disabled,
  onChange,
}: {
  label: string
  hint?: string
  checked: boolean
  disabled?: boolean
  onChange: (v: boolean) => void
}) {
  return (
    <div className="flex items-center justify-between gap-4 py-3">
      <div className="min-w-0">
        <p className="text-sm font-medium">{label}</p>
        {hint && <p className="text-xs text-muted-foreground mt-0.5">{hint}</p>}
      </div>
      <Switch checked={checked} disabled={disabled} onCheckedChange={onChange} />
    </div>
  )
}

function NumberField({
  label,
  value,
  onCommit,
  disabled,
  min = 100,
  max = 10000,
  step = 100,
}: {
  label: string
  value: number
  onCommit: (v: number) => void
  disabled?: boolean
  min?: number
  max?: number
  step?: number
}) {
  const [draft, setDraft] = useState(String(value))
  useEffect(() => setDraft(String(value)), [value])
  return (
    <label className="flex items-center justify-between gap-4 py-2">
      <span className="text-sm">{label}</span>
      <input
        type="number"
        className="w-28 rounded-md border border-border bg-transparent px-2 py-1 text-sm text-right tabular-nums"
        value={draft}
        min={min}
        max={max}
        step={step}
        disabled={disabled}
        onChange={(e) => setDraft(e.target.value)}
        onBlur={() => {
          const n = Number(draft)
          if (Number.isFinite(n) && n !== value) onCommit(Math.min(max, Math.max(min, Math.round(n))))
          else setDraft(String(value))
        }}
      />
    </label>
  )
}

export function DecisionTab() {
  const { t } = useTranslation("settings")
  const [config, setConfig] = useState<DecisionConfig | null>(null)
  const [busy, setBusy] = useState(false)
  const [advancedOpen, setAdvancedOpen] = useState(false)

  const load = useCallback(() => {
    decisionApi.getConfig().then(setConfig).catch(() => {})
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
      <div className="flex items-center justify-center py-16 text-muted-foreground">
        <Loader2 className="h-5 w-5 animate-spin" />
      </div>
    )
  }

  const backendBadge = config.backend
    ? config.backend
    : t("decision.backendNone")

  return (
    <div className="max-w-2xl space-y-6">
      {/* Status card */}
      <div className="rounded-lg border border-border p-4 space-y-2">
        <div className="flex items-center gap-2">
          <Activity className="h-4 w-4 text-primary" />
          <span className="text-sm font-medium">{t("decision.statusTitle")}</span>
          <Badge variant={config.shadow ? "secondary" : "default"} className="ml-auto">
            {config.shadow ? t("decision.shadowOn") : t("decision.activeMode")}
          </Badge>
        </div>
        <div className="text-xs text-muted-foreground flex flex-wrap gap-x-4 gap-y-1">
          <span>{t("decision.backend")}: <span className="text-foreground">{backendBadge}</span></span>
          <span>
            {t("decision.backendAvail")}:{" "}
            <span className="text-foreground">
              {[
                config.backends.extension.available && t("decision.beExtension"),
                config.backends.native.available && t("decision.beNative"),
                config.backends.env_url.available && t("decision.beEnv"),
              ]
                .filter(Boolean)
                .join(" / ") || t("decision.beNone")}
            </span>
          </span>
        </div>
        {config.shadow && (
          <p className="text-xs text-muted-foreground">{t("decision.shadowHint")}</p>
        )}
      </div>

      {/* Action-mode toggles */}
      <div className="rounded-lg border border-border divide-y divide-border px-4">
        <div className="flex items-center gap-2 py-3">
          <Zap className="h-4 w-4 text-primary" />
          <span className="text-sm font-medium">{t("decision.modesTitle")}</span>
          {busy && <Loader2 className="h-3.5 w-3.5 animate-spin ml-auto" />}
        </div>
        <ToggleRow
          label={t("decision.shadow")}
          hint={t("decision.shadowHintShort")}
          checked={config.shadow}
          disabled={busy || !config.backend}
          onChange={(v) => apply({ shadow: v })}
        />
        <ToggleRow
          label={t("decision.prefill")}
          hint={t("decision.prefillHint")}
          checked={config.prefill}
          disabled={busy || config.shadow || !config.backend}
          onChange={(v) => apply({ prefill: v })}
        />
        <ToggleRow
          label={t("decision.guidance")}
          hint={t("decision.guidanceHint")}
          checked={config.guidance}
          disabled={busy || config.shadow || !config.backend}
          onChange={(v) => apply({ guidance: v })}
        />
        <ToggleRow
          label={t("decision.shortcircuit")}
          hint={t("decision.shortcircuitHint")}
          checked={config.shortcircuit}
          disabled={busy || config.shadow || !config.backend}
          onChange={(v) => apply({ shortcircuit: v })}
        />
      </div>

      {/* Advanced: gates / budgets / engine */}
      <Collapsible open={advancedOpen} onOpenChange={setAdvancedOpen}>
        <CollapsibleTrigger asChild>
          <Button variant="ghost" className="w-full justify-between px-4">
            <span className="flex items-center gap-2 text-sm">
              <ShieldCheck className="h-4 w-4" />
              {t("decision.advanced")}
            </span>
            <ChevronDown
              className={`h-4 w-4 transition-transform ${advancedOpen ? "rotate-180" : ""}`}
            />
          </Button>
        </CollapsibleTrigger>
        <CollapsibleContent>
          <div className="rounded-lg border border-border px-4 py-2 divide-y divide-border">
            <NumberField
              label={t("decision.gateDomain")}
              value={Math.round(config.gates.domain * 100)}
              min={0}
              max={100}
              step={5}
              disabled={busy}
              onCommit={(v) => apply({ gates: { domain: v / 100 } })}
            />
            <NumberField
              label={t("decision.gateOther")}
              value={Math.round(config.gates.other * 100)}
              min={0}
              max={100}
              step={5}
              disabled={busy}
              onCommit={(v) => apply({ gates: { other: v / 100 } })}
            />
            <NumberField
              label={t("decision.budgetPrefill")}
              value={config.budgets.prefill_ms}
              disabled={busy}
              onCommit={(v) => apply({ budgets: { prefill_ms: v } })}
            />
            <NumberField
              label={t("decision.budgetGuidance")}
              value={config.budgets.guidance_ms}
              disabled={busy}
              onCommit={(v) => apply({ budgets: { guidance_ms: v } })}
            />
            <NumberField
              label={t("decision.budgetAsk")}
              value={config.budgets.ask_ms}
              disabled={busy}
              onCommit={(v) => apply({ budgets: { ask_ms: v } })}
            />
            <NumberField
              label={t("decision.budgetSidecar")}
              value={config.budgets.sidecar_ms}
              disabled={busy}
              onCommit={(v) => apply({ budgets: { sidecar_ms: v } })}
            />
            <ToggleRow
              label={t("decision.int8")}
              hint={t("decision.int8Hint")}
              checked={config.engine.int8}
              disabled={busy}
              onChange={(v) => apply({ engine: { int8: v } })}
            />
            <p className="text-xs text-muted-foreground py-2">{t("decision.advancedHint")}</p>
          </div>
        </CollapsibleContent>
      </Collapsible>
    </div>
  )
}
