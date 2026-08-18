/**
 * WelcomeArea - Clean welcome area shown when no active conversation
 * Shows greeting, system status summary, and suggested prompts
 */

import { useEffect, useState } from "react"
import { useNavigate } from "react-router-dom"
import { useTranslation } from "react-i18next"
import { api } from "@/lib/api"
import { cn } from "@/lib/utils"
import { useErrorHandler } from "@/hooks/useErrorHandler"
import { MessageSquare, Sparkles, ArrowRight, Bell } from "lucide-react"
import { useBrandMessages } from "@/hooks/useBrand"

interface WelcomeAreaProps {
  className?: string
  onQuickAction?: (prompt: string) => void
}

interface SystemStats {
  devicesOnline: number
  devicesTotal: number
  activeRules: number
  pendingAlerts: number
}

export function WelcomeArea({ className, onQuickAction }: WelcomeAreaProps) {
  const { t } = useTranslation("common")
  const navigate = useNavigate()
  const { getWelcomeMessage } = useBrandMessages()
  const { handleError } = useErrorHandler()

  const [stats, setStats] = useState<SystemStats | null>(null)

  // Fetch system stats using dedicated stats APIs
  useEffect(() => {
    const fetchStats = async () => {
      try {
        const results = await Promise.allSettled([
          api.getRuleStats(),
          api.getMessageStats(),
        ])

        const ruleStats = results[0].status === 'fulfilled' ? results[0].value : null
        const msgStats = results[1].status === 'fulfilled' ? results[1].value : null

        setStats({
          devicesOnline: 0,
          devicesTotal: 0,
          activeRules: ruleStats?.stats?.enabled_rules ?? 0,
          pendingAlerts: msgStats?.active ?? 0,
        })
      } catch (error) {
        handleError(error, { operation: 'Fetch welcome stats', showToast: false })
      }
    }

    fetchStats()
  }, [handleError])

  // Get greeting based on time
  const getGreetingKey = () => {
    const hour = new Date().getHours()
    if (hour < 6) return "welcome.greeting.earlyMorning"
    if (hour < 12) return "welcome.greeting.morning"
    if (hour < 18) return "welcome.greeting.afternoon"
    return "welcome.greeting.evening"
  }

  // Prompt suggestions
  const promptSuggestions = [
    t("welcome.suggestionPrompts.checkDevices"),
    t("welcome.suggestionPrompts.createRule"),
    t("welcome.suggestionPrompts.checkAlerts"),
    t("welcome.suggestionPrompts.showLogs"),
  ]

  return (
    <div className={cn("flex min-h-full w-full flex-col items-center p-6", className)}>
      {/* Top spacer */}
      <div className="min-h-0 flex-1 shrink" />
      <div className="w-full max-w-2xl shrink-0 space-y-8">
        {/* Greeting */}
        <div className="text-center">
          <div className="flex items-center justify-center gap-2.5 mb-3">
            <div className="w-10 h-10 rounded-xl bg-primary-light flex items-center justify-center">
              <MessageSquare className="h-5 w-5 text-primary" />
            </div>
          </div>
          <h1 className="text-2xl font-semibold text-foreground mb-2">
            {t(getGreetingKey())}
          </h1>
          <p className="text-muted-foreground">
            {getWelcomeMessage("tagline")}
          </p>
        </div>

        {/* Status pills */}
        {stats && (
          <div className="flex items-center justify-center gap-3 flex-wrap">
            {stats.activeRules > 0 && (
              <span className="inline-flex items-center gap-1.5 px-3 py-1.5 rounded-full bg-muted text-xs text-muted-foreground">
                <Sparkles className="h-3 w-3" />
                {stats.activeRules} {t("common.active", { defaultValue: "Active" })}
              </span>
            )}
            {stats.pendingAlerts > 0 && (
              <button
                type="button"
                onClick={() => navigate("/messages")}
                className="inline-flex items-center gap-1.5 px-3 py-1.5 rounded-full bg-warning-light text-xs text-warning hover:bg-warning hover:text-primary-foreground transition-colors"
                aria-label={t("welcome.stats.pendingAlertsShort", { defaultValue: "Alerts" })}
              >
                <Bell className="h-3 w-3" aria-hidden="true" />
                {stats.pendingAlerts} {t("welcome.stats.pendingAlertsShort", { defaultValue: "Alerts" })}
              </button>
            )}
          </div>
        )}

        {/* Prompt suggestions */}
        <div className="space-y-3">
          <div className="flex items-center justify-center gap-2 text-xs text-muted-foreground">
            <Sparkles className="h-3.5 w-3.5" />
            <span>{t("welcome.suggestionPrompts.title")}</span>
          </div>
          <div className="grid grid-cols-1 sm:grid-cols-2 gap-2">
            {promptSuggestions.map((prompt) => (
              <button
                key={prompt}
                onClick={() => onQuickAction?.(prompt)}
                className="group px-4 py-3 rounded-lg bg-muted hover:bg-border text-left text-sm text-foreground transition-colors flex items-center justify-between gap-2"
              >
                <span className="truncate">{prompt}</span>
                <ArrowRight className="h-4 w-4 text-muted-foreground opacity-0 group-hover:opacity-100 transition-opacity shrink-0" />
              </button>
            ))}
          </div>
        </div>
      </div>
      {/* Bottom spacer */}
      <div className="min-h-0 flex-1 shrink" />
    </div>
  )
}
