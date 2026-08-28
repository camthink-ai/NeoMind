import { useState, useEffect } from "react"
import { useTranslation } from "react-i18next"
import { useErrorHandler } from "@/hooks/useErrorHandler"
import { logError } from "@/lib/errors"
import { SettingsRow } from "./SettingsRow"
import { MemorySettingsSection } from "./MemorySettingsSection"
import { AutoOnboardSettings } from "./AutoOnboardSettings"
import { Button } from "@/components/ui/button"
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select"
import {
  Check,
  Info,
  Loader2,
  Database,
  SwitchCamera,
  Download,
} from "lucide-react"
import { Switch } from "@/components/ui/switch"
import { useToast } from "@/hooks/use-toast"
import { api } from "@/lib/api"
import { useStore } from "@/store"
import { useGlobalTimezone } from "@/hooks/useTimeFormat"
import { getLocalizedTimezones } from "@/lib/time"

type Language = "zh" | "en"
type TimeFormat = "12h" | "24h"

interface Preferences {
  language: Language
  timeFormat: TimeFormat
  // Keep timeZone for backward compatibility
  timeZone?: "local" | "utc"
}

const PREFERENCES_KEY = "neomind_preferences"

// Default preferences
const defaultPreferences: Preferences = {
  language: "zh",
  timeFormat: "24h",
}

// Load preferences from localStorage
function loadPreferences(): Preferences {
  try {
    const saved = localStorage.getItem(PREFERENCES_KEY)
    if (saved) {
      const parsed = JSON.parse(saved)
      // Remove legacy theme field if present
      delete parsed.theme
      return { ...defaultPreferences, ...parsed }
    }
  } catch (e) {
    logError(e, { operation: 'Load preferences' })
  }
  return defaultPreferences
}

// Save preferences to localStorage
function savePreferences(pref: Preferences) {
  try {
    localStorage.setItem(PREFERENCES_KEY, JSON.stringify(pref))
  } catch (e) {
    logError(e, { operation: 'Save preferences' })
  }
}

