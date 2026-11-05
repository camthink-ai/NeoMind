/**
 * React hook for time formatting with preferences
 */

import { useState, useEffect } from "react"
import {
  loadTimePreferences,
  formatTime,
  formatTimeShort,
  formatDateTime,
  formatDate,
  formatTimeWithTimezone,
  formatCurrentTimeInTimezone,
  formatRelativeTime,
  getCurrentTimeInfo,
  type TimePreferences,
} from "@/lib/time"

export interface UseTimeFormatReturn {
  preferences: TimePreferences
  formatTime: (date: Date) => string
  formatTimeShort: (date: Date) => string
  formatDateTime: (date: Date) => string
  formatDate: (date: Date) => string
  formatRelativeTime: (timestamp: number) => string
  getCurrentTimeInfo: (timezone?: string) => ReturnType<typeof getCurrentTimeInfo>
  formatTimeWithTimezone: (date: Date, timezone: string) => string
  formatCurrentTimeInTimezone: (timezone: string) => string
  refresh: () => void
}

/**
 * Hook for time formatting with user preferences
 *
 * Automatically updates when preferences change in localStorage
 */
export function useTimeFormat(): UseTimeFormatReturn {
  const [preferences, setPreferences] = useState<TimePreferences>(
    loadTimePreferences()
  )

  // Refresh preferences when storage changes
  const refresh = () => {
    setPreferences(loadTimePreferences())
  }

  // Listen for storage changes from other tabs
  useEffect(() => {
    const handleStorageChange = (e: StorageEvent) => {
      if (e.key === "neomind_preferences" && e.newValue) {
        refresh()
      }
    }

    window.addEventListener("storage", handleStorageChange)
    return () => window.removeEventListener("storage", handleStorageChange)
  }, [])

  return {
    preferences,
    formatTime: (date: Date) => formatTime(date, preferences),
    formatTimeShort: (date: Date) => formatTimeShort(date, preferences),
    formatDateTime: (date: Date) => formatDateTime(date, preferences),
    formatDate,
    formatRelativeTime,
    getCurrentTimeInfo,
    formatTimeWithTimezone: (date: Date, timezone: string) =>
      formatTimeWithTimezone(date, timezone, preferences),
    formatCurrentTimeInTimezone: (timezone: string) =>
      formatCurrentTimeInTimezone(timezone, preferences),
    refresh,
  }
}

/**
 * Hook for global timezone setting (system-wide, not just display)
 * This is used for agent scheduling and server-side operations
 */
export interface UseGlobalTimezoneReturn {
  timezone: string
  isLoading: boolean
  isDefault: boolean
  updateTimezone: (timezone: string) => Promise<void>
  availableTimezones: Array<{ id: string; name: string }>
  refresh: () => Promise<void>
  error: string | null
}

export function useGlobalTimezone(): UseGlobalTimezoneReturn {
  const [timezone, setTimezone] = useState<string>("Asia/Shanghai")
  const [isDefault, setIsDefault] = useState(true)
  const [isLoading, setIsLoading] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [availableTimezones, setAvailableTimezones] = useState<
    Array<{ id: string; name: string }>
  >([])

  const refresh = async () => {
    setIsLoading(true)
    setError(null)

    try {
      const { api } = await import("@/lib/api")

      // Fetch current timezone and available timezones in parallel
      const [tzResponse, timezonesResponse] = await Promise.all([
        api.getTimezone(),
        api.listTimezones().catch(() => ({ timezones: [] })),
      ])

      setTimezone(tzResponse.timezone || "Asia/Shanghai")
      setIsDefault(tzResponse.is_default ?? true)
      setAvailableTimezones(Array.isArray(timezonesResponse.timezones) ? timezonesResponse.timezones : [])
    } catch (e) {
      setError(e instanceof Error ? e.message : "Failed to load timezone settings")
    } finally {
      setIsLoading(false)
    }
  }

  const updateTimezone = async (newTimezone: string) => {
    setIsLoading(true)
    setError(null)

    try {
      const { api } = await import("@/lib/api")
      const result = await api.updateTimezone(newTimezone)

      setTimezone(result.timezone)
      setIsDefault(result.timezone === "Asia/Shanghai")
    } catch (e) {
      setError(e instanceof Error ? e.message : "Failed to update timezone")
      throw e
    } finally {
      setIsLoading(false)
    }
  }

  useEffect(() => {
    refresh()
  }, [])

  return {
    timezone,
    isLoading,
    isDefault,
    updateTimezone,
    availableTimezones,
    refresh,
    error,
  }
}
