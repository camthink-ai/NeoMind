import { useState, useEffect, useCallback, useRef, useMemo, type ReactNode } from "react"
import { useTranslation } from "react-i18next"
import { RefreshCw, Eye, Brain, Wrench, Loader2, Server, RotateCcw, Info, Text } from "lucide-react"
import { Button } from "@/components/ui/button"
import { Input } from "@/components/ui/input"
import { Label } from "@/components/ui/label"
import { Badge } from "@/components/ui/badge"
import { Switch } from "@/components/ui/switch"
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select"
import { FormField } from "@/components/ui/field"
import { FormSection, FormSectionGroup } from "@/components/ui/form-section"
import { ConfigFormBuilder } from "@/components/plugins/ConfigFormBuilder"
import { UnifiedFormDialog } from "@/components/dialog/UnifiedFormDialog"
import { useToast } from "@/hooks/use-toast"
import { useErrorHandler } from "@/hooks/useErrorHandler"
import { extractErrorMessage } from "@/lib/notify"
import { api } from "@/lib/api"
import { cn } from "@/lib/utils"
import type { PluginConfigSchema } from "@/types"
import type { ReasoningCapabilities, ThinkingEffort } from "@/types/llm-backend"

/**
 * Ollama model with capabilities
 */
interface OllamaModel {
  name: string
  size?: number
  modified_at?: string
  digest?: string
  details?: {
    format?: string
    family?: string
    families?: string[]
    parameter_size?: string
    quantization_level?: string
  }
  supports_multimodal: boolean
  supports_thinking: boolean
  supports_tools: boolean
  max_context: number
}

/**
 * Unified plugin instance data structure
 */
export interface PluginInstance {
  id: string
  name: string
  plugin_type: string
  enabled: boolean
  running?: boolean
  config?: Record<string, unknown>
  status?: {
    connected?: boolean
    active?: boolean
    error?: string
    latency_ms?: number
  }
  [key: string]: unknown
}

/**
 * Unified plugin type definition
 */
export interface UnifiedPluginType {
  id: string
  type: "llm_backend" | "device_adapter" | "alert_channel"
  name: string
  description: string
  icon: React.ReactNode
  color: string
  config_schema: PluginConfigSchema
  can_add_multiple: boolean
  builtin: boolean
  requires_api_key?: boolean
  supports_streaming?: boolean
  default_model?: string
  default_endpoint?: string
}

interface UniversalPluginConfigDialogProps {
  open: boolean
  onOpenChange: (open: boolean) => void
  pluginType: UnifiedPluginType
  instances: PluginInstance[]
  editingInstance?: PluginInstance | null
  onCreate: (name: string, config: Record<string, unknown>) => Promise<string>
  onUpdate: (id: string, config: Record<string, unknown>) => Promise<void>
  onDelete?: (id: string) => Promise<void>
  onTest?: (id: string) => Promise<{ success: boolean; message?: string; error?: string; latency_ms?: number }>
  onRefresh: () => Promise<void>
  testResults?: Record<string, { success: boolean; message: string }>
  setTestResults?: (results: Record<string, { success: boolean; message: string }>) => void
}

