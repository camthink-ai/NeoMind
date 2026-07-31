import { useState, useEffect, useCallback, useRef } from "react"
import { useTranslation } from "react-i18next"
import { Button } from "@/components/ui/button"
import { Input } from "@/components/ui/input"
import { Label } from "@/components/ui/label"
import { Badge } from "@/components/ui/badge"
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
  TrendingUp,
  Database,
  FileText,
  Trash2,
} from "lucide-react"
import { useErrorHandler } from "@/hooks/useErrorHandler"
import { useStore } from "@/store"
import { formatTimestamp } from "@/lib/utils/format"
import { useToast } from "@/hooks/use-toast"
import { api } from "@/lib/api"
import type { Extension, ExtensionConfigResponse, ExtensionLogEntry } from "@/types"
import { useIsMobile } from "@/hooks/useMobile"
import { cn } from "@/lib/utils"
import { FormSection, FormSectionGroup } from "@/components/ui/form-section"

interface ExtensionDetailsDialogProps {
  extension: Extension | null
  open: boolean
  onOpenChange: (open: boolean) => void
}

type SectionId = "overview" | "config" | "commands" | "metrics" | "logs"

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
  const clearExtensionLogs = useStore((state) => state.clearExtensionLogs)
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
      toast({ title: "Configuration saved successfully" })

      const reloadSuccess = await reloadExtensionStore(extension.id)
      if (reloadSuccess) {
        toast({ title: "Extension reloaded with new configuration" })
      } else {
        toast({ title: "Extension reloaded, but config may not have been applied", variant: "default" })
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

  // Clear logs
  const handleClearLogs = async () => {
    if (!extension) return
    try {
      await clearExtensionLogs(extension.id)
      setLogs([])
      toast({ title: t("extensions:logs.cleared", { defaultValue: "Logs cleared" }) })
    } catch (error) {
      handleError(error, { operation: "Clear extension logs" })
    }
  }

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
      await api.setExtensionEnabled(extension.id, checked)
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
          <div key={paramName} className="flex items-center justify-between py-2 gap-2">
            <div className="flex-1 min-w-0">
              <Label className="text-sm font-medium break-words">{param.title || paramName}</Label>
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
          <div key={paramName} className="space-y-2 py-2">
            <Label htmlFor={paramName} className="text-sm font-medium">
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
            <div key={paramName} className="space-y-2 py-2">
              <Label htmlFor={paramName} className="text-sm font-medium">
                {param.title || paramName}
              </Label>
              <Select value={value as string ?? ""} onValueChange={(v) => updateConfigValue(paramName, v)}>
                <SelectTrigger id={paramName}>
                  <SelectValue placeholder="Select..." />
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

        return (
          <div key={paramName} className="space-y-2 py-2">
            <Label htmlFor={paramName} className="text-sm font-medium">
              {param.title || paramName}
            </Label>
            <Input
              id={paramName}
              type={paramName.toLowerCase().includes("password") ? "password" : "text"}
              value={value as string ?? ""}
              onChange={(e) => updateConfigValue(paramName, e.target.value)}
            />
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
                  <SelectValue placeholder="Select..." />
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
  const sections: { id: SectionId; icon: typeof Info; label: string }[] = [
    { id: "overview", icon: Info, label: t("extensions:sections.overview", { defaultValue: "Overview" }) },
    { id: "config", icon: Settings, label: t("extensions:sections.config", { defaultValue: "Configuration" }) },
    { id: "commands", icon: Terminal, label: t("extensions:sections.commands", { defaultValue: "Commands" }) },
    { id: "metrics", icon: Activity, label: t("extensions:sections.metrics", { defaultValue: "Metrics" }) },
    { id: "logs", icon: FileText, label: t("extensions:sections.logs", { defaultValue: "Logs" }) },
  ]

  // ── Section Renderers ──────────────────────────────────────────────

  const renderOverview = () => (
    <FormSectionGroup>
      {/* Basic Info Grid */}
      <FormSection title={t("extensions:overview.basicInfo", { defaultValue: "Basic Information" })} defaultExpanded>
        <div className="grid grid-cols-2 gap-3">
          <div>
            <Label className="text-muted-foreground text-xs">{t("extensions:info.id", { defaultValue: "ID" })}</Label>
            <p className="text-sm font-mono break-all">{extension?.id}</p>
          </div>
          <div>
            <Label className="text-muted-foreground text-xs">{t("extensions:info.name", { defaultValue: "Name" })}</Label>
            <p className="text-sm break-words">{extension?.name}</p>
          </div>
          <div>
            <Label className="text-muted-foreground text-xs">{t("extensions:info.version", { defaultValue: "Version" })}</Label>
            <p className="text-sm">{extension?.version}</p>
          </div>
          <div>
            <Label className="text-muted-foreground text-xs">{t("extensions:info.state", { defaultValue: "State" })}</Label>
            <div className="mt-1">
              <Badge variant={extension?.state === "Error" || extension?.state === "Warning" ? "destructive" : "default"}>
                {extension?.state}
              </Badge>
            </div>
          </div>
          <div>
            <Label className="text-muted-foreground text-xs">{t("extensions:info.commands", { defaultValue: "Commands" })}</Label>
            <p className="text-sm">{extension?.commands?.length ?? 0}</p>
          </div>
          <div>
            <Label className="text-muted-foreground text-xs">{t("extensions:info.metrics", { defaultValue: "Metrics" })}</Label>
            <p className="text-sm">{extension?.metrics?.length ?? 0}</p>
          </div>
        </div>

        {extension?.description && (
          <div className="mt-3">
            <Label className="text-muted-foreground text-xs">{t("extensions:info.description", { defaultValue: "Description" })}</Label>
            <p className="text-sm break-words">{extension.description}</p>
          </div>
        )}

        {extension?.author && (
          <div className="mt-3">
            <Label className="text-muted-foreground text-xs">{t("extensions:info.author", { defaultValue: "Author" })}</Label>
            <p className="text-sm break-words">{extension.author}</p>
          </div>
        )}
      </FormSection>

      {/* Health Status */}
      {health && (
        <FormSection title={t("extensions:overview.health", { defaultValue: "Health Status" })} defaultExpanded>
          <Badge variant={health.healthy ? "default" : "destructive"}>
            {health.healthy
              ? t("extensions:info.healthy", { defaultValue: "Healthy" })
              : t("extensions:info.unhealthy", { defaultValue: "Unhealthy" })}
          </Badge>
        </FormSection>
      )}

      {/* File Info */}
      <FormSection title={t("extensions:overview.fileInfo", { defaultValue: "File Information" })} collapsible>
        <div className="space-y-3">
          <div>
            <Label className="text-muted-foreground text-xs">{t("extensions:file.filePath", { defaultValue: "File Path" })}</Label>
            <p className="text-sm font-mono break-all">{extension?.file_path || "N/A"}</p>
          </div>
          {extension?.loaded_at && (
            <div>
              <Label className="text-muted-foreground text-xs">{t("extensions:file.loadedAt", { defaultValue: "Loaded At" })}</Label>
              <p className="text-sm break-all">{formatTimestamp(extension.loaded_at)}</p>
            </div>
          )}
        </div>
      </FormSection>

      {loading && (
        <div className="flex justify-center py-4">
          <Loader2 className="h-5 w-5 animate-spin text-muted-foreground" />
        </div>
      )}
    </FormSectionGroup>
  )

  const renderConfig = () => {
    if (configLoading) {
      return (
        <div className="flex justify-center py-8">
          <Loader2 className="h-6 w-6 animate-spin text-muted-foreground" />
        </div>
      )
    }

    if (!hasConfigParams) {
      return (
        <div className="text-center py-8 text-muted-foreground text-sm">
          <Settings className="h-8 w-8 mx-auto mb-2 opacity-50" />
          <p>{t("extensions:config.noConfig", { defaultValue: "This extension does not have configurable options." })}</p>
        </div>
      )
    }

    return (
      <FormSectionGroup>
        <div className="space-y-1">
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

        <div className="pt-4">
          <Button onClick={saveConfig} disabled={saving}>
            <Save className="h-4 w-4 mr-2" />
            {saving
              ? t("common:saving", { defaultValue: "Saving..." })
              : t("extensions:config.saveReload", { defaultValue: "Save & Reload" })}
          </Button>
        </div>
      </FormSectionGroup>
    )
  }

  const renderCommands = () => {
    if (!extension?.commands || extension.commands.length === 0) {
      return (
        <div className="text-center py-8 text-muted-foreground text-sm">
          <Terminal className="h-8 w-8 mx-auto mb-2 opacity-50" />
          <p>{t("extensions:commands.noCommands", { defaultValue: "This extension does not expose any commands." })}</p>
        </div>
      )
    }

    const masterEnabled = extension.enabled !== false

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
        <div className="flex items-center justify-between gap-3 border rounded-lg p-3 bg-muted-30">
          <div className="min-w-0">
            <p className="text-sm font-medium">
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

          return (
            <div key={command.id} className={cn("border rounded-lg overflow-hidden", !cmdEnabled && "opacity-60")}>
              {/* Command Header — clickable to expand */}
              <div className="w-full flex items-center gap-3 p-3">
                <button
                  className="flex items-center gap-3 flex-1 min-w-0 text-left hover:bg-muted-30 transition-colors -m-3 p-3"
                  onClick={() => setExpandedCommand(isExpanded ? null : command.id)}
                >
                  <Badge variant="outline" className="text-xs shrink-0">
                    {command.id}
                  </Badge>
                  <span className="font-medium text-sm flex-1 min-w-0 break-words">
                    {command.display_name}
                  </span>
                  <ChevronDown
                    className={cn(
                      "h-4 w-4 shrink-0 text-muted-foreground transition-transform",
                      isExpanded && "rotate-180"
                    )}
                  />
                </button>
                {/* Per-command tool toggle */}
                <Switch
                  checked={!!command.disabled ? false : masterEnabled}
                  disabled={!masterEnabled}
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
                <div className="border-t px-3 pb-3 pt-2 space-y-3">
                  {command.description && (
                    <p className="text-xs text-muted-foreground break-words">{command.description}</p>
                  )}

                  {/* Parameter Form */}
                  {hasParams && (
                    <div className="space-y-1">
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
                    <div className={cn(
                      "rounded-lg p-3 text-xs font-mono overflow-auto max-h-64",
                      result.success ? "bg-success-light" : "bg-error-light"
                    )}>
                      <pre className="whitespace-pre-wrap break-words">
                        {JSON.stringify(result.success ? result.result : result.message, null, 2)}
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
        <div className="text-center py-8 text-muted-foreground text-sm">
          <Activity className="h-8 w-8 mx-auto mb-2 opacity-50" />
          <p>{t("extensions:metrics.noMetrics", { defaultValue: "This extension does not expose any metrics." })}</p>
        </div>
      )
    }

    return (
      <div className="space-y-2">
        {extension.metrics.map((metric) => {
          const isExpanded = expandedMetric === metric.name
          const data = metricHistoryData[metric.name] || []
          const isLoading = metricHistoryLoading[metric.name]
          const error = metricHistoryError[metric.name]
          const timeRange = metricTimeRange[metric.name] || '24h'

          return (
            <div key={metric.name} className="border rounded-lg overflow-hidden">
              {/* Metric Header */}
              <div className="p-3 flex flex-col sm:flex-row sm:items-center sm:justify-between gap-2 bg-muted-30">
                <div className="flex flex-wrap items-center gap-2">
                  <span className="font-medium text-sm break-words">{metric.display_name}</span>
                  <Badge variant="outline" className="text-xs break-all">
                    {metric.name}
                  </Badge>
                </div>
                <div className="flex flex-wrap items-center gap-2">
                  <div className="flex flex-wrap items-center gap-3 text-xs text-muted-foreground">
                    <span>{metric.data_type}</span>
                    {metric.unit && <span>({metric.unit})</span>}
                    {metric.min !== undefined && metric.max !== undefined && (
                      <span>[{metric.min} - {metric.max}]</span>
                    )}
                    {metric.required && (
                      <Badge variant="secondary" className="text-xs">
                        {t("common:required", { defaultValue: "required" })}
                      </Badge>
                    )}
                  </div>
                  <Button
                    variant="ghost"
                    size="sm"
                    className="h-7 text-xs"
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
                  >
                    <TrendingUp className="h-3.5 w-3.5 mr-1" />
                    {t("extensions:metrics.viewHistory", { defaultValue: "History" })}
                    <ChevronDown className={cn(
                      "h-3.5 w-3.5 ml-1 transition-transform",
                      isExpanded && "rotate-180"
                    )} />
                  </Button>
                </div>
              </div>

              {/* Expanded History Panel */}
              {isExpanded && (
                <div className="border-t p-3 space-y-3">
                  {/* Time Range Selector */}
                  <div className="flex items-center gap-2">
                    <span className="text-xs text-muted-foreground">
                      {t("extensions:metrics.timeRange", { defaultValue: "Range" })}:
                    </span>
                    <div className="flex gap-1">
                      {(['1h', '24h', '7d', '30d'] as const).map((range) => (
                        <Button
                          key={range}
                          variant={timeRange === range ? "default" : "outline"}
                          size="sm"
                          className="h-6 text-xs px-2"
                          onClick={() => {
                            setMetricTimeRange(prev => ({ ...prev, [metric.name]: range }))
                            // Re-fetch with new range
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
                                .catch(() => setMetricHistoryError(prev => ({ ...prev, [metric.name]: "Failed to load" })))
                                .finally(() => setMetricHistoryLoading(prev => ({ ...prev, [metric.name]: false })))
                            }
                          }}
                        >
                          {range}
                        </Button>
                      ))}
                    </div>
                  </div>

                  {isLoading ? (
                    <div className="flex justify-center py-6">
                      <Loader2 className="h-5 w-5 animate-spin text-muted-foreground" />
                    </div>
                  ) : error ? (
                    <div className="text-center py-4 text-error text-xs">{error}</div>
                  ) : data.length === 0 ? (
                    <div className="flex flex-col items-center py-6 text-muted-foreground text-xs">
                      <Database className="h-6 w-6 mb-1 opacity-50" />
                      <p>{t("extensions:metrics.noHistory", { defaultValue: "No historical data available" })}</p>
                    </div>
                  ) : (
                    <div className="max-h-48 overflow-y-auto border rounded-md">
                      <table className="w-full text-xs">
                        <thead className="bg-muted sticky top-0">
                          <tr>
                            <th className="p-1.5 text-left">{t("extensions:metrics.time", { defaultValue: "Time" })}</th>
                            <th className="p-1.5 text-right">{t("extensions:metrics.value", { defaultValue: "Value" })}</th>
                          </tr>
                        </thead>
                        <tbody>
                          {data.slice(0, 50).map((point, i) => (
                            <tr key={i} className="border-t">
                              <td className="p-1.5 text-muted-foreground">
                                {new Date(point.timestamp * 1000).toLocaleString()}
                              </td>
                              <td className="p-1.5 text-right font-mono">
                                {typeof point.value === 'number' ? point.value.toFixed(2) : String(point.value)}
                              </td>
                            </tr>
                          ))}
                        </tbody>
                      </table>
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
    return (
      <div className="flex h-full flex-col">
        {/* Log entries */}
        {logsLoading && logs.length === 0 ? (
          <div className="flex-1 flex items-center justify-center">
            <Loader2 className="h-5 w-5 animate-spin text-muted-foreground" />
          </div>
        ) : logs.length === 0 ? (
          <div className="flex-1 flex flex-col items-center justify-center text-muted-foreground text-sm">
            <FileText className="h-8 w-8 mb-2 opacity-50" />
            <p>{t("extensions:logs.noLogs", { defaultValue: "No log entries yet" })}</p>
          </div>
        ) : (
          <div className="relative flex-1 min-h-0 flex flex-col rounded-lg overflow-hidden bg-muted-30 font-mono text-xs">
            {/* Clear — overlay icon so we don't need a dedicated toolbar row */}
            <Button
              variant="ghost"
              size="xs"
              className="absolute right-1.5 top-1.5 z-10 h-7 w-7 p-0 text-muted-foreground hover:bg-error-light hover:text-error"
              onClick={handleClearLogs}
              aria-label={t("extensions:logs.clear", { defaultValue: "Clear" })}
              title={t("extensions:logs.clear", { defaultValue: "Clear" })}
            >
              <Trash2 className="h-3.5 w-3.5" />
            </Button>
            <div
              ref={logListRef}
              onScroll={(e) => {
                const el = e.currentTarget
                // "Pinned to bottom" while within 32px of the end — avoids
                // flipping off auto-scroll on minor scrollbar rounding.
                pinnedToBottomRef.current =
                  el.scrollHeight - el.scrollTop - el.clientHeight < 32
              }}
              className="flex-1 min-h-0 overflow-y-auto"
            >
              {logs.map((log) => {
                const levelKey = (log.level || 'info').toLowerCase()
                return (
                  <div
                    key={`${log.timestamp}-${log.message}`}
                    className={cn(
                      "px-3 py-1 leading-5 whitespace-pre-wrap break-words hover:bg-muted-50",
                      levelKey === 'error' && "bg-error-light",
                      levelKey === 'warn' && "bg-warning-light",
                    )}
                  >
                    <span className="text-muted-foreground">
                      {new Date(log.timestamp).toLocaleTimeString()}
                    </span>{" "}
                    <span className={cn("uppercase font-medium tracking-wide", getLogLevelColor(levelKey))}>
                      {levelKey}
                    </span>{" "}
                    <span>{log.message}</span>
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
    <div className="p-2 space-y-0.5">
      {sections.map((s) => (
        <button
          key={s.id}
          onClick={() => handleSectionChange(s.id)}
          className={cn(
            "flex items-center gap-2.5 w-full px-3 py-2 rounded-lg text-sm font-medium transition-colors",
            activeSection === s.id
              ? "bg-primary text-primary-foreground"
              : "text-muted-foreground hover:text-foreground hover:bg-muted-30"
          )}
        >
          <s.icon className="h-4 w-4 shrink-0" />
          <span>{s.label}</span>
        </button>
      ))}
    </div>
  )

  // Mobile tabs renderer — 5-column grid, single row, no scroll, no divider
  const mobileTabs = (
    <div className="shrink-0 px-2 pt-3 pb-2">
      <div className="grid grid-cols-5 gap-1">
        {sections.map((s) => (
          <button
            key={s.id}
            onClick={() => handleSectionChange(s.id)}
            className={cn(
              "flex flex-col items-center justify-center gap-1 py-1.5 min-w-0 rounded-lg transition-colors",
              activeSection === s.id
                ? "bg-primary text-primary-foreground"
                : "bg-muted-30 text-muted-foreground"
            )}
          >
            <s.icon className="h-4 w-4 shrink-0" />
            <span className="text-[11px] font-medium leading-none truncate w-full text-center">
              {s.label}
            </span>
          </button>
        ))}
      </div>
    </div>
  )

  return (
    <FullScreenDialog open={open} onOpenChange={(newOpen) => { if (!newOpen) handleClose() }}>
      <FullScreenDialogHeader
        icon={<Zap className="h-5 w-5" />}
        title={extension.name}
        subtitle={`v${extension.version}`}
        onClose={handleClose}
      />

      <FullScreenDialogContent>
        {/* Sidebar — hidden on mobile */}
        <FullScreenDialogSidebar>
          {sidebarNav}
        </FullScreenDialogSidebar>

        {/* Main area */}
        <FullScreenDialogMain>
          <div className="flex flex-col h-full">
            {/* Mobile tabs */}
            {isMobile && mobileTabs}

            {/* Section content */}
            <div className="flex-1 overflow-y-auto p-4 md:p-6">
              {activeSection === "overview" && renderOverview()}
              {activeSection === "config" && renderConfig()}
              {activeSection === "commands" && renderCommands()}
              {activeSection === "metrics" && renderMetrics()}
              {activeSection === "logs" && renderLogs()}
            </div>
          </div>
        </FullScreenDialogMain>
      </FullScreenDialogContent>

      {/* Confirm tool-disable (master or per-command). OFF changes agent
          capabilities, so confirm; ON is immediate and needs no dialog. */}
      <AlertDialog
        open={!!pendingDisable}
        onOpenChange={(open) => { if (!open) setPendingDisable(null) }}
      >
        <AlertDialogContent className="z-[200]">
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
              className="bg-destructive text-error-foreground hover:bg-destructive-hover"
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
