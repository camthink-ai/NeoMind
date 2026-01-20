import { useState } from "react"
import { useTranslation } from "react-i18next"
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card"
import { Label } from "@/components/ui/label"
import { Button } from "@/components/ui/button"
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select"
import {
  Globe,
  Clock,
  Check,
  Info,
} from "lucide-react"
import { useToast } from "@/hooks/use-toast"

type Language = "zh" | "en"
type TimeFormat = "12h" | "24h"
type TimeZone = "local" | "utc"

interface Preferences {
  language: Language
  timeFormat: TimeFormat
  timeZone: TimeZone
}

const PREFERENCES_KEY = "neotalk_preferences"

// Default preferences
const defaultPreferences: Preferences = {
  language: "zh",
  timeFormat: "24h",
  timeZone: "local",
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
    console.error("Failed to load preferences:", e)
  }
  return defaultPreferences
}

// Save preferences to localStorage
function savePreferences(pref: Preferences) {
  try {
    localStorage.setItem(PREFERENCES_KEY, JSON.stringify(pref))
  } catch (e) {
    console.error("Failed to save preferences:", e)
  }
}

export function PreferencesTab() {
  const { t, i18n } = useTranslation(["common", "settings"])
  const { toast } = useToast()
  const [preferences, setPreferences] = useState<Preferences>(loadPreferences)
  const [hasChanges, setHasChanges] = useState(false)

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
    { value: "12h" as TimeFormat, label: "12小时制 (12:00 PM)" },
    { value: "24h" as TimeFormat, label: "24小时制 (14:00)" },
  ]

  const timeZoneOptions = [
    { value: "local" as TimeZone, label: t("settings:localTime") },
    { value: "utc" as TimeZone, label: "UTC (Coordinated Universal Time)" },
  ]

  return (
    <div className="space-y-6">
      {/* Header */}
      <div className="space-y-1">
        <h2 className="text-2xl font-bold">
          {t("settings:preferences")}
        </h2>
        <p className="text-muted-foreground text-sm">
          {t("settings:preferencesDesc")}
        </p>
      </div>

      {/* Actions */}
      {hasChanges && (
        <div className="flex items-center justify-between p-4 bg-muted/50 rounded-lg">
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
            <Globe className="h-5 w-5 text-blue-500" />
            {t("settings:languageRegion")}
          </CardTitle>
        </CardHeader>
        <CardContent className="space-y-4">
          {/* Language */}
          <div className="flex items-center justify-between">
            <div>
              <Label className="text-sm font-medium">
                {t("settings:language")}
              </Label>
              <p className="text-xs text-muted-foreground mt-0.5">
                {t("settings:languageDesc")}
              </p>
            </div>
            <Select
              value={preferences.language}
              onValueChange={(v) => updatePreference("language", v as Language)}
            >
              <SelectTrigger className="w-[180px]">
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
          </div>
        </CardContent>
      </Card>

      {/* Time Settings */}
      <Card>
        <CardHeader>
          <CardTitle className="flex items-center gap-2">
            <Clock className="h-5 w-5 text-green-500" />
            {t("settings:timeSettings")}
          </CardTitle>
        </CardHeader>
        <CardContent className="space-y-4">
          {/* Time Format */}
          <div className="flex items-center justify-between">
            <div>
              <Label className="text-sm font-medium">
                {t("settings:timeFormat")}
              </Label>
              <p className="text-xs text-muted-foreground mt-0.5">
                {t("settings:timeFormatDesc")}
              </p>
            </div>
            <Select
              value={preferences.timeFormat}
              onValueChange={(v) => updatePreference("timeFormat", v as TimeFormat)}
            >
              <SelectTrigger className="w-[200px]">
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
          </div>

          {/* Time Zone */}
          <div className="flex items-center justify-between">
            <div>
              <Label className="text-sm font-medium">
                {t("settings:timeZone")}
              </Label>
              <p className="text-xs text-muted-foreground mt-0.5">
                {t("settings:timeZoneDesc")}
              </p>
            </div>
            <Select
              value={preferences.timeZone}
              onValueChange={(v) => updatePreference("timeZone", v as TimeZone)}
            >
              <SelectTrigger className="w-[200px]">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                {timeZoneOptions.map((option) => (
                  <SelectItem key={option.value} value={option.value}>
                    {option.label}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </div>

          {/* Current Time Preview */}
          <div className="pt-4 border-t">
            <div className="text-center p-4 bg-muted/50 rounded-lg">
              <div className="text-xs text-muted-foreground mb-1">
                {t("settings:currentTime")}
              </div>
              <div className="text-2xl font-mono font-medium">
                {formatTime(new Date(), preferences.timeFormat, preferences.timeZone)}
              </div>
            </div>
          </div>
        </CardContent>
      </Card>

      {/* Info */}
      <div className="text-sm text-muted-foreground text-center py-4">
        <p>{t("settings:preferencesInfo")}</p>
      </div>
    </div>
  )
}

// Format time based on preferences
function formatTime(date: Date, format: TimeFormat, timeZone: TimeZone): string {
  let displayDate = date

  // Convert to UTC if needed
  if (timeZone === "utc") {
    displayDate = new Date(date.toUTCString())
  }

  const hours = displayDate.getHours()
  const minutes = displayDate.getMinutes().toString().padStart(2, "0")

  if (format === "12h") {
    const period = hours >= 12 ? "PM" : "AM"
    const displayHours = hours % 12 || 12
    return `${displayHours}:${minutes} ${period}`
  }

  return `${hours.toString().padStart(2, "0")}:${minutes}`
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