export function PreferencesTab() {
  const { t, i18n } = useTranslation(["common", "settings"])
  const { handleError } = useErrorHandler()
  const { toast } = useToast()
  const [preferences, setPreferences] = useState<Preferences>(loadPreferences)
  const [hasChanges, setHasChanges] = useState(false)

  // Global timezone for scheduling (separate from UI display)
  const {
    timezone: globalTimezone,
    isLoading: timezoneLoading,
    updateTimezone,
    availableTimezones,
    refresh: refreshTimezone,
  } = useGlobalTimezone()

  // Update preferences
  const updatePreference = <K extends keyof Preferences>(
    key: K,
    value: Preferences[K]
  ) => {
    setPreferences((prev) => ({ ...prev, [key]: value }))
    setHasChanges(true)
  }

  // Save all preferences
  const handleSave = () => {
    savePreferences(preferences)
    i18n.changeLanguage(preferences.language)
    setHasChanges(false)

    toast({
      title: t("settings:settingsSaved"),
    })
  }

  // Reset to defaults
  const handleReset = () => {
    setPreferences(defaultPreferences)
    setHasChanges(true)
  }

  const languageOptions = [
    { value: "zh" as Language, label: "简体中文" },
    { value: "en" as Language, label: "English" },
  ]

  const timeFormatOptions = [
    { value: "12h" as TimeFormat, label: t("settings:timeFormat12h") },
    { value: "24h" as TimeFormat, label: t("settings:timeFormat24h") },
  ]

  // Get localized timezone list
  const localizedTimezones = getLocalizedTimezones(t)

  return (
    <div className="space-y-8">
      {/* Actions */}
      {hasChanges && (
        <div className="flex items-center justify-between p-4 bg-muted-50 rounded-lg">
          <div className="flex items-center gap-2 text-sm text-muted-foreground">
            <Info className="h-4 w-4" />
            <span>{t("settings:unsavedChanges")}</span>
          </div>
          <div className="flex gap-2">
            <Button variant="outline" size="sm" onClick={handleReset}>
              {t("common:reset")}
            </Button>
            <Button size="sm" onClick={handleSave}>
              <Check className="h-4 w-4 mr-1" />
              {t("settings:saveSettings")}
            </Button>
          </div>
        </div>
      )}

      {/* Language & Region Settings */}
      <section>
        <h3 className="text-xs font-semibold uppercase tracking-wider text-muted-foreground mb-2">
          {t("settings:languageRegion")}
        </h3>
        <div className="rounded-lg bg-card border border-border shadow-sm p-5 space-y-4">
          {/* Language */}
          <SettingsRow
            label={t("settings:language")}
            description={t("settings:languageDesc")}
          >
            <Select
              value={preferences.language}
              onValueChange={(v) => updatePreference("language", v as Language)}
            >
              <SelectTrigger className="w-full sm:w-[180px]">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                {languageOptions.map((option) => (
                  <SelectItem key={option.value} value={option.value}>
                    {option.label}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </SettingsRow>
        </div>
      </section>

      {/* Time Settings */}
      <section>
        <h3 className="text-xs font-semibold uppercase tracking-wider text-muted-foreground mb-2">
          {t("settings:timeSettings")}
        </h3>
        <div className="rounded-lg bg-card border border-border shadow-sm p-5 space-y-4">
          {/* Time Format */}
          <SettingsRow
            label={t("settings:timeFormat")}
            description={t("settings:timeFormatDesc")}
          >
            <Select
              value={preferences.timeFormat}
              onValueChange={(v) => updatePreference("timeFormat", v as TimeFormat)}
            >
              <SelectTrigger className="w-full sm:w-[180px]">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                {timeFormatOptions.map((option) => (
                  <SelectItem key={option.value} value={option.value}>
                    {option.label}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </SettingsRow>

          {/* System Timezone */}
          <SettingsRow
            label={t("settings:systemTimezone")}
            description={t("settings:systemTimezoneDesc")}
          >
            <div className="flex items-center gap-2">
              {timezoneLoading && (
                <Loader2 className="h-4 w-4 animate-spin text-muted-foreground" />
              )}
              <Select
                value={globalTimezone}
                onValueChange={async (value) => {
                  try {
                    await updateTimezone(value)
                    toast({
                      title: t("settings:timezoneUpdated"),
                    })
                  } catch (e) {
                    toast({
                      title: t("settings:timezoneUpdateFailed"),
                      variant: "destructive",
                    })
                  }
                }}
                disabled={timezoneLoading}
              >
                <SelectTrigger className="w-full sm:w-[280px]">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  {(availableTimezones.length > 0 ? availableTimezones : localizedTimezones).map(
                    (tz: { id: string; name: string }) => (
                      <SelectItem key={tz.id} value={tz.id}>
                        {tz.name}
                      </SelectItem>
                    )
                  )}
                </SelectContent>
              </Select>
            </div>
          </SettingsRow>

          {/* Current Time Preview */}
          <div className="pt-3">
            <div className="text-center p-4 bg-muted-50 rounded-lg">
              <div className="text-xs text-muted-foreground mb-1">
                {t("settings:currentTime")}
              </div>
              <div className="text-2xl font-mono font-medium">
                {formatTimeInTimezone(globalTimezone, preferences.timeFormat)}
              </div>
            </div>
          </div>
        </div>
      </section>

      {/* AI Agent Defaults */}
      <AgentDefaultsSection />

      {/* Memory System — moved from agents-page MemoryPanel config dialog */}
      <MemorySettingsSection />

      {/* Auto-onboarding — moved from devices-page pending-drafts dialog */}
      <AutoOnboardSettings />

      {/* Device Defaults */}
      <DeviceDefaultsSection />

      {/* Data Management */}
      <DataManagementSection />

      {/* Backup schedule */}
      <BackupSettingsSection />

      {/* Diagnostic Data — log archive download */}
      <DiagnosticDataSection />

      {/* Info */}
      <div className="text-sm text-muted-foreground text-center py-4">
        <p>{t("settings:preferencesInfo")}</p>
      </div>
    </div>
  )
}

// Retention option values (hours, null = forever)
const retentionOptions: { value: string; labelKey: string }[] = [
  { value: "never", labelKey: "settings:retentionNever" },
  { value: "12", labelKey: "settings:retention12h" },
  { value: "24", labelKey: "settings:retention1d" },
  { value: "72", labelKey: "settings:retention3d" },
  { value: "168", labelKey: "settings:retention7d" },
  { value: "720", labelKey: "settings:retention30d" },
  { value: "2160", labelKey: "settings:retention90d" },
]

function hoursToOption(hours: number | null | undefined): string {
  if (hours === null || hours === undefined) return "never"
  return String(hours)
}

function optionToHours(value: string): number | null {
  if (value === "never") return null
  return Number(value)
}

function AgentDefaultsSection() {
  const { t } = useTranslation(["common", "settings"])
  const { toast } = useToast()
  const [config, setConfig] = useState<{
    max_rounds: number
    execution_timeout_secs: number
    tool_concurrency: number
    default_temperature: number
    default_top_p: number
    default_thinking_enabled: boolean | null
  } | null>(null)
  const [loading, setLoading] = useState(true)

  useEffect(() => {
    api.get("/settings/agent")
      .then((data: any) => setConfig(data))
      .catch(() => {})
      .finally(() => setLoading(false))
  }, [])

  const saveConfig = async (updates: Partial<typeof config>) => {
    if (!config) return
    const next = { ...config, ...updates }
    setConfig(next)
    try {
      await api.put("/settings/agent", next)
    } catch {
      toast({ title: "Failed to save", variant: "destructive" })
      setConfig(config)
    }
  }

  if (loading || !config) {
    return <div className="h-32 w-full animate-pulse rounded-md bg-muted" />
  }

  const roundOpts = [10, 20, 30, 40, 50]
  const timeoutOpts = [
    { v: 60, l: "1 min" }, { v: 180, l: "3 min" }, { v: 300, l: "5 min" },
    { v: 600, l: "10 min" }, { v: 1800, l: "30 min" },
  ]
  const concOpts = [2, 4, 6, 8, 12, 16]
  const tempOpts = [0.1, 0.2, 0.3, 0.4, 0.5, 0.6, 0.7, 0.8, 0.9, 1.0]
  const topPOpts = [0.5, 0.6, 0.7, 0.8, 0.9, 1.0]

  return (
    <section>
      <h3 className="text-xs font-semibold uppercase tracking-wider text-muted-foreground mb-2">
        {t("settings:agentDefaults")}
      </h3>
      <div className="rounded-lg bg-card border border-border shadow-sm p-5 space-y-4">
        <SettingsRow label={t("settings:maxRounds")} description={t("settings:maxRoundsDesc")}>
          <Select value={String(config.max_rounds)} onValueChange={(v) => saveConfig({ max_rounds: +v })}>
            <SelectTrigger className="w-full sm:w-[180px]"><SelectValue /></SelectTrigger>
            <SelectContent>
              {roundOpts.map((r) => <SelectItem key={r} value={String(r)}>{r}</SelectItem>)}
            </SelectContent>
          </Select>
        </SettingsRow>
        <SettingsRow label={t("settings:executionTimeout")} description={t("settings:executionTimeoutDesc")}>
          <Select value={String(config.execution_timeout_secs)} onValueChange={(v) => saveConfig({ execution_timeout_secs: +v })}>
            <SelectTrigger className="w-full sm:w-[180px]"><SelectValue /></SelectTrigger>
            <SelectContent>
              {timeoutOpts.map((o) => <SelectItem key={o.v} value={String(o.v)}>{o.l}</SelectItem>)}
            </SelectContent>
          </Select>
        </SettingsRow>
        <SettingsRow label={t("settings:toolConcurrency")} description={t("settings:toolConcurrencyDesc")}>
          <Select value={String(config.tool_concurrency)} onValueChange={(v) => saveConfig({ tool_concurrency: +v })}>
            <SelectTrigger className="w-full sm:w-[180px]"><SelectValue /></SelectTrigger>
            <SelectContent>
              {concOpts.map((c) => <SelectItem key={c} value={String(c)}>{c}</SelectItem>)}
            </SelectContent>
          </Select>
        </SettingsRow>
        <SettingsRow label={t("settings:defaultTemperature")} description={t("settings:defaultTemperatureDesc")}>
          <Select value={config.default_temperature.toFixed(1)} onValueChange={(v) => saveConfig({ default_temperature: +v })}>
            <SelectTrigger className="w-full sm:w-[180px]"><SelectValue /></SelectTrigger>
            <SelectContent>
              {tempOpts.map((tp) => <SelectItem key={tp} value={tp.toFixed(1)}>{tp.toFixed(1)}</SelectItem>)}
            </SelectContent>
          </Select>
        </SettingsRow>
        <SettingsRow label={t("settings:defaultTopP")} description={t("settings:defaultTopPDesc")}>
          <Select value={config.default_top_p.toFixed(1)} onValueChange={(v) => saveConfig({ default_top_p: +v })}>
            <SelectTrigger className="w-full sm:w-[180px]"><SelectValue /></SelectTrigger>
            <SelectContent>
              {topPOpts.map((tp) => <SelectItem key={tp} value={tp.toFixed(1)}>{tp.toFixed(1)}</SelectItem>)}
            </SelectContent>
          </Select>
        </SettingsRow>
        <SettingsRow label={t("settings:defaultThinking")} description={t("settings:defaultThinkingDesc")}>
          <Select
            value={config.default_thinking_enabled === null ? "auto" : String(config.default_thinking_enabled)}
            onValueChange={(v) => saveConfig({ default_thinking_enabled: v === "auto" ? null : v === "true" })}
          >
            <SelectTrigger className="w-full sm:w-[180px]"><SelectValue /></SelectTrigger>
            <SelectContent>
              <SelectItem value="auto">{t("settings:thinkingAuto")}</SelectItem>
              <SelectItem value="true">On</SelectItem>
              <SelectItem value="false">Off</SelectItem>
            </SelectContent>
          </Select>
        </SettingsRow>
      </div>
    </section>
  )
}

function DeviceDefaultsSection() {
  const { t } = useTranslation(["common", "settings"])
  const { toast } = useToast()
  const [config, setConfig] = useState<{
    default_offline_timeout_secs: number
    auto_onboard_enabled: boolean
  } | null>(null)
  const [loading, setLoading] = useState(true)

  useEffect(() => {
    api.get("/settings/device")
      .then((data: any) => setConfig(data))
      .catch(() => {})
      .finally(() => setLoading(false))
  }, [])

  const saveConfig = async (updates: Partial<typeof config>) => {
    if (!config) return
    const next = { ...config, ...updates }
    setConfig(next)
    try {
      await api.put("/settings/device", next)
    } catch {
      toast({ title: "Failed to save", variant: "destructive" })
      setConfig(config)
    }
  }

  if (loading || !config) {
    return <div className="h-20 w-full animate-pulse rounded-md bg-muted" />
  }

  const timeoutOpts = [
    { v: 60, l: "1 min" }, { v: 120, l: "2 min" }, { v: 300, l: "5 min" },
    { v: 600, l: "10 min" }, { v: 1800, l: "30 min" },
  ]

  return (
    <section>
      <h3 className="text-xs font-semibold uppercase tracking-wider text-muted-foreground mb-2">
        {t("settings:deviceDefaults")}
      </h3>
      <div className="rounded-lg bg-card border border-border shadow-sm p-5 space-y-4">
        <SettingsRow label={t("settings:defaultOfflineTimeout")} description={t("settings:defaultOfflineTimeoutDesc")}>
          <Select value={String(config.default_offline_timeout_secs)} onValueChange={(v) => saveConfig({ default_offline_timeout_secs: +v })}>
            <SelectTrigger className="w-full sm:w-[180px]"><SelectValue /></SelectTrigger>
            <SelectContent>
              {timeoutOpts.map((o) => <SelectItem key={o.v} value={String(o.v)}>{o.l}</SelectItem>)}
            </SelectContent>
          </Select>
        </SettingsRow>
        <SettingsRow label={t("settings:autoOnboardEnabled")} description={t("settings:autoOnboardEnabledDesc")}>
          <Switch checked={config.auto_onboard_enabled} onCheckedChange={(checked) => saveConfig({ auto_onboard_enabled: checked })} />
        </SettingsRow>
      </div>
    </section>
  )
}

function DataManagementSection() {
  const { t } = useTranslation(["common", "settings"])
  const { toast } = useToast()
  const [config, setConfig] = useState<{
    enabled: boolean
    interval_hours: number
    default_retention: number | null
    image_retention: number | null
  } | null>(null)
  const [loading, setLoading] = useState(true)
  const [cleaning, setCleaning] = useState(false)

  useEffect(() => {
    api.get("/settings/retention")
      .then((data: any) => setConfig(data))
      .catch(() => {})
      .finally(() => setLoading(false))
  }, [])

  const saveConfig = async (updates: Partial<typeof config>) => {
    if (!config) return
    const newConfig = { ...config, ...updates }
    setConfig(newConfig)
    try {
      await api.put("/settings/retention", newConfig)
      toast({ title: t("settings:retentionUpdated") })
    } catch {
      toast({ title: t("settings:retentionUpdateFailed"), variant: "destructive" })
    }
  }

  const handleCleanup = async () => {
    setCleaning(true)
    try {
      const result: any = await api.post("/settings/retention/cleanup", {})
      toast({
        title: t("settings:cleanupSuccess", { count: result.points_removed ?? 0 }),
      })
    } catch {
      toast({ title: t("settings:cleanupFailed"), variant: "destructive" })
    } finally {
      setCleaning(false)
    }
  }

  if (loading) {
    return <div className="h-40 w-full animate-pulse rounded-md bg-muted" />
  }

  if (!config) return null

  return (
    <section>
      <h3 className="text-xs font-semibold uppercase tracking-wider text-muted-foreground mb-2">
        {t("settings:dataManagement")}
      </h3>
      <div className="rounded-lg bg-card border border-border shadow-sm p-5 space-y-5">
        {/* Auto Cleanup Toggle */}
        <SettingsRow
          label={t("settings:autoCleanup")}
          description={t("settings:autoCleanupDesc")}
        >
          <Switch
            checked={config.enabled}
            onCheckedChange={(checked) => saveConfig({ enabled: checked })}
          />
        </SettingsRow>

        {/* Default Retention */}
        <SettingsRow
          label={t("settings:defaultRetention")}
          description={t("settings:defaultRetentionDesc")}
        >
          <Select
            value={hoursToOption(config.default_retention)}
            onValueChange={(v) => saveConfig({ default_retention: optionToHours(v) })}
            disabled={!config.enabled}
          >
            <SelectTrigger className="w-full sm:w-[180px]">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              {retentionOptions.map((opt) => (
                <SelectItem key={opt.value} value={opt.value}>
                  {t(opt.labelKey)}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        </SettingsRow>

        {/* Image Retention */}
        <SettingsRow
          label={t("settings:imageRetention")}
          description={t("settings:imageRetentionDesc")}
          leadingIcon={<SwitchCamera className="h-4 w-4 text-muted-foreground" />}
        >
          <Select
            value={hoursToOption(config.image_retention)}
            onValueChange={(v) => saveConfig({ image_retention: optionToHours(v) })}
            disabled={!config.enabled}
          >
            <SelectTrigger className="w-full sm:w-[180px]">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              {retentionOptions.map((opt) => (
                <SelectItem key={opt.value} value={opt.value}>
                  {t(opt.labelKey)}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        </SettingsRow>

        {/* Manual Cleanup */}
        <div className="pt-3">
          <Button
            variant="outline"
            size="sm"
            onClick={handleCleanup}
            disabled={cleaning}
          >
            {cleaning ? (
              <Loader2 className="h-4 w-4 mr-2 animate-spin" />
            ) : (
              <Database className="h-4 w-4 mr-2" />
            )}
            {cleaning ? t("settings:cleanupRunning") : t("settings:cleanupNow")}
          </Button>
        </div>
      </div>
    </section>
  )
}

function DiagnosticDataSection() {
  const { t } = useTranslation(["common", "settings"])
  const { handleError, showSuccess } = useErrorHandler()
  const [downloading, setDownloading] = useState(false)
  /**
   * Time-range filter for log download. Default "7" matches the Tauri
   * shell's 7-day log retention (`cleanup_old_logs` in main.rs) — i.e. all
   * logs the app actually keeps on disk.
   */
  const [logDays, setLogDays] = useState<string>("7")

  const handleDownload = async () => {
    setDownloading(true)
    try {
      const days = Number.parseInt(logDays, 10) || 0
      const { filename } = await api.downloadLogs(days > 0 ? days : undefined)
      showSuccess(t("settings:downloadLogsSuccess", { filename }))
    } catch (error) {
      handleError(error, { operation: "Download diagnostic logs" })
    } finally {
      setDownloading(false)
    }
  }

  return (
    <section>
      <h3 className="text-xs font-semibold uppercase tracking-wider text-muted-foreground mb-2">
        {t("settings:diagnosticData")}
      </h3>
      <div className="rounded-lg bg-card border border-border shadow-sm p-5 space-y-4">
        <SettingsRow
          label={t("settings:logTimeRange")}
          description={t("settings:diagnosticDataDesc")}
        >
          <Select value={logDays} onValueChange={setLogDays}>
            <SelectTrigger className="w-full sm:w-[180px]">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              <SelectItem value="1">{t("settings:logRangeToday")}</SelectItem>
              <SelectItem value="3">{t("settings:logRangeLast3Days")}</SelectItem>
              <SelectItem value="7">{t("settings:logRangeLast7Days")}</SelectItem>
            </SelectContent>
          </Select>
        </SettingsRow>
        <div className="pt-3">
          <Button
            variant="outline"
            size="sm"
            onClick={handleDownload}
            disabled={downloading}
          >
            {downloading ? (
              <Loader2 className="h-4 w-4 mr-2 animate-spin" />
            ) : (
              <Download className="h-4 w-4 mr-2" />
            )}
            {downloading ? t("settings:downloadingLogs") : t("settings:downloadLogs")}
          </Button>
        </div>
      </div>
    </section>
  )
}

// Format time in a specific timezone (IANA format like "Asia/Shanghai")
function formatTimeInTimezone(timezone: string, format: TimeFormat = "24h"): string {
  try {
    const now = new Date()
    const formatter = new Intl.DateTimeFormat("zh-CN", {
      hour: "2-digit",
      minute: "2-digit",
      second: "2-digit",
      timeZone: timezone,
      hour12: format === "12h",
    })
    return formatter.format(now)
  } catch {
    return new Date().toLocaleTimeString()
  }
}

// Export hook for using preferences
export function usePreferences() {
  const [preferences, setPreferences] = useState<Preferences>(loadPreferences)

  const updatePreferences = (updates: Partial<Preferences>) => {
    const newPrefs = { ...preferences, ...updates }
    setPreferences(newPrefs)
    savePreferences(newPrefs)
  }

  return { preferences, updatePreferences }
}

function BackupSettingsSection() {
  const { t } = useTranslation(["common", "settings"])
  const { toast } = useToast()
  const isAdmin = useStore((s) => s.user?.role === "admin")
  const [config, setConfig] = useState<{
    enabled: boolean
    interval_secs: number
    keep: number
  } | null>(null)
  const [loading, setLoading] = useState(true)
  const [backing, setBacking] = useState(false)
  const [lastBackup, setLastBackup] = useState<{
    id: string
    created_at: string
    total_bytes: number
  } | null>(null)

  const refreshLastBackup = () => {
    api
      .get("/settings/backups")
      .then((data: any) => {
        const list = data?.backups ?? []
        setLastBackup(list.length > 0 ? list[0] : null)
      })
      .catch(() => {})
  }

  useEffect(() => {
    api
      .get("/settings/backup-config")
      .then((data: any) => setConfig(data))
      .catch(() => {})
      .finally(() => setLoading(false))
    refreshLastBackup()
  }, [])

  const saveConfig = async (updates: Partial<NonNullable<typeof config>>) => {
    if (!config) return
    const next = { ...config, ...updates }
    setConfig(next)
    try {
      await api.put("/settings/backup-config", next)
    } catch {
      toast({ title: t("common:failed"), variant: "destructive" })
      setConfig(config)
    }
  }

  const runNow = async () => {
    setBacking(true)
    try {
      await api.post("/settings/backup", {})
      toast({ title: t("settings:backupNowDone") })
      refreshLastBackup()
    } catch {
      toast({ title: t("common:failed"), variant: "destructive" })
    } finally {
      setBacking(false)
    }
  }

  if (loading || !config) {
    return <div className="h-32 w-full animate-pulse rounded-md bg-muted" />
  }

  const intervalOpts = [
    { v: 6 * 3600, l: t("settings:backupInterval6h") },
    { v: 12 * 3600, l: t("settings:backupInterval12h") },
    { v: 24 * 3600, l: t("settings:backupInterval1d") },
    { v: 48 * 3600, l: t("settings:backupInterval2d") },
    { v: 7 * 24 * 3600, l: t("settings:backupInterval7d") },
  ]
  const keepOpts = [3, 5, 7, 10, 14]
  const lastLabel = lastBackup
    ? `${new Date(lastBackup.created_at).toLocaleString()} (${(lastBackup.total_bytes / 1024 / 1024).toFixed(1)} MB)`
    : t("settings:backupLastNone")

  return (
    <section>
      <h3 className="text-xs font-semibold uppercase tracking-wider text-muted-foreground mb-2">
        {t("settings:backupSchedule")}
      </h3>
      <div className="rounded-lg bg-card border border-border shadow-sm p-5 space-y-4">
        <SettingsRow
          label={t("settings:backupEnabled")}
          description={t("settings:backupEnabledDesc")}
        >
          <Switch
            checked={config.enabled}
            onCheckedChange={(v) => saveConfig({ enabled: v })}
          />
        </SettingsRow>
        <SettingsRow
          label={t("settings:backupInterval")}
          description={t("settings:backupIntervalDesc")}
        >
          <Select
            value={String(config.interval_secs)}
            onValueChange={(v) => saveConfig({ interval_secs: +v })}
            disabled={!config.enabled}
          >
            <SelectTrigger className="w-full sm:w-[180px]">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              {intervalOpts.map((o) => (
                <SelectItem key={o.v} value={String(o.v)}>
                  {o.l}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        </SettingsRow>
        <SettingsRow label={t("settings:backupKeep")} description={t("settings:backupKeepDesc")}>
          <Select
            value={String(config.keep)}
            onValueChange={(v) => saveConfig({ keep: +v })}
            disabled={!config.enabled}
          >
            <SelectTrigger className="w-full sm:w-[180px]">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              {keepOpts.map((k) => (
                <SelectItem key={k} value={String(k)}>
                  {k}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        </SettingsRow>
        <div className="flex flex-col sm:flex-row sm:items-center justify-between gap-3 pt-1">
          <span className="text-sm text-muted-foreground">
            {t("settings:backupLast")}: {lastLabel}
          </span>
          {isAdmin && (
            <Button variant="outline" size="sm" onClick={runNow} disabled={backing}>
              {backing ? <Loader2 className="h-4 w-4 animate-spin" /> : <Database className="h-4 w-4" />}
              {t("settings:backupNow")}
            </Button>
          )}
        </div>
      </div>
    </section>
  )
}
