import { useState, useEffect } from "react"
import { useTranslation } from "react-i18next"
import { useErrorHandler } from "@/hooks/useErrorHandler"
import { logError } from "@/lib/errors"
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card"
import { SettingsRow } from "./SettingsRow"
import { Button } from "@/components/ui/button"
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select"
import {
  Clock,
  Check,
  Info,
  Loader2,
  Globe,
  Database,
  SwitchCamera,
  ScrollText,
  Download,
} from "lucide-react"
import { Switch } from "@/components/ui/switch"
import { useToast } from "@/hooks/use-toast"
import { api } from "@/lib/api"
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
    <div className="space-y-6">
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
      <Card>
        <CardHeader>
          <CardTitle className="flex items-center gap-2">
            <Globe className="h-5 w-5 text-info" />
            {t("settings:languageRegion")}
          </CardTitle>
        </CardHeader>
        <CardContent className="space-y-4">
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
        </CardContent>
      </Card>

      {/* Time Settings */}
      <Card>
        <CardHeader>
          <CardTitle className="flex items-center gap-2">
            <Clock className="h-5 w-5 text-success" />
            {t("settings:timeSettings")}
          </CardTitle>
        </CardHeader>
        <CardContent className="space-y-4">
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
          <div className="pt-4 border-t">
            <div className="text-center p-4 bg-muted-50 rounded-lg">
              <div className="text-xs text-muted-foreground mb-1">
                {t("settings:currentTime")}
              </div>
              <div className="text-2xl font-mono font-medium">
                {formatTimeInTimezone(globalTimezone, preferences.timeFormat)}
              </div>
            </div>
          </div>
        </CardContent>
      </Card>

      {/* Data Management */}
      <DataManagementCard />

      {/* Diagnostic Data — log archive download */}
      <DiagnosticDataCard />

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

function DataManagementCard() {
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
    return (
      <Card>
        <CardHeader>
          <CardTitle className="flex items-center gap-2">
            <Database className="h-5 w-5 text-accent-orange" />
            {t("settings:dataManagement")}
          </CardTitle>
        </CardHeader>
        <CardContent>
          <div className="flex items-center justify-center py-6">
            <Loader2 className="h-5 w-5 animate-spin text-muted-foreground" />
          </div>
        </CardContent>
      </Card>
    )
  }

  if (!config) return null

  return (
    <Card>
      <CardHeader>
        <CardTitle className="flex items-center gap-2">
          <Database className="h-5 w-5 text-accent-orange" />
          {t("settings:dataManagement")}
        </CardTitle>
      </CardHeader>
      <CardContent className="space-y-5">
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
        <div className="pt-4 border-t">
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
      </CardContent>
    </Card>
  )
}

function DiagnosticDataCard() {
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
    <Card>
      <CardHeader>
        <CardTitle className="flex items-center gap-2">
          <ScrollText className="h-5 w-5 text-info" />
          {t("settings:diagnosticData")}
        </CardTitle>
      </CardHeader>
      <CardContent className="space-y-4">
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
        <div className="pt-4 border-t">
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
      </CardContent>
    </Card>
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
