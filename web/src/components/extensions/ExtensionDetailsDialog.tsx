import { useState, useEffect, useCallback, useRef, useMemo } from "react"
import { useTranslation } from "react-i18next"
import { Button } from "@/components/ui/button"
import { Input } from "@/components/ui/input"
import { PasswordInput } from "@/components/ui/password-input"
import { Label } from "@/components/ui/label"
import { Badge } from "@/components/ui/badge"
import { SegmentedControl } from "@/components/ui/segmented-control"
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select"
import { Switch } from "@/components/ui/switch"
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
} from "@/components/ui/alert-dialog"
import {
  FullScreenDialog,
  FullScreenDialogHeader,
  FullScreenDialogContent,
  FullScreenDialogSidebar,
  FullScreenDialogMain,
  FullScreenDialogFooter,
} from "@/components/automation/dialog/FullScreenDialog"
import {
  Settings,
  Info,
  Loader2,
  Terminal,
  Zap,
  Save,
  Activity,
  ChevronDown,
  Play,
  Database,
  FileText,
  Copy,
  RefreshCw,
  CheckCircle2,
  XCircle,
  AlertTriangle,
} from "lucide-react"
import { useErrorHandler } from "@/hooks/useErrorHandler"
import { useStore } from "@/store"
import { formatTimestamp } from "@/lib/utils/format"
import { copyToClipboard } from "@/lib/clipboard"
import { useToast } from "@/hooks/use-toast"
import { api } from "@/lib/api"
import type { Extension, ExtensionConfigResponse, ExtensionLogEntry } from "@/types"
import { useIsMobile } from "@/hooks/useMobile"
import { cn } from "@/lib/utils"
import { textMini, textNano } from "@/design-system/tokens/typography"
import { Sparkline } from '@/components/dashboard/generic/Sparkline'
import { MobileTabStrip } from '@/components/shared/MobileTabStrip'

interface ExtensionDetailsDialogProps {
  extension: Extension | null
  open: boolean
  onOpenChange: (open: boolean) => void
}

type SectionId = "overview" | "config" | "commands" | "metrics" | "logs"
type LogLevelFilter = "all" | "error" | "warn" | "info"

