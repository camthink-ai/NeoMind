/**
 * WelcomeArea - Smart welcome area shown when no active conversation
 * Contains status cards, AI suggestions, and quick actions
 * Fully internationalized with i18n
 */

import { useEffect, useState } from "react"
import { useTranslation } from "react-i18next"
import { useStore } from "@/store"
import { fetchAPI } from "@/lib/api"
import { cn } from "@/lib/utils"
import {
  Sparkles,
  Cpu,
  Workflow,
  Bell,
  Settings,
  ArrowRight,
  Lightbulb,
  Bot,
} from "lucide-react"
import { Button } from "@/components/ui/button"
import { Link } from "react-router-dom"

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

interface AISuggestion {
  id: string
  text: string
  prompt: string
  priority: "high" | "medium" | "low"
}

export function WelcomeArea({ className, onQuickAction }: WelcomeAreaProps) {
  const { t } = useTranslation("common")
  const user = useStore((state) => state.user)

  const [stats, setStats] = useState<SystemStats | null>(null)
  const [suggestions, setSuggestions] = useState<AISuggestion[]>([])
  const [isLoadingStats, setIsLoadingStats] = useState(true)

  // Fetch system stats
  useEffect(() => {
    const fetchStats = async () => {
      try {
        // Fetch device stats
        const devicesData = await fetchAPI<{ devices: any[] }>("/devices")
        const devices = devicesData.devices || []
        const onlineDevices = devices.filter((d: any) => d.is_online).length

        // Fetch rules stats
        const rulesData = await fetchAPI<{ rules: any[] }>("/rules")
        const rules = rulesData.rules || []
        const activeRules = rules.filter((r: any) => r.enabled).length

        // Fetch alerts stats
        const alertsData = await fetchAPI<{ alerts: any[] }>("/alerts")
        const alerts = alertsData.alerts || []
        const pendingAlerts = alerts.filter((a: any) => !a.resolved).length

        setStats({
          devicesOnline: onlineDevices,
          devicesTotal: devices.length,
          activeRules: activeRules,
          pendingAlerts: pendingAlerts,
        })

        // Generate AI suggestions based on stats
        const newSuggestions: AISuggestion[] = []

        if (pendingAlerts > 0) {
          newSuggestions.push({
            id: "alerts",
            text: t("welcome.suggestions.pendingAlerts", { count: pendingAlerts }),
            prompt: "显示所有未处理的告警",
            priority: "high",
          })
        }

        const offlineDevices = devices.length - onlineDevices
        if (offlineDevices > 0) {
          newSuggestions.push({
            id: "offline",
            text: t("welcome.suggestions.offlineDevices", { count: offlineDevices }),
            prompt: "显示所有离线设备的详情",
            priority: offlineDevices > 3 ? "high" : "medium",
          })
        }

        if (activeRules === 0 && devices.length > 0) {
          newSuggestions.push({
            id: "rules",
            text: t("welcome.suggestions.noRules"),
            prompt: "帮我创建第一个自动化规则",
            priority: "low",
          })
        }

        setSuggestions(newSuggestions)
      } catch (error) {
        console.error("Failed to fetch stats:", error)
      } finally {
        setIsLoadingStats(false)
      }
    }

    fetchStats()
  }, [t])

  // Get greeting based on time
  const getGreetingKey = () => {
    const hour = new Date().getHours()
    if (hour < 6) return "welcome.greeting.earlyMorning"
    if (hour < 12) return "welcome.greeting.morning"
    if (hour < 18) return "welcome.greeting.afternoon"
    return "welcome.greeting.evening"
  }

  // Quick action buttons
  const quickActions = [
    { id: "devices", label: t("welcome.quickActions.devices"), icon: Cpu, path: "/devices" },
    { id: "automation", label: t("welcome.quickActions.automation"), icon: Workflow, path: "/automation" },
    { id: "agents", label: t("nav.agents"), icon: Bot, path: "/agents" },
    { id: "settings", label: t("welcome.quickActions.settings"), icon: Settings, path: "/settings" },
  ]

  // Prompt suggestions
  const promptSuggestions = [
    t("welcome.suggestionPrompts.checkDevices"),
    t("welcome.suggestionPrompts.createRule"),
    t("welcome.suggestionPrompts.checkAlerts"),
    t("welcome.suggestionPrompts.showLogs"),
  ]

  return (
    <div className={cn("flex-1 flex flex-col items-center justify-center p-6", className)}>
      <div className="w-full max-w-2xl space-y-8">
        {/* Logo and greeting */}
        <div className="text-center animate-fade-in-up">
          <div className="w-16 h-16 rounded-2xl bg-foreground flex items-center justify-center mx-auto mb-6">
            <Sparkles className="h-8 w-8 text-background" />
          </div>
          <h1 className="text-2xl font-semibold text-foreground mb-2">
            {t(getGreetingKey())}, {user?.username || t("common.user", { defaultValue: "User" })}
          </h1>
          <p className="text-muted-foreground">
            {t("welcome.tagline")}
          </p>
        </div>

        {/* Status cards */}
        {stats && (
          <div className="grid grid-cols-1 sm:grid-cols-3 gap-3 animate-fade-in-up delay-100">
            <Link
              to="/devices"
              className="p-4 rounded-xl bg-muted/50 hover:bg-muted transition-colors text-left group"
            >
              <div className="flex items-center justify-between mb-2">
                <Cpu className="h-5 w-5 text-muted-foreground group-hover:text-foreground transition-colors" />
                <ArrowRight className="h-4 w-4 text-muted-foreground opacity-0 group-hover:opacity-100 transition-opacity" />
              </div>
              <div className="text-2xl font-semibold text-foreground">
                {stats.devicesOnline}
                <span className="text-sm font-normal text-muted-foreground">/{stats.devicesTotal}</span>
              </div>
              <div className="text-xs text-muted-foreground">
                {t("welcome.stats.devicesOnline", {
                  online: stats.devicesOnline,
                  total: stats.devicesTotal
                }).split(" ").slice(1).join(" ")}
              </div>
            </Link>

            <Link
              to="/automation"
              className="p-4 rounded-xl bg-muted/50 hover:bg-muted transition-colors text-left group"
            >
              <div className="flex items-center justify-between mb-2">
                <Workflow className="h-5 w-5 text-muted-foreground group-hover:text-foreground transition-colors" />
                <ArrowRight className="h-4 w-4 text-muted-foreground opacity-0 group-hover:opacity-100 transition-opacity" />
              </div>
              <div className="text-2xl font-semibold text-foreground">{stats.activeRules}</div>
              <div className="text-xs text-muted-foreground">
                {t("common.active", { defaultValue: "Active" })}
              </div>
            </Link>

            <Link
              to="/settings"
              className="p-4 rounded-xl bg-muted/50 hover:bg-muted transition-colors text-left group"
            >
              <div className="flex items-center justify-between mb-2">
                <Bell className="h-5 w-5 text-muted-foreground group-hover:text-foreground transition-colors" />
                <ArrowRight className="h-4 w-4 text-muted-foreground opacity-0 group-hover:opacity-100 transition-opacity" />
              </div>
              <div className="text-2xl font-semibold text-foreground">{stats.pendingAlerts}</div>
              <div className="text-xs text-muted-foreground">
                {t("welcome.stats.pendingAlerts", { count: stats.pendingAlerts }).split(" ").slice(1).join(" ")}
              </div>
            </Link>
          </div>
        )}

        {/* Loading skeleton for stats */}
        {isLoadingStats && (
          <div className="grid grid-cols-1 sm:grid-cols-3 gap-3">
            {[1, 2, 3].map((i) => (
              <div key={i} className="p-4 rounded-xl bg-muted/30 animate-pulse">
                <div className="h-5 w-5 rounded bg-muted mb-2" />
                <div className="h-8 w-12 rounded bg-muted mb-1" />
                <div className="h-3 w-16 rounded bg-muted" />
              </div>
            ))}
          </div>
        )}

        {/* AI Suggestions */}
        {suggestions.length > 0 && (
          <div className="space-y-2 animate-fade-in-up delay-200">
            <div className="flex items-center gap-2 text-sm text-muted-foreground">
              <Lightbulb className="h-4 w-4" />
              <span>{t("welcome.aiSuggestion")}</span>
            </div>
            <div className="space-y-2">
              {suggestions.map((suggestion) => (
                <button
                  key={suggestion.id}
                  onClick={() => onQuickAction?.(suggestion.prompt)}
                  className={cn(
                    "w-full p-3 rounded-xl text-left transition-colors flex items-center justify-between group",
                    suggestion.priority === "high"
                      ? "bg-muted/70 hover:bg-muted"
                      : "bg-muted/30 hover:bg-muted/50"
                  )}
                >
                  <span className="text-sm text-foreground">{suggestion.text}</span>
                  <ArrowRight className="h-4 w-4 text-muted-foreground opacity-0 group-hover:opacity-100 transition-opacity" />
                </button>
              ))}
            </div>
          </div>
        )}

        {/* Prompt suggestions */}
        <div className="space-y-3 animate-fade-in-up delay-300">
          <p className="text-sm text-muted-foreground text-center">
            {t("welcome.suggestionPrompts.title")}
          </p>
          <div className="flex flex-wrap justify-center gap-2">
            {promptSuggestions.map((prompt) => (
              <button
                key={prompt}
                onClick={() => onQuickAction?.(prompt)}
                className="px-4 py-2 rounded-xl bg-muted/30 hover:bg-muted text-sm text-foreground transition-colors"
              >
                {prompt}
              </button>
            ))}
          </div>
        </div>

        {/* Quick actions */}
        <div className="flex justify-center gap-2 animate-fade-in-up delay-400">
          {quickActions.map((action) => {
            const Icon = action.icon
            return (
              <Button
                key={action.id}
                variant="ghost"
                asChild
                className="flex-col h-auto py-3 px-4 rounded-xl hover:bg-muted"
              >
                <Link to={action.path}>
                  <Icon className="h-5 w-5 mb-1 text-muted-foreground" />
                  <span className="text-xs text-muted-foreground">{action.label}</span>
                </Link>
              </Button>
            )
          })}
        </div>
      </div>
    </div>
  )
}