export function UniversalPluginConfigDialog(props: UniversalPluginConfigDialogProps) {
  const {
    open,
    onOpenChange,
    pluginType,
    editingInstance,
    onCreate,
    onUpdate,
    onRefresh,
    testResults: externalTestResults,
    setTestResults: setExternalTestResults,
  } = props

  const { t } = useTranslation(["common", "plugins", "devices"])
  const { toast } = useToast()
  const { handleError, showSuccess } = useErrorHandler()

  const [saving, setSaving] = useState(false)
  const [newInstanceName, setNewInstanceName] = useState("")
  const [nameError, setNameError] = useState<string | null>(null)
  const [internalTestResults, setInternalTestResults] = useState<Record<string, { success: boolean; message: string }>>({})

  // Ollama model state
  const [ollamaModels, setOllamaModels] = useState<OllamaModel[]>([])
  const [loadingModels, setLoadingModels] = useState(false)
  const [selectedModel, setSelectedModel] = useState("")
  const [ollamaEndpoint, setOllamaEndpoint] = useState("http://localhost:11434")

  // llama.cpp server info state
  const [llamacppEndpoint, setLlamacppEndpoint] = useState("http://127.0.0.1:8080")
  const [llamacppApiKey, setLlamacppApiKey] = useState("")
  const [llamacppServerInfo, setLlamacppServerInfo] = useState<{
    status: string
    health: { status: string; latency_ms: number }
    server: {
      model_name?: string
      n_ctx?: number
      total_slots?: number
      version?: string
    }
    capabilities: {
      supports_streaming: boolean
      supports_multimodal: boolean
      supports_thinking: boolean
      supports_tools: boolean
      max_context: number
    }
  } | null>(null)
  const [loadingLlamacppInfo, setLoadingLlamacppInfo] = useState(false)

  // Auto-detected capabilities state
  const [detectedCapabilities, setDetectedCapabilities] = useState<{
    supports_multimodal: boolean
    supports_thinking: boolean
    supports_tools: boolean
    max_context: number
    reasoning?: ReasoningCapabilities
  }>({
    supports_multimodal: false,
    supports_thinking: false,
    supports_tools: true,
    max_context: 8192,
  })

  // Multimodal override state — only meaningful in edit mode (requires backend id for PATCH).
  // `override`: user-set pin (true/false) or null (auto-detection).
  // `effective`: what the backend currently reports (override value when set, else auto).
  // `source`: provenance string from backend (registry / heuristic / runtime_api / user_override).
  // `pending`: disables the switch while a PATCH is in flight.
  const [overrideState, setOverrideState] = useState<{
    override: boolean | null
    effective: boolean
    source: string | null
    pending: boolean
  }>({ override: null, effective: false, source: null, pending: false })

  // Thinking effort state. Unlike multimodal, thinking is a plain backend
  // config field (not an override) — the user choice IS the effective value.
  // `effort: "high"` = thinking on (default), `"none"` = off.
  const [thinkingState, setThinkingState] = useState<{ effort: ThinkingEffort; pending: boolean }>({
    effort: "high",
    pending: false,
  })

  const testResults = externalTestResults ?? internalTestResults
  const setTestResults = setExternalTestResults ?? setInternalTestResults
  const isOllamaBackend = pluginType.type === "llm_backend" && pluginType.id === "ollama"
  const isLlamaCppBackend = pluginType.type === "llm_backend" && pluginType.id === "llamacpp"

  const fetchOllamaModels = useCallback(async (endpoint?: string) => {
    if (!isOllamaBackend) return

    setLoadingModels(true)
    try {
      const response = await api.listOllamaModels(endpoint)
      setOllamaModels(response.models || [])

      if (editingInstance?.config?.model) {
        const currentModel = String(editingInstance.config.model)
        setSelectedModel(currentModel)
        const modelWithCaps = response.models?.find(m => m.name === currentModel)
        if (modelWithCaps) {
          setDetectedCapabilities({
            supports_multimodal: modelWithCaps.supports_multimodal,
            supports_thinking: modelWithCaps.supports_thinking,
            supports_tools: modelWithCaps.supports_tools,
            max_context: modelWithCaps.max_context,
            reasoning: modelWithCaps.reasoning,
          })
        }
      }
    } catch (error) {
      handleError(error, { operation: 'Fetch Ollama models', showToast: false })
    } finally {
      setLoadingModels(false)
    }
  }, [isOllamaBackend, editingInstance, handleError])

  const fetchLlamacppServerInfo = useCallback(async (endpoint?: string, apiKey?: string) => {
    if (!isLlamaCppBackend) return

    setLoadingLlamacppInfo(true)
    try {
      const response = await api.listLlamaCppServerInfo(endpoint, apiKey)
      setLlamacppServerInfo(response)
      if (response.status === "ok") {
        setDetectedCapabilities({
          supports_multimodal: response.capabilities.supports_multimodal,
          supports_thinking: response.capabilities.supports_thinking,
          supports_tools: response.capabilities.supports_tools,
          max_context: response.capabilities.max_context,
        })
      }
    } catch (error) {
      handleError(error, { operation: 'Fetch llama.cpp server info', showToast: false })
      setLlamacppServerInfo(null)
    } finally {
      setLoadingLlamacppInfo(false)
    }
  }, [isLlamaCppBackend, handleError])

  // Track previous open state to detect dialog open transitions
  const prevOpenRef = useRef(open)

  // Reset form when dialog opens (only on open transition, not on every render)
  useEffect(() => {
    // Only reset when dialog transitions from closed to open
    if (open && !prevOpenRef.current) {
      // Create mode: blank name; Edit mode: pre-fill with current name so
      // the field is editable (rename supported). Backend accepts name in
      // UpdateLlmBackendRequest, but the form previously rendered it as a
      // read-only <h3>, making rename impossible.
      setNewInstanceName(editingInstance ? editingInstance.name : "")
      setSelectedModel("")
      setNameError(null)
      // Reset override state on every dialog open; the edit-mode branch below
      // will re-initialise it from the editing instance's saved capabilities.
      setOverrideState({ override: null, effective: false, source: null, pending: false })
      // Reset thinking state too; re-initialised below from instance config.
      setThinkingState({ effort: "high", pending: false })

      if (editingInstance && (editingInstance as any).capabilities) {
        const existingCaps = (editingInstance as any).capabilities
        setDetectedCapabilities({
          supports_multimodal: existingCaps.supports_multimodal ?? false,
          supports_thinking: existingCaps.supports_thinking ?? false,
          supports_tools: existingCaps.supports_tools ?? true,
          max_context: existingCaps.max_context ?? 8192,
          reasoning: existingCaps.reasoning ?? undefined,
        })
        // Initialise multimodal override state from saved instance capabilities.
        // In create mode this branch is skipped — overrideState stays at its default
        // (override=null) and the switch is not rendered.
        setOverrideState({
          override: existingCaps.multimodal_user_override ?? null,
          effective: existingCaps.supports_multimodal ?? false,
          source: existingCaps.multimodal_source ?? null,
          pending: false,
        })
        // Initialise thinking effort from saved backend config. Prefer the
        // unified `thinking_effort`; fall back to the legacy `thinking_enabled`
        // bool (true → "high", false → "none"). Defaults to "high" if absent.
        const savedEffort = (editingInstance.config as any)?.thinking_effort as
          | ThinkingEffort
          | undefined
        const savedEnabled = (editingInstance.config as any)?.thinking_enabled
        const initEffort: ThinkingEffort = savedEffort ?? (savedEnabled === false ? "none" : "high")
        setThinkingState({
          effort: initEffort,
          pending: false,
        })
        if (pluginType.id === "ollama") {
          const instanceEndpoint = editingInstance?.config?.endpoint as string | undefined
          fetchOllamaModels(instanceEndpoint)
          if (instanceEndpoint) {
            setOllamaEndpoint(instanceEndpoint)
          }
        }
        if (pluginType.id === "llamacpp") {
          const instanceEndpoint = editingInstance?.config?.endpoint as string | undefined
          const instanceApiKey = editingInstance?.config?.api_key as string | undefined
          if (instanceEndpoint) {
            setLlamacppEndpoint(instanceEndpoint)
          }
          if (instanceApiKey) {
            setLlamacppApiKey(instanceApiKey)
          }
          fetchLlamacppServerInfo(instanceEndpoint, instanceApiKey)
        }
      } else if (pluginType.type === "llm_backend") {
        if (pluginType.id === "ollama") {
          setDetectedCapabilities({
            supports_multimodal: false,
            supports_thinking: true,
            supports_tools: true,
            max_context: 8192,
          })
          fetchOllamaModels()
        } else if (pluginType.id === "openai" || pluginType.id === "google" || pluginType.id === "anthropic") {
          setDetectedCapabilities({
            supports_multimodal: true,
            supports_thinking: false,
            supports_tools: true,
            max_context: 128000,
          })
        } else if (pluginType.id === "llamacpp") {
          setDetectedCapabilities({
            supports_multimodal: false,
            supports_thinking: true,
            supports_tools: true,
            max_context: 4096,
          })
          setLlamacppServerInfo(null)
        } else {
          setDetectedCapabilities({
            supports_multimodal: false,
            supports_thinking: false,
            supports_tools: true,
            max_context: 128000,
          })
        }
      }
    }
    // Update ref for next render
    prevOpenRef.current = open
  }, [open, pluginType.id, pluginType.type, fetchOllamaModels, fetchLlamacppServerInfo, editingInstance])

  const handleModelChange = (modelName: string) => {
    setSelectedModel(modelName)
    const model = ollamaModels.find(m => m.name === modelName)
    if (model) {
      setDetectedCapabilities({
        supports_multimodal: model.supports_multimodal,
        supports_thinking: model.supports_thinking,
        supports_tools: model.supports_tools,
        max_context: model.max_context,
      })
    }
  }

  const getInstanceStatus = (instance: PluginInstance) => {
    if (instance.status?.connected !== undefined) return instance.status.connected
    if (instance.status?.active !== undefined) return instance.status.active
    return instance.running ?? instance.enabled
  }

  const handleCreate = async (values: Record<string, unknown>) => {
    if (!newInstanceName.trim()) {
      setNameError(t("plugins:instanceNameRequired", { defaultValue: "Instance name is required" }))
      return
    }

    setSaving(true)
    try {
      let configWithCaps = values
      if (pluginType.type === "llm_backend") {
        configWithCaps = {
          ...values,
          capabilities: detectedCapabilities,
          ...(isOllamaBackend && selectedModel ? { model: selectedModel } : {}),
          ...(isOllamaBackend ? { endpoint: ollamaEndpoint } : {}),
          ...(isLlamaCppBackend ? { endpoint: llamacppEndpoint } : {}),
          ...(isLlamaCppBackend && llamacppApiKey ? { api_key: llamacppApiKey } : {}),
          ...(isLlamaCppBackend && llamacppServerInfo?.server?.model_name ? { model: llamacppServerInfo.server.model_name } : {}),
        }
      }

      await onCreate(newInstanceName.trim(), configWithCaps)
      toast({
        title: t("common:success"),
        description: t("plugins:instanceCreated", { defaultValue: "Instance created successfully" }),
      })
      setNewInstanceName("")
      setSelectedModel("")
      onOpenChange(false)
      await onRefresh()
    } catch (error) {
      toast({
        title: t("common:failed"),
        description: String(error),
        variant: "destructive",
      })
    } finally {
      setSaving(false)
    }
  }

  const handleUpdate = async (values: Record<string, unknown>) => {
    if (!editingInstance) return

    // Validate rename: instance name is required (mirrors create mode).
    if (!newInstanceName.trim()) {
      setNameError(t("plugins:instanceNameRequired", { defaultValue: "Instance name is required" }))
      return
    }

    setSaving(true)
    try {
      let configWithCaps: Record<string, unknown> = {
        ...values,
        // Include the (possibly renamed) instance name. The parent extracts
        // this and sends it as UpdateLlmBackendRequest.name.
        name: newInstanceName.trim(),
      }
      if (pluginType.type === "llm_backend") {
        configWithCaps = {
          ...configWithCaps,
          capabilities: detectedCapabilities,
          ...(isOllamaBackend && selectedModel ? { model: selectedModel } : {}),
          ...(isOllamaBackend ? { endpoint: ollamaEndpoint } : {}),
          ...(isLlamaCppBackend ? { endpoint: llamacppEndpoint } : {}),
          ...(isLlamaCppBackend && llamacppApiKey ? { api_key: llamacppApiKey } : {}),
          ...(isLlamaCppBackend && llamacppServerInfo?.server?.model_name ? { model: llamacppServerInfo.server.model_name } : {}),
        }
      }

      await onUpdate(editingInstance.id, configWithCaps)
      toast({
        title: t("common:success"),
        description: t("plugins:instanceUpdated", { defaultValue: "Instance updated successfully" }),
      })
      onOpenChange(false)
      await onRefresh()
    } catch (error) {
      toast({
        title: t("common:failed"),
        description: String(error),
        variant: "destructive",
      })
    } finally {
      setSaving(false)
    }
  }

  const getConfigSchema = () => {
    const schema = { ...pluginType.config_schema }

    // For Ollama backends, exclude endpoint and model fields since they're handled separately
    if (isOllamaBackend && schema.properties) {
      const { model, endpoint, ...restProperties } = schema.properties
      schema.properties = restProperties
      if (schema.required) {
        schema.required = schema.required.filter((field: string) => field !== 'model' && field !== 'endpoint')
      }
      if (schema.ui_hints?.field_order) {
        schema.ui_hints.field_order = schema.ui_hints.field_order.filter((field: string) => field !== 'model' && field !== 'endpoint')
      }
    }

    // For llama.cpp backends, exclude endpoint and model fields since they're handled by the server info UI
    if (isLlamaCppBackend && schema.properties) {
      const { endpoint, model, ...restProperties } = schema.properties
      schema.properties = restProperties
      if (schema.required) {
        schema.required = schema.required.filter((field: string) => field !== 'endpoint' && field !== 'model')
      }
      if (schema.ui_hints?.field_order) {
        schema.ui_hints.field_order = schema.ui_hints.field_order.filter((field: string) => field !== 'endpoint' && field !== 'model')
      }
    }

    if (editingInstance && editingInstance.config) {
      if (!schema.properties) schema.properties = {}
      for (const [key, value] of Object.entries(editingInstance.config)) {
        if (schema.properties[key]) {
          schema.properties[key] = {
            ...schema.properties[key] as any,
            default: value,
          }
        }
      }
    }

    return schema
  }

  const isEditing = !!editingInstance
  const schema = getConfigSchema()

  // PATCH the multimodal override. The dialog's main Save button is decoupled —
  // override is a correction to auto-detection, not a model-parameter edit, and
  // should take effect on the next chat turn without a full save.
  //
  // We deliberately do NOT call `onRefresh()` here: the parent's `loadData`
  // flips `setLoading(true)` which replaces the whole tab with a page-level
  // loading skeleton — that would unmount this dialog mid-interaction. The
  // PATCH response is authoritative, so local state stays correct; the card
  // list's Eye tooltip reconciles on the next natural parent refresh.
  const patchMultimodalOverride = async (value: boolean | null) => {
    if (!editingInstance || overrideState.pending) return
    const prev = overrideState
    setOverrideState({ ...prev, pending: true })
    try {
      const res = await api.updateLlmBackendCapabilitiesOverride(editingInstance.id, {
        multimodal: value,
      })
      setOverrideState({
        override: res.multimodal_user_override,
        effective: res.supports_multimodal,
        source: res.multimodal_source,
        pending: false,
      })
      setDetectedCapabilities((cur) => ({ ...cur, supports_multimodal: res.supports_multimodal }))
      showSuccess(t("plugins:llm.overrideSavedToast"))
    } catch (error) {
      setOverrideState(prev)
      const isNotFound = (error as { status?: number })?.status === 404
      handleError(error as Error, {
        operation: "Update multimodal override",
        userMessage: isNotFound
          ? t("plugins:llm.backendNotFound")
          : t("plugins:llm.overrideSaveFailed", { message: extractErrorMessage(error) }),
      })
    }
  }

  const handleToggleMultimodalOverride = () =>
    patchMultimodalOverride(!overrideState.effective)
  const handleResetMultimodalOverride = () => patchMultimodalOverride(null)

  // PATCH thinking_effort directly on the backend (not via capabilities
  // override endpoint — thinking is a plain config field, not an override).
  // Optimistic update with rollback on error, mirroring patchMultimodalOverride.
  //
  // We deliberately do NOT call `onRefresh()` here: the parent's loadData
  // flips setLoading(true) which replaces the whole tab with a page-level
  // loading skeleton — that unmounts this dialog mid-interaction and wipes
  // any in-progress form edits. The PATCH response is authoritative, so local
  // state stays correct; the card list reconciles on the next natural refresh.
  const patchEffort = async (effort: ThinkingEffort) => {
    if (!editingInstance || thinkingState.pending) return
    const prev = thinkingState
    setThinkingState({ effort, pending: true })
    try {
      await api.updateLlmBackend(editingInstance.id, { thinking_effort: effort })
      setThinkingState({ effort, pending: false })
      showSuccess(t("plugins:llm.thinkingSavedToast"))
    } catch (error) {
      setThinkingState(prev)
      const isNotFound = (error as { status?: number })?.status === 404
      handleError(error as Error, {
        operation: "Update thinking effort",
        userMessage: isNotFound
          ? t("plugins:llm.backendNotFound")
          : t("plugins:llm.thinkingSaveFailed", { message: extractErrorMessage(error) }),
      })
    }
  }

  const renderCapabilityBadges = () => {
    // Spec-row layout: each capability is one row — [icon] name on the left,
    // value/control right-aligned. Deliberately NOT FormField: this is a
    // capability summary panel, and wrapping mixed read-only/interactive items
    // in form-field chrome reads as "weird half-form". A bordered tinted panel
    // with iconified rows reads as a spec sheet instead.
    const renderRow = (
      key: string,
      icon: ReactNode,
      label: string,
      children: ReactNode
    ) => (
      <div key={key} className="flex items-center justify-between gap-3 min-h-8">
        <div className="flex items-center gap-2 min-w-0">
          <span className="shrink-0">{icon}</span>
          <span className="text-sm truncate">{label}</span>
        </div>
        <div className="flex items-center gap-2 shrink-0">{children}</div>
      </div>
    )

    const rows: ReactNode[] = []

    // Vision / multimodal — interactive override in edit mode, read-only in create
    if (detectedCapabilities.supports_multimodal) {
      if (!isEditing) {
        rows.push(
          renderRow(
            "vision",
            <Eye className="h-4 w-4 text-info" />,
            t("plugins:llm.capabilityVision"),
            <Badge variant="outline" className="text-xs h-6">
              {t("plugins:llm.capabilityVisionSupported", { defaultValue: "Supported" })}
            </Badge>
          )
        )
      } else {
        const effective = overrideState.effective
        const override = overrideState.override
        rows.push(
          renderRow(
            "vision",
            <Eye className="h-4 w-4 text-info" />,
            t("plugins:llm.capabilityVision"),
            <>
              <Switch
                checked={effective}
                onCheckedChange={handleToggleMultimodalOverride}
                disabled={overrideState.pending}
                aria-label={t("plugins:llm.capabilityVision")}
              />
              <span className="text-xs text-muted-foreground">
                {override != null
                  ? t("plugins:llm.capabilityVisionOverrideLabelText")
                  : t("plugins:llm.capabilityVisionAutoLabel")}
              </span>
              {override != null && (
                <Button
                  variant="ghost"
                  size="sm"
                  className="h-6 w-6 p-0"
                  onClick={handleResetMultimodalOverride}
                  disabled={overrideState.pending}
                  aria-label={t("plugins:llm.resetToAuto")}
                  title={t("plugins:llm.resetToAuto")}
                >
                  <RotateCcw className="h-3 w-3" />
                </Button>
              )}
            </>
          )
        )
      }
    }

    // Thinking effort — read-only badge when model-controlled or non-edit, else dropdown
    if (detectedCapabilities.supports_thinking) {
      const control = detectedCapabilities.reasoning?.control
      const isReadOnly = control === "readonly" || control === undefined
      if (!isEditing || isReadOnly) {
        rows.push(
          renderRow(
            "thinking",
            <Brain className="h-4 w-4 text-accent-purple" />,
            t("plugins:llm.capabilityThinking"),
            <Badge variant="outline" className="text-xs h-6">
              {t("plugins:llm.capabilityThinkingReadOnly", { defaultValue: "Thinking (model default)" })}
            </Badge>
          )
        )
      } else {
        const showLevels = control !== "boolean"
        rows.push(
          renderRow(
            "thinking",
            <Brain className="h-4 w-4 text-accent-purple" />,
            t("plugins:llm.capabilityThinking"),
            <>
              <Select
                value={thinkingState.effort}
                onValueChange={(v) => patchEffort(v as ThinkingEffort)}
                disabled={thinkingState.pending}
              >
                <SelectTrigger className="w-[140px] h-8" aria-label={t("plugins:llm.capabilityThinking")}>
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value="none">{t("plugins:llm.effortNone")}</SelectItem>
                  {showLevels && (
                    <>
                      <SelectItem value="low">{t("plugins:llm.effortLow")}</SelectItem>
                      <SelectItem value="medium">{t("plugins:llm.effortMedium")}</SelectItem>
                    </>
                  )}
                  <SelectItem value="high">{t("plugins:llm.effortHigh")}</SelectItem>
                </SelectContent>
              </Select>
              {thinkingState.pending && (
                <Loader2 className="h-3 w-3 animate-spin text-muted-foreground" />
              )}
            </>
          )
        )
      }
    }

    // Tools — read-only
    if (detectedCapabilities.supports_tools) {
      rows.push(
        renderRow(
          "tools",
          <Wrench className="h-4 w-4 text-accent-orange" />,
          t("plugins:llm.capabilityTools"),
          <Badge variant="outline" className="text-xs h-6">
            {t("plugins:llm.capabilityToolsSupported", { defaultValue: "Supported" })}
          </Badge>
        )
      )
    }

    // Context window — read-only
    rows.push(
      renderRow(
        "context",
        <Text className="h-4 w-4 text-accent-cyan" />,
        t("plugins:llm.capabilityContext", { defaultValue: "Context Window" }),
        <span className="text-sm text-muted-foreground tabular-nums">
          {detectedCapabilities.max_context >= 100000
            ? `${Math.round(detectedCapabilities.max_context / 1000)}k tokens`
            : `${detectedCapabilities.max_context} tokens`}
        </span>
      )
    )

    if (rows.length === 0) return null

    return (
      <div className="rounded-lg border border-border bg-muted-30 p-3 space-y-2">
        {rows}
      </div>
    )
  }

  const getModelIcon = (model: OllamaModel) => {
    const icons = []
    if (model.supports_multimodal) icons.push(<Eye key="vision" className="h-4 w-4 text-info" />)
    if (model.supports_thinking) icons.push(<Brain key="thinking" className="h-4 w-4 text-accent-purple" />)
    if (model.supports_tools) icons.push(<Wrench key="tools" className="h-4 w-4 text-accent-orange" />)
    if (icons.length === 0) return null
    return <span className="flex items-center gap-0.5">{icons}</span>
  }

  // Stable key for form content to prevent unnecessary remounting
  // Changes only when dialog type/instance changes, not on every keystroke
  const formKey = useMemo(() =>
    `${pluginType.id}-${editingInstance?.id || 'new'}-${open ? 'open' : 'closed'}`,
    [pluginType.id, editingInstance?.id, open]
  )

  // Stable form id so the footer Submit button can bind to this form via the
  // HTML `form` attribute (button lives in dialog footer, form lives in body).
  const formId = useMemo(() => `plugin-config-form-${formKey}`, [formKey])
  const submitLabel = isEditing
    ? t("common:save", { defaultValue: "Save" })
    : t("common:create", { defaultValue: "Create" })

  return (
    <UnifiedFormDialog
      open={open}
      onOpenChange={onOpenChange}
      title={
        isEditing
          ? t("plugins:editInstance", { defaultValue: "Edit Instance" })
          : pluginType.name
      }
      description={
        isEditing
          ? t("plugins:editInstanceDesc", { defaultValue: "Configure this instance" })
          : pluginType.description
      }
      icon={<span className={pluginType.color}>{pluginType.icon}</span>}
      width="xl"
      className="z-[110]"
      isSubmitting={saving}
      preventCloseOnSubmit={false}
      footer={
        <>
          <Button
            type="button"
            variant="outline"
            onClick={() => onOpenChange(false)}
            className="min-w-[80px]"
          >
            {t("common:cancel", { defaultValue: "Cancel" })}
          </Button>
          <Button
            type="submit"
            form={formId}
            disabled={saving}
            className="min-w-[80px]"
          >
            {saving && <Loader2 className="mr-2 h-4 w-4 animate-spin" />}
            {submitLabel}
          </Button>
        </>
      }
    >
      <FormSectionGroup key={formKey}>
        {/* Instance Name Field (only for create mode) */}
        {!isEditing && (
          <FormField
            label={t("plugins:instanceName", { defaultValue: "Instance Name" })}
            required
            error={nameError || undefined}
          >
            <Input
              value={newInstanceName}
              onChange={(e) => {
                setNewInstanceName(e.target.value)
                if (nameError) setNameError(null)
              }}
              placeholder={t("plugins:instanceNamePlaceholder", { defaultValue: "My Instance" })}
              disabled={saving}
            />
          </FormField>
        )}

        {/* Edit Mode: Editable name + status badge (rename supported) */}
        {isEditing && (
          <FormField
            label={t("plugins:instanceName", { defaultValue: "Instance Name" })}
            required
            error={nameError || undefined}
          >
            <div className="flex items-center gap-3">
              <Input
                value={newInstanceName}
                onChange={(e) => {
                  setNewInstanceName(e.target.value)
                  if (nameError) setNameError(null)
                }}
                placeholder={t("plugins:instanceNamePlaceholder", { defaultValue: "My Instance" })}
                disabled={saving}
                className="flex-1"
              />
              <Badge variant={getInstanceStatus(editingInstance) ? "default" : "secondary"} className="shrink-0">
                {getInstanceStatus(editingInstance)
                  ? t("plugins:active", { defaultValue: "Active" })
                  : t("plugins:inactive", { defaultValue: "Inactive" })
                }
              </Badge>
            </div>
          </FormField>
        )}

        {/* Ollama endpoint configuration */}
        {isOllamaBackend && (
          <FormField label={t("plugins:llm.endpoint", { defaultValue: "Ollama Endpoint" })}>
            <div className="flex items-center gap-2">
              <Input
                value={ollamaEndpoint}
                onChange={(e) => setOllamaEndpoint(e.target.value)}
                placeholder="http://localhost:11434"
                disabled={saving}
              />
              <Button
                variant="outline"
                size="icon"
                onClick={() => fetchOllamaModels(ollamaEndpoint)}
                disabled={loadingModels}
              >
                <RefreshCw className={cn("h-4 w-4", loadingModels && "animate-spin")} />
              </Button>
            </div>
          </FormField>
        )}

        {/* Ollama model selector */}
        {isOllamaBackend && ollamaModels.length > 0 && (
          <FormField label={t("plugins:llm.selectModel", { defaultValue: "Select Model" })}>
            <div className="flex items-center gap-2">
              <Select value={selectedModel} onValueChange={handleModelChange}>
                <SelectTrigger className="flex-1">
                  <SelectValue placeholder={t("plugins:llm.selectModelPlaceholder", { defaultValue: "Select a model..." })} />
                </SelectTrigger>
                <SelectContent>
                  {ollamaModels.map((model) => (
                    <SelectItem
                      key={model.name}
                      value={model.name}
                    >
                      {model.name}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
              <Button
                variant="outline"
                size="icon"
                onClick={() => fetchOllamaModels(ollamaEndpoint)}
                disabled={loadingModels}
              >
                <RefreshCw className={cn("h-4 w-4", loadingModels && "animate-spin")} />
              </Button>
            </div>
            {selectedModel && renderCapabilityBadges()}
          </FormField>
        )}

        {/* No models message for Ollama */}
        {isOllamaBackend && ollamaModels.length === 0 && !loadingModels && (
          <div className="p-3 bg-muted-30 rounded-lg text-sm text-muted-foreground">
            {t("plugins:llm.noModelsFound", { defaultValue: "No models found. Click refresh to fetch from Ollama." })}
          </div>
        )}

        {/* Capability display for non-Ollama, non-llama.cpp LLM backends */}
        {pluginType.type === "llm_backend" && !isOllamaBackend && !isLlamaCppBackend && (
          <div className="space-y-2">
            <label className="text-sm font-medium">{t("plugins:llm.capabilities", { defaultValue: "Capabilities" })}</label>
            {renderCapabilityBadges()}
          </div>
        )}

        {/* llama.cpp endpoint check + server info */}
        {isLlamaCppBackend && (
          <FormField label={t("plugins:llm.llamacppEndpoint", { defaultValue: "llama.cpp Endpoint" })}>
            <div className="space-y-3">
              <div className="flex items-center gap-2">
                <Input
                  value={llamacppEndpoint}
                  onChange={(e) => setLlamacppEndpoint(e.target.value)}
                  placeholder="http://127.0.0.1:8080"
                  disabled={saving}
                />
                <Button
                  variant="outline"
                  size="icon"
                  onClick={() => fetchLlamacppServerInfo(llamacppEndpoint, llamacppApiKey || undefined)}
                  disabled={loadingLlamacppInfo}
                >
                  <RefreshCw className={cn("h-4 w-4", loadingLlamacppInfo && "animate-spin")} />
                </Button>
              </div>

              {/* Model name - always visible when available */}
              {(() => {
                // Priority: live server info > saved config
                const liveModel = llamacppServerInfo?.status === "ok" ? llamacppServerInfo.server.model_name : undefined
                const savedModel = editingInstance?.config?.model as string | undefined
                const modelName = liveModel || savedModel

                if (!modelName) return (
                  <div className="p-2 bg-muted-30 rounded-md text-xs text-muted-foreground">
                    {t("plugins:llm.clickToDetectModel", { defaultValue: "Click the check button to detect the loaded model from the server." })}
                  </div>
                )

                return (
                  <div className="flex items-center gap-2 p-2 bg-background rounded-md border">
                    <Brain className="h-4 w-4 text-info shrink-0" />
                    <div className="min-w-0 flex-1">
                      <div className="text-xs text-muted-foreground">{t("plugins:llm.loadedModel", { defaultValue: "Loaded Model" })}</div>
                      <div className="font-medium text-sm truncate">{modelName}</div>
                    </div>
                    {!liveModel && savedModel && (
                      <Badge variant="outline" className="text-xs shrink-0">
                        {t("plugins:llm.saved", { defaultValue: "saved" })}
                      </Badge>
                    )}
                  </div>
                )
              })()}

              {/* Server info display (live details) */}
              {llamacppServerInfo && llamacppServerInfo.status === "ok" && (
                <div className="p-3 bg-muted-30 rounded-lg space-y-2 text-sm">
                  <div className="flex items-center gap-2">
                    <Server className="h-4 w-4 text-success" />
                    <span className="font-medium text-success">
                      {t("plugins:llm.serverConnected", { defaultValue: "Server connected" })}
                    </span>
                    <span className="text-muted-foreground">
                      ({llamacppServerInfo.health.latency_ms}ms)
                    </span>
                  </div>

                  {/* Server properties */}
                  {(llamacppServerInfo.server.n_ctx || llamacppServerInfo.server.total_slots || llamacppServerInfo.server.version) && (
                    <div className="flex flex-wrap gap-2">
                      {llamacppServerInfo.server.n_ctx && (
                        <Badge variant="secondary" className="text-xs">
                          {llamacppServerInfo.server.n_ctx >= 100000
                            ? `${Math.round(llamacppServerInfo.server.n_ctx / 1000)}k ctx`
                            : `${llamacppServerInfo.server.n_ctx} ctx`}
                        </Badge>
                      )}
                      {llamacppServerInfo.server.total_slots && (
                        <Badge variant="secondary" className="text-xs">
                          {llamacppServerInfo.server.total_slots} slots
                        </Badge>
                      )}
                      {llamacppServerInfo.server.version && (
                        <Badge variant="secondary" className="text-xs">
                          v{llamacppServerInfo.server.version}
                        </Badge>
                      )}
                    </div>
                  )}
                </div>
              )}

              {/* Capabilities — always visible for llama.cpp, independent of
                  server connectivity. (llama.cpp is ReadOnly for thinking —
                  thinking follows the model default — but the control must not
                  silently disappear.) */}
              {renderCapabilityBadges()}

              {/* Error state */}
              {llamacppServerInfo && llamacppServerInfo.status !== "ok" && (
                <div className="p-3 bg-muted rounded-lg text-sm text-error">
                  {t("plugins:llm.serverUnreachable", { defaultValue: "Server unreachable" })}: {llamacppServerInfo.health.status}
                </div>
              )}
            </div>
          </FormField>
        )}

        {/* Schema-driven notice banner (e.g. external broker feature diff) */}
        {schema.ui_hints?.notice?.i18nKey && (
          <div className="mt-4 flex gap-2.5 rounded-lg bg-muted p-3">
            <Info className="h-4 w-4 shrink-0 mt-0.5 text-muted-foreground" />
            <p className="text-sm text-muted-foreground leading-relaxed">
              {t(schema.ui_hints.notice.i18nKey, { defaultValue: '' })}
            </p>
          </div>
        )}

        {/* Config Form - Embedded directly */}
        <div className="mt-4">
          <ConfigFormBuilder
            schema={schema}
            onSubmit={isEditing ? handleUpdate : handleCreate}
            loading={saving}
            formId={formId}
            hideSubmitButton
          />
        </div>
      </FormSectionGroup>
    </UnifiedFormDialog>
  )
}