export function ExtensionDetailsDialog({
  extension,
  open,
  onOpenChange,
}: ExtensionDetailsDialogProps) {
  const { t } = useTranslation(["extensions", "common"])
  const { handleError } = useErrorHandler()
  const { toast } = useToast()
  const getExtensionHealth = useStore((state) => state.getExtensionHealth)
  const fetchExtensions = useStore((state) => state.fetchExtensions)
  const reloadExtensionStore = useStore((state) => state.reloadExtension)
  const executeExtensionCommand = useStore((state) => state.executeExtensionCommand)
  const getExtensionLogs = useStore((state) => state.getExtensionLogs)
  const isMobile = useIsMobile()

  const [health, setHealth] = useState<{ healthy: boolean } | null>(null)
  const [loading, setLoading] = useState(false)

  // Config state
  const [configData, setConfigData] = useState<ExtensionConfigResponse | null>(null)
  const [configLoading, setConfigLoading] = useState(false)
  const [configValues, setConfigValues] = useState<Record<string, unknown>>({})
  const [saving, setSaving] = useState(false)

  // Logs state
  const [logs, setLogs] = useState<ExtensionLogEntry[]>([])
  const [logsLoading, setLogsLoading] = useState(false)
  const [logLevelFilter, setLogLevelFilter] = useState<LogLevelFilter>("all")
  const logListRef = useRef<HTMLDivElement>(null)
  // True while the log list is scrolled to (near) the bottom. Auto-scroll only
  // sticks to bottom when this is true, so scrolling up to read history isn't
  // yanked back down on every 3s poll.
  const pinnedToBottomRef = useRef(true)

  // Section navigation
  const [activeSection, setActiveSection] = useState<SectionId>("overview")

  // Command execution state
  const [expandedCommand, setExpandedCommand] = useState<string | null>(null)
  const [commandArgs, setCommandArgs] = useState<Record<string, Record<string, unknown>>>({})
  const [commandResults, setCommandResults] = useState<Record<string, { success: boolean; result?: unknown; message?: string }>>({})
  const [executingCommand, setExecutingCommand] = useState<string | null>(null)

  // Pending tool-disable confirmation. Toggling OFF needs confirmation since
  // it changes what the agent can do; ON is immediate (safe direction).
  // kind: "master" = whole extension, "command" = single command.
  const [pendingDisable, setPendingDisable] =
    useState<{ kind: "master" | "command"; cmdId?: string } | null>(null)

  // Metric history state
  const [expandedMetric, setExpandedMetric] = useState<string | null>(null)
  const [metricHistoryData, setMetricHistoryData] = useState<Record<string, Array<{ timestamp: number; value: unknown }>>>({})
  const [metricHistoryLoading, setMetricHistoryLoading] = useState<Record<string, boolean>>({})
  const [metricHistoryError, setMetricHistoryError] = useState<Record<string, string | null>>({})
  const [metricTimeRange, setMetricTimeRange] = useState<Record<string, '1h' | '24h' | '7d' | '30d'>>({})

  // Load extension details when dialog opens
  const loadDetails = useCallback(async () => {
    if (!extension) return

    setLoading(true)
    try {
      const healthData = await getExtensionHealth(extension.id)
      setHealth(healthData)
    } catch (error) {
      handleError(error, { operation: "Load extension details", showToast: false })
    } finally {
      setLoading(false)
    }
  }, [extension, getExtensionHealth, handleError])

  // Load config
  const loadConfig = useCallback(async () => {
    if (!extension) return

    setConfigLoading(true)
    try {
      const response = await api.get<ExtensionConfigResponse>(`/extensions/${extension.id}/config`)
      setConfigData(response)

      const saved = response.current_config || {}
      const defaults: Record<string, unknown> = {}
      const props = response.config_schema?.properties || {}
      for (const [key, schema] of Object.entries(props)) {
        if (saved[key] === undefined && (schema as any).default !== undefined) {
          defaults[key] = (schema as any).default
        }
      }
      setConfigValues({ ...defaults, ...saved })
    } catch (error) {
      handleError(error, { operation: "Load extension config", showToast: false })
    } finally {
      setConfigLoading(false)
    }
  }, [extension, handleError])

  // Save config
  const saveConfig = async () => {
    if (!extension) return

    setSaving(true)
    try {
      await api.put(`/extensions/${extension.id}/config`, configValues)
      toast({ title: t("extensions:config.saved", { defaultValue: "Configuration saved" }) })

      const reloadSuccess = await reloadExtensionStore(extension.id)
      if (reloadSuccess) {
        toast({ title: t("extensions:config.reloadedWithConfig", { defaultValue: "Extension reloaded with new configuration" }) })
      } else {
        toast({ title: t("extensions:config.reloadedPartial", { defaultValue: "Extension reloaded, but config may not have been applied" }), variant: "default" })
      }

      await Promise.all([fetchExtensions(), loadConfig()])
    } catch (error) {
      handleError(error, { operation: "Save extension config" })
    } finally {
      setSaving(false)
    }
  }

  // Update config value
  const updateConfigValue = (name: string, value: unknown) => {
    setConfigValues((prev) => ({ ...prev, [name]: value }))
  }

  // Fetch metric history
  const fetchMetricHistory = useCallback(async (metricName: string) => {
    if (!extension) return

    const range = metricTimeRange[metricName] || '24h'
    const now = Math.floor(Date.now() / 1000)
    let start: number
    switch (range) {
      case '1h': start = now - 3600; break
      case '24h': start = now - 86400; break
      case '7d': start = now - 604800; break
      case '30d': start = now - 2592000; break
      default: start = now - 86400
    }

    setMetricHistoryLoading(prev => ({ ...prev, [metricName]: true }))
    setMetricHistoryError(prev => ({ ...prev, [metricName]: null }))

    try {
      const result = await api.getMetricData(extension.id, metricName, { start, end: now, limit: 1000 })
      setMetricHistoryData(prev => ({ ...prev, [metricName]: result.data || [] }))
    } catch (err) {
      setMetricHistoryError(prev => ({
        ...prev,
        [metricName]: t("extensions:metrics.historyError", { defaultValue: "Failed to load metric history" }),
      }))
    } finally {
      setMetricHistoryLoading(prev => ({ ...prev, [metricName]: false }))
    }
  }, [extension, metricTimeRange, t])

  // Load logs (with loading spinner for initial load)
  const loadLogs = useCallback(async () => {
    if (!extension) return
    setLogsLoading(true)
    try {
      const logEntries = await getExtensionLogs(extension.id)
      setLogs(logEntries)
    } catch (error) {
      handleError(error, { operation: "Load extension logs", showToast: false })
    } finally {
      setLogsLoading(false)
    }
  }, [extension, getExtensionLogs, handleError])

  // Silent refresh for auto-polling (no loading state to avoid re-render cascade)
  const silentRefreshLogs = useCallback(async () => {
    if (!extension) return
    try {
      const logEntries = await getExtensionLogs(extension.id)
      setLogs(logEntries)
    } catch {
      // Silent refresh — ignore errors
    }
  }, [extension, getExtensionLogs])

  // Handle section change — lazy load config
  const handleSectionChange = (section: SectionId) => {
    setActiveSection(section)
    if (section === "config" && !configData) {
      loadConfig()
    }
    if (section === "logs") {
      // Re-entering logs: jump to latest on next render.
      pinnedToBottomRef.current = true
      loadLogs()
    }
  }

  // Execute command
  const handleExecuteCommand = async (commandId: string) => {
    if (!extension) return

    setExecutingCommand(commandId)
    const args = commandArgs[commandId] || {}
    try {
      const result = await executeExtensionCommand(extension.id, commandId, args)
      setCommandResults((prev) => ({ ...prev, [commandId]: result }))
    } catch (error) {
      setCommandResults((prev) => ({
        ...prev,
        [commandId]: { success: false, message: error instanceof Error ? error.message : "Unknown error" },
      }))
    } finally {
      setExecutingCommand(null)
    }
  }

  // Update command arg
  const updateCommandArg = (commandId: string, paramName: string, value: unknown) => {
    setCommandArgs((prev) => ({
      ...prev,
      [commandId]: { ...(prev[commandId] || {}), [paramName]: value },
    }))
  }

  // Reset state when dialog closes
  const handleClose = useCallback(() => {
    if (!saving) {
      setHealth(null)
      setLogs([])
      setLogsLoading(false)
      setLogLevelFilter("all")
      setConfigData(null)
      setConfigValues({})
      setActiveSection("overview")
      setExpandedCommand(null)
      setCommandArgs({})
      setCommandResults({})
      setExecutingCommand(null)
      setExpandedMetric(null)
      setMetricHistoryData({})
      setMetricHistoryLoading({})
      setMetricHistoryError({})
      onOpenChange(false)
    }
  }, [saving, onOpenChange])

  // --- AI tools toggle helpers (master + per-command) ---
  // OFF asks for confirmation (changes agent capabilities); ON is immediate.
  const runMasterToggle = async (checked: boolean) => {
    if (!extension) return
    try {
      await api.setExtensionAiEnabled(extension.id, checked)
      toast({
        title: checked
          ? t("extensions:tools.enableMasterDone", { defaultValue: "AI tools enabled" })
          : t("extensions:tools.disableMasterDone", { defaultValue: "AI tools disabled" }),
      })
      await fetchExtensions()
    } catch (e) {
      handleError(e, { operation: "Toggle extension tools", showToast: true })
    }
  }

  const runCmdToggle = async (cmdId: string, checked: boolean) => {
    if (!extension) return
    try {
      await api.setExtensionCommandEnabled(extension.id, cmdId, checked)
      await fetchExtensions()
    } catch (e) {
      handleError(e, { operation: "Toggle extension command", showToast: true })
    }
  }

  const confirmDisable = () => {
    if (!pendingDisable) return
    if (pendingDisable.kind === "master") {
      runMasterToggle(false)
    } else if (pendingDisable.cmdId) {
      runCmdToggle(pendingDisable.cmdId, false)
    }
    setPendingDisable(null)
  }

  // Load details when dialog opens
  useEffect(() => {
    if (open && extension) {
      loadDetails()
    }
  }, [open, extension?.id])

  // Reset section on open
  useEffect(() => {
    if (open) {
      setActiveSection("overview")
      setExpandedCommand(null)
      setCommandArgs({})
      setCommandResults({})
      setExpandedMetric(null)
      setMetricHistoryData({})
      setMetricHistoryLoading({})
      setMetricHistoryError({})
    }
  }, [open])

  // Auto-refresh logs when viewing the logs section
  useEffect(() => {
    if (activeSection !== "logs" || !extension || !open) return

    loadLogs()
    const interval = setInterval(() => {
      silentRefreshLogs()
    }, 3000)

    return () => clearInterval(interval)
  }, [activeSection, extension?.id, open, loadLogs, silentRefreshLogs])

  // Auto-scroll log list to bottom when new logs arrive (latest at bottom) —
  // but only when the user is already pinned to the bottom. If they've scrolled
  // up to read history, leave them be; otherwise the 3s poll would yank them
  // back down every cycle.
  useEffect(() => {
    if (activeSection !== 'logs' || !logListRef.current) return
    if (!pinnedToBottomRef.current) return
    logListRef.current.scrollTop = logListRef.current.scrollHeight
  }, [logs, activeSection])

  // Render config input based on type
  const renderConfigInput = (paramName: string, param: any) => {
    const value = configValues[paramName] ?? param.default

    switch (param.type) {
      case "boolean":
        return (
          <div key={paramName} className="flex items-center justify-between py-3.5 gap-3">
            <div className="flex-1 min-w-0">
              <Label className="text-sm font-medium text-foreground break-words">{param.title || paramName}</Label>
              {param.description && (
                <p className="text-xs text-muted-foreground mt-1 break-words">{param.description}</p>
              )}
            </div>
            <Switch checked={value as boolean} onCheckedChange={(v) => updateConfigValue(paramName, v)} />
          </div>
        )

      case "integer":
      case "number":
        return (
          <div key={paramName} className="space-y-2 py-3.5">
            <Label htmlFor={paramName} className="text-sm font-medium text-foreground">
              {param.title || paramName}
            </Label>
            <Input
              id={paramName}
              type="number"
              value={value as number ?? ""}
              onChange={(e) => updateConfigValue(paramName, Number(e.target.value))}
              min={param.minimum}
              max={param.maximum}
            />
            {param.description && <p className="text-xs text-muted-foreground">{param.description}</p>}
          </div>
        )

      case "string":
      default:
        if (param.enum && param.enum.length > 0) {
          return (
            <div key={paramName} className="space-y-2 py-3.5">
              <Label htmlFor={paramName} className="text-sm font-medium text-foreground">
                {param.title || paramName}
              </Label>
              <Select value={value as string ?? ""} onValueChange={(v) => updateConfigValue(paramName, v)}>
                <SelectTrigger id={paramName}>
                  <SelectValue placeholder={t("common:selectPlaceholder")} />
                </SelectTrigger>
                <SelectContent>
                  {param.enum.map((opt: string) => (
                    <SelectItem key={opt} value={opt}>
                      {opt}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
              {param.description && <p className="text-xs text-muted-foreground">{param.description}</p>}
            </div>
          )
        }

        const isSecret = /password|secret|token|api_?key/i.test(paramName)
        return (
          <div key={paramName} className="space-y-2 py-3.5">
            <Label htmlFor={paramName} className="text-sm font-medium text-foreground">
              {param.title || paramName}
            </Label>
            {isSecret ? (
              <PasswordInput
                id={paramName}
                value={value as string ?? ""}
                onChange={(e) => updateConfigValue(paramName, e.target.value)}
                autoComplete="new-password"
              />
            ) : (
              <Input
                id={paramName}
                type="text"
                value={value as string ?? ""}
                onChange={(e) => updateConfigValue(paramName, e.target.value)}
              />
            )}
            {param.description && <p className="text-xs text-muted-foreground">{param.description}</p>}
          </div>
        )
    }
  }

  // Render dynamic input for command parameters
  const renderCommandInput = (commandId: string, paramName: string, param: any) => {
    const value = commandArgs[commandId]?.[paramName] ?? param.default ?? ""

    switch (param.type) {
      case "boolean":
        return (
          <div key={paramName} className="flex items-center justify-between py-1.5 gap-2">
            <Label className="text-sm">{param.title || paramName}</Label>
            <Switch
              checked={value as boolean}
              onCheckedChange={(v) => updateCommandArg(commandId, paramName, v)}
            />
          </div>
        )

      case "integer":
      case "number":
        return (
          <div key={paramName} className="space-y-1 py-1.5">
            <Label className="text-sm">{param.title || paramName}</Label>
            <Input
              type="number"
              value={value as number ?? ""}
              onChange={(e) => updateCommandArg(commandId, paramName, Number(e.target.value))}
              min={param.minimum}
              max={param.maximum}
              placeholder={param.description}
            />
          </div>
        )

      default:
        if (param.enum && param.enum.length > 0) {
          return (
            <div key={paramName} className="space-y-1 py-1.5">
              <Label className="text-sm">{param.title || paramName}</Label>
              <Select value={value as string ?? ""} onValueChange={(v) => updateCommandArg(commandId, paramName, v)}>
                <SelectTrigger>
                  <SelectValue placeholder={t("common:selectPlaceholder")} />
                </SelectTrigger>
                <SelectContent>
                  {param.enum.map((opt: string) => (
                    <SelectItem key={opt} value={opt}>{opt}</SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </div>
          )
        }

        return (
          <div key={paramName} className="space-y-1 py-1.5">
            <Label className="text-sm">{param.title || paramName}</Label>
            <Input
              value={value as string ?? ""}
              onChange={(e) => updateCommandArg(commandId, paramName, e.target.value)}
              placeholder={param.description}
            />
          </div>
        )
    }
  }

  // Check if config has any parameters
  const hasConfigParams =
    configData?.config_schema && Object.keys(configData.config_schema.properties || {}).length > 0

  // Sections for navigation
  const sections: { id: SectionId; icon: typeof Info; label: string; count?: number }[] = [
    { id: "overview", icon: Info, label: t("extensions:sections.overview", { defaultValue: "Overview" }) },
    { id: "config", icon: Settings, label: t("extensions:sections.config", { defaultValue: "Configuration" }) },
    {
      id: "commands",
      icon: Terminal,
      label: t("extensions:sections.commands", { defaultValue: "Commands" }),
      count: extension?.commands?.length,
    },
    {
      id: "metrics",
      icon: Activity,
      label: t("extensions:sections.metrics", { defaultValue: "Metrics" }),
      count: extension?.metrics?.length,
    },
    { id: "logs", icon: FileText, label: t("extensions:sections.logs", { defaultValue: "Logs" }) },
  ]

  const filteredLogs = useMemo(() => {
    if (logLevelFilter === "all") return logs
    return logs.filter((log) => {
      const lvl = (log.level || "info").toLowerCase()
      if (logLevelFilter === "error") return lvl === "error" || lvl === "fatal"
      if (logLevelFilter === "warn") return lvl === "warn" || lvl === "warning"
      if (logLevelFilter === "info") return lvl === "info" || lvl === "debug" || lvl === "trace"
      return true
    })
  }, [logs, logLevelFilter])

  // ── Section Renderers ──────────────────────────────────────────────

  const renderOverview = () => {
    const extState = extension?.state ?? "Active"
    const isErrState = extState === "Error" || extState === "Crashed"
    const isWarnState = extState === "Warning"
    const isMutedState = extState === "Failed" || extState === "Stopped"
    const errorDetail = extension?.last_error || extension?.last_crash_reason

    return (
      <div className="space-y-4">
        {/* 4-column status & capability summary strip */}
        <div className="grid grid-cols-2 gap-2.5 sm:grid-cols-4">
          <div className="rounded-xl border border-border bg-card px-3.5 py-3">
            <div className={cn(textMini, "text-muted-foreground")}>
              {t("extensions:info.state", { defaultValue: "State" })}
            </div>
            <div className="mt-1.5 flex items-center gap-1.5">
              <span
                className={cn(
                  "h-2 w-2 shrink-0 rounded-full",
                  isErrState ? "bg-error" : isWarnState ? "bg-warning" : isMutedState ? "bg-muted-foreground" : "bg-success",
                )}
              />
              <span className="truncate text-sm font-semibold text-foreground">{extState}</span>
            </div>
          </div>

          <div className="rounded-xl border border-border bg-card px-3.5 py-3">
            <div className={cn(textMini, "text-muted-foreground")}>
              {t("extensions:overview.health", { defaultValue: "Health" })}
            </div>
            <div className="mt-1.5 flex items-center gap-1.5">
              {loading && !health ? (
                <Loader2 className="h-3.5 w-3.5 animate-spin text-muted-foreground" />
              ) : health ? (
                <>
                  {health.healthy ? (
                    <CheckCircle2 className="h-3.5 w-3.5 shrink-0 text-success" />
                  ) : (
                    <XCircle className="h-3.5 w-3.5 shrink-0 text-error" />
                  )}
                  <span className={cn("truncate text-sm font-semibold", health.healthy ? "text-success" : "text-error")}>
                    {health.healthy
                      ? t("extensions:info.healthy", { defaultValue: "Healthy" })
                      : t("extensions:info.unhealthy", { defaultValue: "Unhealthy" })}
                  </span>
                </>
              ) : (
                <span className="text-sm font-semibold text-muted-foreground">—</span>
              )}
            </div>
          </div>

          <div className="rounded-xl border border-border bg-card px-3.5 py-3">
            <div className={cn(textMini, "text-muted-foreground")}>
              {t("extensions:info.commands", { defaultValue: "Commands" })}
            </div>
            <div className="mt-1.5 text-sm font-semibold tabular-nums text-foreground">
              {extension?.commands?.length ?? 0}
            </div>
          </div>

          <div className="rounded-xl border border-border bg-card px-3.5 py-3">
            <div className={cn(textMini, "text-muted-foreground")}>
              {t("extensions:info.metrics", { defaultValue: "Metrics" })}
            </div>
            <div className="mt-1.5 text-sm font-semibold tabular-nums text-foreground">
              {extension?.metrics?.length ?? 0}
            </div>
          </div>
        </div>

        {/* Last error / crash alert */}
        {errorDetail && (isErrState || isWarnState) && (
          <div className="flex items-start gap-2.5 rounded-xl border border-error bg-error-light px-4 py-3 text-xs text-error">
            <AlertTriangle className="mt-0.5 h-4 w-4 shrink-0" />
            <p className="min-w-0 break-words [overflow-wrap:anywhere] leading-relaxed">{errorDetail}</p>
          </div>
        )}

        {/* Basic Info Card */}
        <section className="rounded-xl border border-border bg-card p-4 space-y-3.5">
          <h4 className="text-xs font-semibold uppercase tracking-wide text-muted-foreground">
            {t("extensions:overview.basicInfo", { defaultValue: "Basic Information" })}
          </h4>
          <div className="grid grid-cols-1 gap-3.5 sm:grid-cols-2">
            <div className="min-w-0 space-y-1">
              <div className={cn(textMini, "text-muted-foreground")}>
                {t("extensions:info.id", { defaultValue: "ID" })}
              </div>
              <div className="flex items-center gap-1.5">
                <code className="min-w-0 truncate rounded-md border border-border bg-muted-30 px-2 py-1 font-mono text-xs text-foreground">
                  {extension?.id}
                </code>
                {extension?.id && (
                  <Button
                    type="button"
                    variant="ghost"
                    size="icon"
                    className="h-7 w-7 shrink-0 sm:h-7 sm:w-7"
                    onClick={() => {
                      copyToClipboard(extension.id)
                      toast({ title: t("common:copied", { defaultValue: "Copied" }) })
                    }}
                    aria-label={t("common:copy", { defaultValue: "Copy" })}
                  >
                    <Copy className="h-3.5 w-3.5" />
                  </Button>
                )}
              </div>
            </div>
            <div className="min-w-0 space-y-1">
              <div className={cn(textMini, "text-muted-foreground")}>
                {t("extensions:info.version", { defaultValue: "Version" })}
              </div>
              <p className="font-mono text-sm font-medium text-foreground">
                {extension?.version ? `v${extension.version}` : "—"}
              </p>
            </div>
            {extension?.author && (
              <div className="min-w-0 space-y-1">
                <div className={cn(textMini, "text-muted-foreground")}>
                  {t("extensions:info.author", { defaultValue: "Author" })}
                </div>
                <p className="text-sm text-foreground break-words">{extension.author}</p>
              </div>
            )}
            <div className="min-w-0 space-y-1">
              <div className={cn(textMini, "text-muted-foreground")}>
                {t("extensions:info.name", { defaultValue: "Name" })}
              </div>
              <p className="text-sm text-foreground break-words">{extension?.name}</p>
            </div>
          </div>

          {extension?.description && (
            <div className="border-t border-border pt-3 space-y-1">
              <div className={cn(textMini, "text-muted-foreground")}>
                {t("extensions:info.description", { defaultValue: "Description" })}
              </div>
              <p className="text-sm leading-relaxed text-foreground break-words [overflow-wrap:anywhere]">
                {extension.description}
              </p>
            </div>
          )}
        </section>

        {/* File Info Card */}
        <section className="rounded-xl border border-border bg-card p-4 space-y-3">
          <h4 className="text-xs font-semibold uppercase tracking-wide text-muted-foreground">
            {t("extensions:overview.fileInfo", { defaultValue: "File Information" })}
          </h4>
          <div className="space-y-3">
            <div className="min-w-0 space-y-1">
              <div className={cn(textMini, "text-muted-foreground")}>
                {t("extensions:file.filePath", { defaultValue: "File Path" })}
              </div>
              <div className="flex items-start gap-2 rounded-lg border border-border bg-muted-30 px-3 py-2">
                <code className="min-w-0 flex-1 font-mono text-xs text-foreground break-words [overflow-wrap:anywhere]">
                  {extension?.file_path || "N/A"}
                </code>
                {extension?.file_path && (
                  <Button
                    type="button"
                    variant="ghost"
                    size="icon"
                    className="h-6 w-6 shrink-0 sm:h-6 sm:w-6"
                    onClick={() => {
                      copyToClipboard(extension.file_path!)
                      toast({ title: t("common:copied", { defaultValue: "Copied" }) })
                    }}
                    aria-label={t("common:copy", { defaultValue: "Copy" })}
                  >
                    <Copy className="h-3.5 w-3.5" />
                  </Button>
                )}
              </div>
            </div>
            {extension?.loaded_at && (
              <div className="space-y-1">
                <div className={cn(textMini, "text-muted-foreground")}>
                  {t("extensions:file.loadedAt", { defaultValue: "Loaded At" })}
                </div>
                <p className="text-sm tabular-nums text-foreground">{formatTimestamp(extension.loaded_at)}</p>
              </div>
            )}
          </div>
        </section>
      </div>
    )
  }

  const renderConfig = () => {
    if (configLoading) {
      return (
        <div className="flex justify-center py-12">
          <Loader2 className="h-6 w-6 animate-spin text-muted-foreground" />
        </div>
      )
    }

    if (!hasConfigParams) {
      return (
        <div className="flex flex-col items-center justify-center rounded-xl border border-border bg-card px-6 py-12 text-center">
          <div className="mb-3 flex h-10 w-10 items-center justify-center rounded-xl border border-border bg-muted-30 text-muted-foreground">
            <Settings className="h-5 w-5" strokeWidth={1.75} />
          </div>
          <p className="text-sm text-muted-foreground">
            {t("extensions:config.noConfig", { defaultValue: "This extension does not have configurable options." })}
          </p>
        </div>
      )
    }

    return (
      <div className="rounded-xl border border-border bg-card divide-y divide-border px-4">
        {(() => {
          const props = configData!.config_schema.properties || {}
          const order = configData!.config_schema.propertyOrder as string[] | undefined
          const entries: [string, any][] = order
            ? order.filter((name) => props[name]).map((name) => [name, props[name]])
            : Object.entries(props)

          return entries
            .filter(([name]) => {
              if (name === "custom_server_url" && configValues["server_region"] !== "Custom") {
                return false
              }
              return true
            })
            .map(([name, param]) => renderConfigInput(name, param))
        })()}
      </div>
    )
  }

  const renderCommands = () => {
    if (!extension?.commands || extension.commands.length === 0) {
      return (
        <div className="flex flex-col items-center justify-center rounded-xl border border-border bg-card px-6 py-12 text-center">
          <div className="mb-3 flex h-10 w-10 items-center justify-center rounded-xl border border-border bg-muted-30 text-muted-foreground">
            <Terminal className="h-5 w-5" strokeWidth={1.75} />
          </div>
          <p className="text-sm text-muted-foreground">
            {t("extensions:commands.noCommands", { defaultValue: "This extension does not expose any commands." })}
          </p>
        </div>
      )
    }

    const masterEnabled = extension.ai_enabled !== false

    // Turning OFF asks for confirmation; ON is immediate.
    const handleMasterToggle = (checked: boolean) => {
      if (!checked) {
        setPendingDisable({ kind: "master" })
        return
      }
      runMasterToggle(true)
    }

    const handleCmdToggle = (cmdId: string, checked: boolean) => {
      if (!checked) {
        setPendingDisable({ kind: "command", cmdId })
        return
      }
      runCmdToggle(cmdId, true)
    }

    return (
      <div className="space-y-3">
        {/* Master toggle — hides ALL commands from the LLM when off */}
        <div className="flex items-center justify-between gap-3 rounded-xl border border-border bg-card px-4 py-3.5">
          <div className="min-w-0">
            <p className="text-sm font-medium text-foreground">
              {t("extensions:tools.masterTitle", { defaultValue: "Expose as AI tools" })}
            </p>
            <p className="text-xs text-muted-foreground mt-0.5">
              {masterEnabled
                ? t("extensions:tools.masterOnHint", { defaultValue: "All commands are visible to the agent." })
                : t("extensions:tools.masterOffHint", { defaultValue: "No command from this extension reaches the agent." })}
            </p>
          </div>
          <Switch
            checked={masterEnabled}
            onCheckedChange={handleMasterToggle}
            aria-label={t("extensions:tools.masterTitle", { defaultValue: "Expose as AI tools" })}
          />
        </div>

        {extension.commands.map((command) => {
          const isExpanded = expandedCommand === command.id
          const isExecuting = executingCommand === command.id
          const result = commandResults[command.id]
          const inputProps = (command.input_schema?.properties || {}) as Record<string, any>
          const hasParams = Object.keys(inputProps).length > 0
          const cmdEnabled = masterEnabled && !command.disabled
          const resultText = result
            ? JSON.stringify(result.success ? result.result : result.message, null, 2)
            : ""

          return (
            <div
              key={command.id}
              className={cn(
                "overflow-hidden rounded-xl border border-border bg-card transition-opacity",
                !cmdEnabled && "opacity-65",
              )}
            >
              {/* Command Header — clickable to expand */}
              <div className="flex items-center gap-2 pr-3.5">
                <button
                  type="button"
                  className="flex min-w-0 flex-1 items-center gap-2.5 px-4 py-3 text-left transition-colors hover:bg-muted-30 focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
                  onClick={() => setExpandedCommand(isExpanded ? null : command.id)}
                  aria-expanded={isExpanded}
                >
                  <Badge variant="outline" className="shrink-0 bg-muted-20 font-mono text-xs font-normal text-muted-foreground">
                    {command.id}
                  </Badge>
                  <span className="min-w-0 flex-1 break-words text-sm font-medium text-foreground">
                    {command.display_name}
                  </span>
                  <ChevronDown
                    className={cn(
                      "h-4 w-4 shrink-0 text-muted-foreground transition-transform",
                      isExpanded && "rotate-180"
                    )}
                  />
                </button>
                {/* Per-command tool toggle. Author-declared ai_exposed=false
                    locks the toggle: the command never reaches the LLM. */}
                <Switch
                  checked={command.disabled ? false : masterEnabled}
                  disabled={!masterEnabled || command.ai_exposed === false}
                  title={
                    command.ai_exposed === false
                      ? t("extensions:tools.authorHidden", {
                          defaultValue: "Not AI-exposed (declared by extension)",
                        })
                      : undefined
                  }
                  onCheckedChange={(checked) => handleCmdToggle(command.id, checked)}
                  onClick={(e) => e.stopPropagation()}
                  aria-label={t("extensions:tools.cmdToggleAria", {
                    defaultValue: "Toggle {cmd} as AI tool",
                    cmd: command.id,
                  })}
                />
              </div>

              {/* Expanded Content */}
              {isExpanded && (
                <div className="border-t border-border bg-muted-20 px-4 pb-4 pt-3 space-y-3">
                  {command.description && (
                    <p className="text-xs leading-relaxed text-muted-foreground break-words">{command.description}</p>
                  )}

                  {/* Parameter Form */}
                  {hasParams && (
                    <div className="rounded-lg border border-border bg-card px-3.5 py-2 space-y-1">
                      {Object.entries(inputProps).map(([paramName, param]) =>
                        renderCommandInput(command.id, paramName, param)
                      )}
                    </div>
                  )}

                  {/* Execute Button */}
                  <Button
                    size="sm"
                    onClick={() => handleExecuteCommand(command.id)}
                    disabled={isExecuting}
                  >
                    {isExecuting ? (
                      <Loader2 className="h-3.5 w-3.5 mr-1.5 animate-spin" />
                    ) : (
                      <Play className="h-3.5 w-3.5 mr-1.5" />
                    )}
                    {isExecuting
                      ? t("extensions:commands.executing", { defaultValue: "Executing..." })
                      : t("extensions:commands.execute", { defaultValue: "Execute" })}
                  </Button>

                  {/* Result Display */}
                  {result && (
                    <div
                      className={cn(
                        "overflow-hidden rounded-lg border text-xs",
                        result.success ? "border-border bg-card" : "border-error bg-error-light",
                      )}
                    >
                      <div
                        className={cn(
                          "flex items-center justify-between gap-2 border-b px-3 py-1.5",
                          result.success ? "border-border bg-success-light text-success" : "border-error bg-error-light text-error",
                        )}
                      >
                        <span className="inline-flex items-center gap-1.5 font-medium">
                          {result.success ? (
                            <CheckCircle2 className="h-3.5 w-3.5" />
                          ) : (
                            <XCircle className="h-3.5 w-3.5" />
                          )}
                          {result.success
                            ? t("extensions:commands.resultSuccess", { defaultValue: "Execution succeeded" })
                            : t("extensions:commands.resultFailed", { defaultValue: "Execution failed" })}
                        </span>
                        <Button
                          type="button"
                          variant="ghost"
                          size="icon"
                          className="h-6 w-6 shrink-0 sm:h-6 sm:w-6"
                          onClick={() => {
                            copyToClipboard(resultText)
                            toast({ title: t("common:copied", { defaultValue: "Copied" }) })
                          }}
                          aria-label={t("common:copy", { defaultValue: "Copy" })}
                        >
                          <Copy className="h-3.5 w-3.5" />
                        </Button>
                      </div>
                      <pre className="max-h-64 overflow-auto p-3 font-mono whitespace-pre-wrap break-words [overflow-wrap:anywhere] text-foreground">
                        {resultText}
                      </pre>
                    </div>
                  )}
                </div>
              )}
            </div>
          )
        })}
      </div>
    )
  }

  const renderMetrics = () => {
    if (!extension?.metrics || extension.metrics.length === 0) {
      return (
        <div className="flex flex-col items-center justify-center rounded-xl border border-border bg-card px-6 py-12 text-center">
          <div className="mb-3 flex h-10 w-10 items-center justify-center rounded-xl border border-border bg-muted-30 text-muted-foreground">
            <Activity className="h-5 w-5" strokeWidth={1.75} />
          </div>
          <p className="text-sm text-muted-foreground">
            {t("extensions:metrics.noMetrics", { defaultValue: "This extension does not expose any metrics." })}
          </p>
        </div>
      )
    }

    const rangeOptions = (['1h', '24h', '7d', '30d'] as const).map((r) => ({
      value: r,
      label: r,
    }))

    return (
      <div className="space-y-2.5">
        {extension.metrics.map((metric) => {
          const isExpanded = expandedMetric === metric.name
          const data = metricHistoryData[metric.name] || []
          const isLoading = metricHistoryLoading[metric.name]
          const error = metricHistoryError[metric.name]
          const timeRange = metricTimeRange[metric.name] || '24h'

          // Numeric series for sparkline + summary stats (non-numeric points
          // stay visible in the raw table).
          const numeric = data
            .map((p) => p.value)
            .filter((v): v is number => typeof v === 'number')
          const latest = numeric.length ? numeric[numeric.length - 1] : undefined
          const min = numeric.length ? Math.min(...numeric) : undefined
          const max = numeric.length ? Math.max(...numeric) : undefined
          const avg = numeric.length
            ? numeric.reduce((a, b) => a + b, 0) / numeric.length
            : undefined
          const fmt = (v?: number) =>
            v === undefined ? '—' : Number.isInteger(v) ? String(v) : v.toFixed(2)

          return (
            <div key={metric.name} className="overflow-hidden rounded-xl border border-border bg-card">
              {/* Header — single clean row: name + id, unit + chevron. */}
              <button
                type="button"
                onClick={() => {
                  if (isExpanded) {
                    setExpandedMetric(null)
                  } else {
                    setExpandedMetric(metric.name)
                    if (!metricHistoryData[metric.name]) {
                      fetchMetricHistory(metric.name)
                    }
                  }
                }}
                className="flex w-full items-center justify-between gap-2 px-4 py-3 text-left transition-colors hover:bg-muted-30 focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
                aria-expanded={isExpanded}
              >
                <span className="min-w-0">
                  <span className="block truncate text-sm font-medium text-foreground">{metric.display_name}</span>
                  <span className={cn(textNano, "block truncate font-mono text-muted-foreground")}>{metric.name}</span>
                </span>
                <span className="flex shrink-0 items-center gap-2">
                  {metric.unit && (
                    <span className="rounded-md border border-border bg-muted-20 px-2 py-0.5 text-xs text-muted-foreground">
                      {metric.unit}
                    </span>
                  )}
                  <ChevronDown className={cn("h-4 w-4 text-muted-foreground transition-transform", isExpanded && "rotate-180")} />
                </span>
              </button>

              {/* Expanded history panel */}
              {isExpanded && (
                <div className="border-t border-border bg-muted-20 p-4 space-y-3">
                  {/* Range selector + data type */}
                  <div className="flex flex-wrap items-center gap-2">
                    <span className="text-xs text-muted-foreground">
                      {t("extensions:metrics.timeRange", { defaultValue: "Range" })}:
                    </span>
                    <SegmentedControl
                      options={rangeOptions}
                      value={timeRange}
                      onChange={(range) => {
                        setMetricTimeRange(prev => ({ ...prev, [metric.name]: range }))
                        setMetricHistoryData(prev => ({ ...prev, [metric.name]: [] }))
                        const now = Math.floor(Date.now() / 1000)
                        let start: number
                        switch (range) {
                          case '1h': start = now - 3600; break
                          case '24h': start = now - 86400; break
                          case '7d': start = now - 604800; break
                          case '30d': start = now - 2592000; break
                          default: start = now - 86400
                        }
                        if (extension) {
                          setMetricHistoryLoading(prev => ({ ...prev, [metric.name]: true }))
                          api.getMetricData(extension.id, metric.name, { start, end: now, limit: 1000 })
                            .then(result => setMetricHistoryData(prev => ({ ...prev, [metric.name]: result.data || [] })))
                            .catch(() => setMetricHistoryError(prev => ({ ...prev, [metric.name]: t("extensions:metrics.historyError", { defaultValue: "Failed to load metric history" }) })))
                            .finally(() => setMetricHistoryLoading(prev => ({ ...prev, [metric.name]: false })))
                        }
                      }}
                    />
                    <span className="ml-auto rounded-md border border-border bg-card px-2 py-0.5 font-mono text-xs text-muted-foreground">
                      {metric.data_type}
                    </span>
                  </div>

                  {isLoading ? (
                    <div className="flex justify-center py-6">
                      <Loader2 className="h-5 w-5 animate-spin text-muted-foreground" />
                    </div>
                  ) : error ? (
                    <div className="rounded-lg border border-error bg-error-light px-3 py-2 text-center text-xs text-error">
                      {error}
                    </div>
                  ) : data.length === 0 ? (
                    <div className="flex flex-col items-center py-6 text-muted-foreground text-xs">
                      <Database className="h-6 w-6 mb-1.5 opacity-60" />
                      <p>{t("extensions:metrics.noHistory", { defaultValue: "No historical data available" })}</p>
                    </div>
                  ) : (
                    <>
                      {numeric.length >= 2 ? (
                        <>
                          {/* Clamped wrapper: the Sparkline sizes itself off its
                              container and, without a hard height in this
                              dialog context, stretched the panel — lock it. */}
                          <div className="rounded-lg border border-border bg-card p-2">
                            <div className="h-12 w-full overflow-hidden">
                              <Sparkline data={numeric} height={48} fill colorMode="primary" />
                            </div>
                          </div>
                          {/* Summary stats (aligned with DeviceDetail) */}
                          <div className="grid grid-cols-2 gap-2 md:grid-cols-4">
                            {[
                              [t("extensions:latest", { defaultValue: "Latest" }), fmt(latest)],
                              [t("extensions:min", { defaultValue: "Min" }), fmt(min)],
                              [t("extensions:max", { defaultValue: "Max" }), fmt(max)],
                              [t("extensions:avg", { defaultValue: "Avg" }), fmt(avg)],
                            ].map(([label, value]) => (
                              <div key={label} className="rounded-lg border border-border bg-card px-3 py-2">
                                <div className={cn(textMini, "text-muted-foreground")}>{label}</div>
                                <div className="mt-0.5 truncate font-mono text-sm font-semibold tabular-nums text-foreground">
                                  {value}
                                </div>
                              </div>
                            ))}
                          </div>
                        </>
                      ) : (
                        <p className="text-xs text-muted-foreground">
                          {t("extensions:metrics.nonNumeric", { defaultValue: "Non-numeric values — see raw data below" })}
                        </p>
                      )}
                      {/* Raw data — collapsed by default */}
                      <details className="group">
                        <summary className="cursor-pointer list-none text-xs font-medium text-muted-foreground hover:text-foreground">
                          {t("extensions:metrics.rawData", { defaultValue: "Raw data" })}
                          <ChevronDown className="ml-1 inline h-3 w-3 align-middle transition-transform group-open:rotate-180" />
                        </summary>
                        <div className="mt-2 max-h-44 overflow-y-auto rounded-lg border border-border bg-card">
                          <table className="w-full text-xs">
                            <thead className="sticky top-0 bg-muted-30 text-muted-foreground">
                              <tr>
                                <th className="px-3 py-2 text-left font-medium">{t("extensions:metrics.time", { defaultValue: "Time" })}</th>
                                <th className="px-3 py-2 text-right font-medium">{t("extensions:metrics.value", { defaultValue: "Value" })}</th>
                              </tr>
                            </thead>
                            <tbody className="divide-y divide-border">
                              {data.slice(0, 50).map((point, i) => (
                                <tr key={i} className="hover:bg-muted-20">
                                  <td className="px-3 py-1.5 tabular-nums text-muted-foreground">
                                    {new Date(point.timestamp * 1000).toLocaleString()}
                                  </td>
                                  <td className="px-3 py-1.5 text-right font-mono tabular-nums text-foreground">
                                    {typeof point.value === 'number' ? point.value.toFixed(2) : String(point.value)}
                                  </td>
                                </tr>
                              ))}
                            </tbody>
                          </table>
                        </div>
                      </details>
                    </>
                  )}
                </div>
              )}
            </div>
          )
        })}
      </div>
    )
  }


  if (!extension) {
    return null
  }

  const getLogLevelColor = (level: string) => {
    switch (level) {
      case 'error':
      case 'fatal':
        return 'text-error'
      case 'warn':
      case 'warning':
        return 'text-warning'
      case 'info':
        return 'text-foreground'
      case 'debug':
      case 'trace':
        return 'text-muted-foreground'
      default:
        return 'text-foreground'
    }
  }

  const renderLogs = () => {
    const levelOptions: { value: LogLevelFilter; label: string }[] = [
      { value: "all", label: t("extensions:logs.levelAll", { defaultValue: "All" }) },
      { value: "error", label: t("extensions:logs.levelError", { defaultValue: "Error" }) },
      { value: "warn", label: t("extensions:logs.levelWarn", { defaultValue: "Warn" }) },
      { value: "info", label: t("extensions:logs.levelInfo", { defaultValue: "Info" }) },
    ]

    return (
      <div className="flex h-full min-h-0 flex-col gap-3">
        {/* Logs toolbar */}
        <div className="flex flex-wrap items-center justify-between gap-2 shrink-0">
          <SegmentedControl
            options={levelOptions}
            value={logLevelFilter}
            onChange={setLogLevelFilter}
          />
          <div className="flex items-center gap-2">
            <span className="text-xs tabular-nums text-muted-foreground">
              {t("extensions:logs.count", { count: filteredLogs.length, defaultValue: `${filteredLogs.length} entries` })}
            </span>
            <Button
              type="button"
              variant="outline"
              size="sm"
              className="h-7 px-2.5 text-xs"
              disabled={filteredLogs.length === 0}
              onClick={() => {
                const text = filteredLogs
                  .map((l) => `${new Date(l.timestamp).toLocaleTimeString()} ${(l.level || "info").toUpperCase()} ${l.message}`)
                  .join("\n")
                copyToClipboard(text)
                toast({ title: t("extensions:logs.copied", { defaultValue: "Logs copied to clipboard" }) })
              }}
            >
              <Copy className="mr-1.5 h-3.5 w-3.5" />
              {t("common:copy", { defaultValue: "Copy" })}
            </Button>
            <Button
              type="button"
              variant="outline"
              size="sm"
              className="h-7 px-2.5 text-xs"
              disabled={logsLoading}
              onClick={() => loadLogs()}
            >
              <RefreshCw className={cn("mr-1.5 h-3.5 w-3.5", logsLoading && "animate-spin")} />
              {t("extensions:logs.refresh", { defaultValue: "Refresh" })}
            </Button>
          </div>
        </div>

        {/* Log entries */}
        {logsLoading && logs.length === 0 ? (
          <div className="flex flex-1 items-center justify-center rounded-xl border border-border bg-card">
            <Loader2 className="h-5 w-5 animate-spin text-muted-foreground" />
          </div>
        ) : logs.length === 0 ? (
          <div className="flex flex-1 flex-col items-center justify-center rounded-xl border border-border bg-card text-muted-foreground text-sm">
            <FileText className="h-8 w-8 mb-2 opacity-50" />
            <p>{t("extensions:logs.noLogs", { defaultValue: "No log entries yet" })}</p>
          </div>
        ) : filteredLogs.length === 0 ? (
          <div className="flex flex-1 flex-col items-center justify-center rounded-xl border border-border bg-card text-muted-foreground text-sm">
            <FileText className="h-7 w-7 mb-2 opacity-50" />
            <p>{t("extensions:logs.noMatchingLogs", { defaultValue: "No logs match the selected level" })}</p>
          </div>
        ) : (
          <div className="relative flex min-h-0 flex-1 flex-col overflow-hidden rounded-xl border border-border bg-card font-mono text-xs">
            <div
              ref={logListRef}
              onScroll={(e) => {
                const el = e.currentTarget
                // "Pinned to bottom" while within 32px of the end — avoids
                // flipping off auto-scroll on minor scrollbar rounding.
                pinnedToBottomRef.current =
                  el.scrollHeight - el.scrollTop - el.clientHeight < 32
              }}
              className="flex-1 min-h-0 overflow-y-auto divide-y divide-border"
            >
              {filteredLogs.map((log, idx) => {
                const levelKey = (log.level || 'info').toLowerCase()
                return (
                  <div
                    key={`${log.timestamp}-${idx}`}
                    className={cn(
                      "px-3.5 py-1.5 leading-5 whitespace-pre-wrap break-words [overflow-wrap:anywhere] hover:bg-muted-20",
                      (levelKey === 'error' || levelKey === 'fatal') && "bg-error-light",
                      (levelKey === 'warn' || levelKey === 'warning') && "bg-warning-light",
                    )}
                  >
                    <span className="tabular-nums text-muted-foreground">
                      {new Date(log.timestamp).toLocaleTimeString()}
                    </span>{" "}
                    <span className={cn("uppercase font-semibold tracking-wide", getLogLevelColor(levelKey))}>
                      {levelKey}
                    </span>{" "}
                    <span className="text-foreground">{log.message}</span>
                  </div>
                )
              })}
            </div>
          </div>
        )}
      </div>
    )
  }

  // Sidebar nav item renderer
  const sidebarNav = (
    <nav className="flex flex-col gap-1 p-2.5">
      {sections.map((s) => {
        const isActive = activeSection === s.id
        return (
          <button
            key={s.id}
            type="button"
            onClick={() => handleSectionChange(s.id)}
            className={cn(
              "flex w-full items-center gap-2.5 rounded-lg border px-3 py-2 text-left text-sm font-medium transition-all focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring",
              isActive
                ? "border-primary-light bg-card text-foreground shadow-sm"
                : "border-transparent text-muted-foreground hover:bg-muted-30 hover:text-foreground"
            )}
          >
            <s.icon className="h-4 w-4 shrink-0" strokeWidth={1.75} />
            <span className="min-w-0 flex-1 truncate">{s.label}</span>
            {s.count !== undefined && s.count > 0 && (
              <span
                className={cn(
                  textNano,
                  "shrink-0 rounded-full px-1.5 py-0.5 font-medium tabular-nums",
                  isActive
                    ? "bg-primary-light text-primary"
                    : "bg-muted-30 text-muted-foreground",
                )}
              >
                {s.count}
              </span>
            )}
          </button>
        )
      })}
    </nav>
  )

  // Mobile uses the same underline strip as every other full-screen detail.
  const mobileTabs = (
    <MobileTabStrip
      items={sections.map((section) => ({ value: section.id, label: section.label }))}
      value={activeSection}
      onValueChange={handleSectionChange}
      ariaLabel={extension.name}
    />
  )

  const showConfigFooter = activeSection === "config" && !configLoading && hasConfigParams

  return (
    <FullScreenDialog open={open} onOpenChange={(newOpen) => { if (!newOpen) handleClose() }}>
      <FullScreenDialogHeader
        icon={<Zap className="h-5 w-5" />}
        title={extension.name}
        onClose={handleClose}
      />

      <FullScreenDialogContent>
        {/* Sidebar — hidden on mobile */}
        <FullScreenDialogSidebar>
          {sidebarNav}
        </FullScreenDialogSidebar>

        {/* Main area + optional sticky footer */}
        <div className="flex min-h-0 flex-1 flex-col overflow-hidden">
          {isMobile && mobileTabs}
          <FullScreenDialogMain
            className={cn(
              "bg-muted-20",
              activeSection === "logs" && "flex min-h-0 flex-col overflow-hidden",
            )}
          >
            <div
              className={cn(
                "w-full",
                activeSection === "logs"
                  ? "flex h-full min-h-0 flex-1 flex-col p-4 md:p-6"
                  : "mx-auto max-w-3xl p-4 md:p-6",
              )}
            >
              {activeSection === "overview" && renderOverview()}
              {activeSection === "config" && renderConfig()}
              {activeSection === "commands" && renderCommands()}
              {activeSection === "metrics" && renderMetrics()}
              {activeSection === "logs" && renderLogs()}
            </div>
          </FullScreenDialogMain>

          {showConfigFooter && (
            <FullScreenDialogFooter>
              <Button onClick={saveConfig} disabled={saving} size="sm">
                {saving ? (
                  <Loader2 className="mr-2 h-4 w-4 animate-spin" />
                ) : (
                  <Save className="mr-2 h-4 w-4" />
                )}
                {saving
                  ? t("common:saving", { defaultValue: "Saving..." })
                  : t("extensions:config.saveReload", { defaultValue: "Save & Reload" })}
              </Button>
            </FullScreenDialogFooter>
          )}
        </div>
      </FullScreenDialogContent>

      {/* Confirm tool-disable (master or per-command). OFF changes agent
          capabilities, so confirm; ON is immediate and needs no dialog. */}
      <AlertDialog
        open={!!pendingDisable}
        onOpenChange={(open) => { if (!open) setPendingDisable(null) }}
      >
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>
              {t("extensions:tools.confirmDisableTitle", {
                defaultValue: "Disable AI tools?",
              })}
            </AlertDialogTitle>
            <AlertDialogDescription>
              {pendingDisable?.kind === "master"
                ? t("extensions:tools.confirmDisableMasterDesc", {
                    defaultValue:
                      "The agent will no longer be able to call any command from this extension.",
                  })
                : t("extensions:tools.confirmDisableCmdDesc", {
                    defaultValue:
                      'The agent will no longer be able to call the "{{cmd}}" command.',
                    cmd: pendingDisable?.cmdId ?? "",
                  })}
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel>
              {t("common:cancel", { defaultValue: "Cancel" })}
            </AlertDialogCancel>
            <AlertDialogAction
              className="bg-destructive text-destructive-foreground hover:bg-destructive-hover"
              onClick={confirmDisable}
            >
              {t("extensions:tools.confirmDisableAction", {
                defaultValue: "Disable",
              })}
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </FullScreenDialog>
  )
}
