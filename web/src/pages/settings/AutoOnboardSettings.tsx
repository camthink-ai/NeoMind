/**
 * Auto-onboarding settings section (Settings → Preferences tab).
 *
 * Moved from the devices page's pending-drafts dialog (2026-08-26):
 * discovery policy is platform-level behavior configuration (like the
 * memory settings), not connection-instance management — the drafts tab's
 * config button now opens Settings here. Same 3 fields, same API, restyled
 * to the settings-row instant-save pattern.
 */
import { useEffect, useState } from "react"
import { useTranslation } from "react-i18next"
import { useToast } from "@/components/ui/use-toast"
import { Switch } from "@/components/ui/switch"
import { Input } from "@/components/ui/input"
import { SettingsRow } from "./SettingsRow"
import { api } from "@/lib/api"

interface OnboardConfig {
  enabled: boolean
  max_samples: number
  draft_retention_secs: number
}

const defaults: OnboardConfig = {
  enabled: true,
  max_samples: 10,
  draft_retention_secs: 86400, // 24 hours
}

export function AutoOnboardSettings() {
  const { t } = useTranslation(["common", "devices"])
  const { toast } = useToast()
  const [config, setConfig] = useState<OnboardConfig | null>(null)

  useEffect(() => {
    api.getOnboardConfig()
      .then((res) => setConfig({ ...defaults, ...res }))
      .catch(() => setConfig(defaults))
  }, [])

  if (!config) {
    return <div className="h-32 w-full animate-pulse rounded-md bg-muted" />
  }

  const save = async (updates: Partial<OnboardConfig>) => {
    const prev = config
    const next = { ...config, ...updates }
    setConfig(next)
    try {
      // Full config, not the delta: the PUT replaces the whole struct
      // (fields have no serde defaults server-side), so a partial body
      // 422s — and a defaulted field would silently reset its sibling.
      await api.updateOnboardConfig(next)
    } catch {
      toast({
        title: t("common:failed"),
        description: t("devices:pending.configSaveFailed"),
        variant: "destructive",
      })
      setConfig(prev)
    }
  }

  return (
    <section>
      <h3 className="text-xs font-semibold uppercase tracking-wider text-muted-foreground mb-2">
        {t("devices:pending.configTitle")}
      </h3>
      <div className="rounded-lg bg-card border border-border shadow-sm p-5 space-y-4">
        <SettingsRow
          label={t("devices:pending.configSettings.enabled")}
          description={t("devices:pending.configSettings.enabledDesc")}
        >
          <Switch
            checked={config.enabled}
            onCheckedChange={(checked) => save({ enabled: checked })}
          />
        </SettingsRow>

        <SettingsRow
          label={t("devices:pending.configSettings.maxSamples")}
          description={t("devices:pending.configSettings.maxSamplesDesc")}
        >
          <Input
            type="number"
            min={1}
            max={100}
            key={`samples-${config.max_samples}`}
            defaultValue={config.max_samples}
            disabled={!config.enabled}
            onBlur={(e) => {
              const parsed = parseInt(e.target.value)
              if (Number.isNaN(parsed)) return
              const clamped = Math.min(100, Math.max(1, parsed))
              if (clamped !== config.max_samples) save({ max_samples: clamped })
            }}
            className="w-full sm:w-[140px]"
          />
        </SettingsRow>

        <SettingsRow
          label={t("devices:pending.configSettings.retention")}
          description={`${Math.round(config.draft_retention_secs / 3600)} ${t("devices:pending.hours")} — ${t("devices:pending.configSettings.retentionDesc")}`}
        >
          <Input
            type="number"
            min={3600}
            max={604800}
            step={3600}
            key={`retention-${config.draft_retention_secs}`}
            defaultValue={config.draft_retention_secs}
            disabled={!config.enabled}
            onBlur={(e) => {
              const parsed = parseInt(e.target.value)
              if (Number.isNaN(parsed)) return
              const clamped = Math.min(604800, Math.max(3600, parsed))
              if (clamped !== config.draft_retention_secs) save({ draft_retention_secs: clamped })
            }}
            className="w-full sm:w-[160px]"
          />
        </SettingsRow>
      </div>
    </section>
  )
}
