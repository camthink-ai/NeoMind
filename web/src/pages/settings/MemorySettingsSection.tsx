/**
 * Memory system settings section (Preferences tab).
 *
 * Moved from the agents-page MemoryPanel's config dialog (2026-08-26):
 * platform-level behavior configuration lives in Settings, not inside a
 * page panel — the panel keeps content management (view/edit memory files),
 * its config entry now jumps here. Same fields, same API, restyled to the
 * settings-row instant-save pattern used by the neighboring sections.
 */
import { useEffect, useState } from "react"
import { useTranslation } from "react-i18next"
import { useToast } from "@/components/ui/use-toast"
import { Switch } from "@/components/ui/switch"
import { Input } from "@/components/ui/input"
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select"
import { SettingsRow } from "./SettingsRow"
import { api } from "@/lib/api"
import type { MemorySystemConfig, LlmBackendInstance } from "@/types"

const defaultConfig: MemorySystemConfig = {
  enabled: true,
  storage_path: "data/memory",
  user_char_limit: 2000,
  knowledge_char_limit: 3000,
  procedures_char_limit: 3000,
  agent_char_limit: 1000,
  temp_file_ttl_days: 7,
  system_context_interval_secs: 600,
  summary_interval_secs: 7200,
  summary_backend_id: null,
}

export function MemorySettingsSection() {
  const { t } = useTranslation(["agents", "settings"])
  const { toast } = useToast()
  const [config, setConfig] = useState<MemorySystemConfig | null>(null)
  const [backends, setBackends] = useState<LlmBackendInstance[]>([])

  useEffect(() => {
    api.getMemoryConfig()
      .then((res) => setConfig({ ...defaultConfig, ...res }))
      .catch(() => {})
    api.listLlmBackends()
      .then((res) => setBackends(res.backends || []))
      .catch(() => {})
  }, [])

  if (!config) {
    return <div className="h-32 w-full animate-pulse rounded-md bg-muted" />
  }

  const save = async (updates: Partial<MemorySystemConfig>) => {
    const prev = config
    const next = { ...config, ...updates }
    setConfig(next)
    try {
      await api.updateMemoryConfig(next)
    } catch {
      toast({ title: t("agents:systemMemory.config.saveFailed", "Failed to save"), variant: "destructive" })
      setConfig(prev)
    }
  }

  /** Number input row: commit on blur (avoid saving per keystroke). */
  const numberRow = (
    label: string,
    desc: string,
    value: number,
    field: keyof MemorySystemConfig,
    min: number,
    max: number,
    fallback: number,
  ) => (
    <SettingsRow label={label} description={desc}>
      <Input
        type="number"
        min={min}
        max={max}
        defaultValue={value}
        key={`${field}-${value}`}
        onBlur={(e) => {
          const parsed = parseInt(e.target.value)
          if (Number.isNaN(parsed)) return
          const clamped = Math.min(max, Math.max(min, parsed))
          if (clamped !== value) save({ [field]: clamped } as Partial<MemorySystemConfig>)
        }}
        className="w-full sm:w-[140px]"
      />
    </SettingsRow>
  )

  return (
    <section>
      <h3 className="text-xs font-semibold uppercase tracking-wider text-muted-foreground mb-2">
        {t("agents:systemMemory.config.title", "Memory Configuration")}
      </h3>
      <div className="rounded-lg bg-card border border-border shadow-sm p-5 space-y-4">
        <SettingsRow
          label={t("agents:systemMemory.config.enabled", "Enabled")}
          description={t("agents:systemMemory.config.description", "Configure memory storage and scheduling")}
        >
          <Switch
            checked={config.enabled}
            onCheckedChange={(checked) => save({ enabled: checked })}
          />
        </SettingsRow>

        {numberRow(
          t("agents:systemMemory.config.userCharLimit", "User File Limit"),
          t("agents:systemMemory.config.userCharLimitHint", "Max characters for user memory file"),
          config.user_char_limit, "user_char_limit", 500, 10000, 2000,
        )}
        {numberRow(
          t("agents:systemMemory.config.knowledgeCharLimit", "Knowledge File Limit"),
          t("agents:systemMemory.config.knowledgeCharLimitHint", "Max characters for knowledge memory file"),
          config.knowledge_char_limit, "knowledge_char_limit", 500, 20000, 3000,
        )}
        {numberRow(
          t("agents:systemMemory.config.proceduresCharLimit", "Procedures File Limit"),
          t("agents:systemMemory.config.proceduresCharLimitHint", "Max characters for procedures memory file (SOPs, playbooks, how-tos)"),
          config.procedures_char_limit, "procedures_char_limit", 500, 20000, 3000,
        )}
        {numberRow(
          t("agents:systemMemory.config.tempFileTtl", "Temp File TTL (Days)"),
          t("agents:systemMemory.config.tempFileTtlHint", "Days before temp files are cleaned up"),
          config.temp_file_ttl_days, "temp_file_ttl_days", 1, 30, 7,
        )}
        {numberRow(
          t("agents:systemMemory.config.refreshInterval", "Refresh Interval (s)"),
          t("agents:systemMemory.config.refreshIntervalHint", "Seconds between resource inventory refresh", {
            minutes: Math.round((config.system_context_interval_secs || 600) / 60),
          }),
          config.system_context_interval_secs, "system_context_interval_secs", 60, 86400, 600,
        )}
        {numberRow(
          t("agents:systemMemory.config.summaryInterval", "Summary Interval (s)"),
          t("agents:systemMemory.config.summaryIntervalHint", "Seconds between LLM chat/agent summaries", {
            minutes: Math.round((config.summary_interval_secs || 7200) / 60),
          }),
          config.summary_interval_secs, "summary_interval_secs", 600, 86400, 7200,
        )}

        <SettingsRow
          label={t("agents:systemMemory.config.summaryBackend", "Summary LLM Backend")}
          description={t("agents:systemMemory.config.summaryBackendHint", "Backend used for periodic summaries (active backend if unset)")}
        >
          <Select
            value={config.summary_backend_id || "__active__"}
            onValueChange={(value) =>
              save({ summary_backend_id: value === "__active__" ? null : value })
            }
          >
            <SelectTrigger className="w-full sm:w-[220px]">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              <SelectItem value="__active__">
                {t("agents:systemMemory.config.activeBackend", "Active backend")}
              </SelectItem>
              {backends.map((b) => (
                <SelectItem key={b.id} value={b.id}>
                  {b.name}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        </SettingsRow>
      </div>
    </section>
  )
}
