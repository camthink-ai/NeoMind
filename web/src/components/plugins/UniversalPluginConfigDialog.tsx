import { useState, useEffect, useCallback } from "react"
import { useTranslation } from "react-i18next"
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
  DialogContentBody,
} from "@/components/ui/dialog"
import { Button } from "@/components/ui/button"
import { Input } from "@/components/ui/input"
import { Label } from "@/components/ui/label"
import { Badge } from "@/components/ui/badge"
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select"
import { TestTube, Check, X, Plus, Trash2, RefreshCw, Eye, Brain, Wrench } from "lucide-react"
import { ConfigFormBuilder } from "@/components/plugins/ConfigFormBuilder"
import { useToast } from "@/hooks/use-toast"
import { useErrorHandler } from "@/hooks/useErrorHandler"
import { confirm } from "@/hooks/use-confirm"
import { api } from "@/lib/api"
import { cn } from "@/lib/utils"
import type { PluginConfigSchema } from "@/types"

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
  // Type-specific status
  status?: {
    connected?: boolean
    active?: boolean
    error?: string
    latency_ms?: number
  }
  // Additional metadata
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

  // Schema-driven configuration
  config_schema: PluginConfigSchema

  // Instance management
  can_add_multiple: boolean
  builtin: boolean

  // Display info
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

  // API callbacks
  onCreate: (name: string, config: Record<string, unknown>) => Promise<string>
  onUpdate: (id: string, config: Record<string, unknown>) => Promise<void>
  onDelete?: (id: string) => Promise<void>
  onTest?: (id: string) => Promise<{ success: boolean; message?: string; error?: string; latency_ms?: number }>

  // Refresh callback
  onRefresh: () => Promise<void>

  // Test result tracking (optional external state)
  testResults?: Record<string, { success: boolean; message: string }>
  setTestResults?: (results: Record<string, { success: boolean; message: string }>) => void
}

export function UniversalPluginConfigDialog(props: UniversalPluginConfigDialogProps) {
  const {
    open,
    onOpenChange,
    pluginType,
    instances,
    editingInstance,
    onCreate,
    onUpdate,
    onDelete,
    onTest,
    onRefresh,
    testResults: externalTestResults,
    setTestResults: setExternalTestResults,
  } = props

  const { t } = useTranslation(["common", "plugins", "devices"])
  const { toast } = useToast()
  const { handleError } = useErrorHandler()

  const [saving, setSaving] = useState(false)
  const [newInstanceName, setNewInstanceName] = useState("")
  const [internalTestResults, setInternalTestResults] = useState<Record<string, { success: boolean; message: string }>>({})

  // Ollama model state
  const [ollamaModels, setOllamaModels] = useState<OllamaModel[]>([])
  const [loadingModels, setLoadingModels] = useState(false)
  const [selectedModel, setSelectedModel] = useState("")
  const [ollamaEndpoint, setOllamaEndpoint] = useState("http://localhost:11434")

  // Auto-detected capabilities state
  const [detectedCapabilities, setDetectedCapabilities] = useState<{
    supports_multimodal: boolean
    supports_thinking: boolean
    supports_tools: boolean
    max_context: number
  }>({
    supports_multimodal: false,
    supports_thinking: false,
    supports_tools: true,
    max_context: 8192,
  })

  const testResults = externalTestResults ?? internalTestResults
  const setTestResults = setExternalTestResults ?? setInternalTestResults

  const isOllamaBackend = pluginType.type === "llm_backend" && pluginType.id === "ollama"

  // Pass test and delete handlers to parent via prop for use in detail view
  // The dialog itself no longer displays instance lists

  // Fetch Ollama models
  const fetchOllamaModels = useCallback(async (endpoint?: string) => {
    if (!isOllamaBackend) return

    setLoadingModels(true)
    try {
      const response = await api.listOllamaModels(endpoint)
      setOllamaModels(response.models || [])

      // Auto-select the current model if editing
      if (editingInstance?.config?.model) {
        const currentModel = String(editingInstance.config.model)
        setSelectedModel(currentModel)
        // Find capabilities for current model
        const modelWithCaps = response.models?.find(m => m.name === currentModel)
        if (modelWithCaps) {
          setDetectedCapabilities({
            supports_multimodal: modelWithCaps.supports_multimodal,
            supports_thinking: modelWithCaps.supports_thinking,
            supports_tools: modelWithCaps.supports_tools,
            max_context: modelWithCaps.max_context,
          })
        }
      }
    } catch (error) {
      handleError(error, { operation: 'Fetch Ollama models', showToast: false })
      // Don't show toast for this error - it's optional functionality
    } finally {
      setLoadingModels(false)
    }
  }, [isOllamaBackend, editingInstance])

  // Reset form when dialog opens or plugin type changes
  useEffect(() => {
    if (open) {
      setNewInstanceName("")
      setSelectedModel("")

      // If editing, load existing capabilities
      if (editingInstance && (editingInstance as any).capabilities) {
        const existingCaps = (editingInstance as any).capabilities
        setDetectedCapabilities({
          supports_multimodal: existingCaps.supports_multimodal ?? false,
          supports_thinking: existingCaps.supports_thinking ?? false,
          supports_tools: existingCaps.supports_tools ?? true,
          max_context: existingCaps.max_context ?? 8192,
        })
        // If it's Ollama and we're editing, also fetch models
        if (pluginType.id === "ollama") {
          fetchOllamaModels()
        }
      } else if (pluginType.type === "llm_backend") {
        // Reset to default capabilities for new instances
        if (pluginType.id === "ollama") {
          // Ollama default - will be updated when user selects a model
          setDetectedCapabilities({
            supports_multimodal: false,  // Will be detected from selected model
            supports_thinking: true,
            supports_tools: true,
            max_context: 8192,
          })
          // Fetch models when Ollama dialog opens
          fetchOllamaModels()
        } else if (pluginType.id === "openai" || pluginType.id === "google" || pluginType.id === "anthropic") {
          setDetectedCapabilities({
            supports_multimodal: true,
            supports_thinking: false,
            supports_tools: true,
            max_context: 128000,
          })
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
  }, [open, pluginType.id, pluginType.type, fetchOllamaModels, editingInstance])

  // Handle model selection change
  const handleModelChange = (modelName: string) => {
    setSelectedModel(modelName)

    // Auto-detect capabilities from selected model
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

  // Get instance status display
  const getInstanceStatus = (instance: PluginInstance) => {
    if (instance.status?.connected !== undefined) {
      return instance.status.connected
    }
    if (instance.status?.active !== undefined) {
      return instance.status.active
    }
    return instance.running ?? instance.enabled
  }

  // Handle create new instance
  const handleCreate = async (values: Record<string, unknown>) => {
    if (!newInstanceName.trim()) {
      toast({
        title: t("common:failed"),
        description: t("plugins:instanceNameRequired", { defaultValue: "Instance name is required" }),
        variant: "destructive",
      })
      return
    }

    setSaving(true)
    try {
      // For LLM backends, add capabilities (auto-detected for Ollama) and model for Ollama
      let configWithCaps = values
      if (pluginType.type === "llm_backend") {
        configWithCaps = {
          ...values,
          capabilities: detectedCapabilities,
          // For Ollama, include the selected model from the dropdown
          ...(isOllamaBackend && selectedModel ? { model: selectedModel } : {}),
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

  // Handle update instance
  const handleUpdate = async (values: Record<string, unknown>) => {
    if (!editingInstance) return

    setSaving(true)
    try {
      // For LLM backends, add capabilities and model for Ollama
      let configWithCaps = values
      if (pluginType.type === "llm_backend") {
        configWithCaps = {
          ...values,
          capabilities: detectedCapabilities,
          // For Ollama, include the selected model from the dropdown
          ...(isOllamaBackend && selectedModel ? { model: selectedModel } : {}),
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

  // Handle delete instance - exported for parent components to use
  // This is no longer called from within the dialog
  const handleDelete = async (instance: PluginInstance) => {
    if (!onDelete) return

    const confirmed = await confirm({
      title: t("common:delete"),
      description: t("plugins:confirmDeleteInstance", { defaultValue: "Delete this instance?" }),
      confirmText: t("common:delete"),
      cancelText: t("common:cancel"),
      variant: "destructive"
    })
    if (!confirmed) return

    setSaving(true)
    try {
      await onDelete(instance.id)
      toast({
        title: t("common:success"),
        description: t("plugins:instanceDeleted", { defaultValue: "Instance deleted" }),
      })
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

  // Handle test connection - exported for parent components to use
  // This is no longer called from within the dialog
  const handleTest = async (instance: PluginInstance) => {
    if (!onTest) return { success: false, message: "Test not available" }

    try {
      const result = await onTest(instance.id)
      const newResult = { success: result.success, message: result.message || result.error || "" }
      setTestResults({
        ...testResults,
        [instance.id]: newResult,
      })
      if (result.success) {
        toast({
          title: t("common:success"),
          description: result.message || t("plugins:testSuccess", { defaultValue: "Connection successful" }),
        })
      } else {
        toast({
          title: t("common:failed"),
          description: result.error || result.message || t("plugins:testFailed", { defaultValue: "Connection failed" }),
          variant: "destructive",
        })
      }
      return newResult
    } catch (error) {
      const message = String(error)
      setTestResults({
        ...testResults,
        [instance.id]: { success: false, message },
      })
      toast({
        title: t("common:failed"),
        description: message,
        variant: "destructive",
      })
      return { success: false, message }
    }
  }

  // Prepare config schema with name field pre-filled
  const getConfigSchema = () => {
    const schema = { ...pluginType.config_schema }

    // For Ollama backend, exclude the model field from the schema since it's handled by the specialized selector above
    if (isOllamaBackend && schema.properties?.model) {
      const { model, ...restProperties } = schema.properties
      schema.properties = restProperties
      // Also remove model from required array if present
      if (schema.required) {
        schema.required = schema.required.filter((field: string) => field !== 'model')
      }
      // Update field_order in ui_hints if present
      if (schema.ui_hints?.field_order) {
        schema.ui_hints.field_order = schema.ui_hints.field_order.filter((field: string) => field !== 'model')
      }
    }

    // If editing, populate default values from existing config
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

  // Render capability badges
  const renderCapabilityBadges = () => (
    <div className="flex flex-wrap gap-2 mt-2">
      {detectedCapabilities.supports_multimodal && (
        <Badge variant="outline" className="text-xs">
          <Eye className="h-3 w-3 mr-1" />
          Vision
        </Badge>
      )}
      {detectedCapabilities.supports_thinking && (
        <Badge variant="outline" className="text-xs">
          <Brain className="h-3 w-3 mr-1" />
          Thinking
        </Badge>
      )}
      {detectedCapabilities.supports_tools && (
        <Badge variant="outline" className="text-xs">
          <Wrench className="h-3 w-3 mr-1" />
          Tools
        </Badge>
      )}
      <Badge variant="secondary" className="text-xs">
        {detectedCapabilities.max_context >= 100000
          ? `${Math.round(detectedCapabilities.max_context / 1000)}k ctx`
          : `${detectedCapabilities.max_context} ctx`}
      </Badge>
    </div>
  )

  // Generate icon for model in select dropdown
  const getModelIcon = (model: OllamaModel) => {
    const icons = []
    if (model.supports_multimodal) icons.push(<Eye key="vision" className="h-3 w-3 text-blue-500" />)
    if (model.supports_thinking) icons.push(<Brain key="thinking" className="h-3 w-3 text-purple-500" />)
    if (icons.length === 0) return null
    return <span className="flex items-center gap-0.5">{icons}</span>
  }

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-2xl sm:max-h-[90vh] flex flex-col overflow-hidden">
        <DialogHeader>
          <DialogTitle className="flex items-center gap-2">
            <span className={pluginType.color}>{pluginType.icon}</span>
            {isEditing
              ? t("plugins:editInstance", { defaultValue: "Edit Instance" })
              : pluginType.name
            }
          </DialogTitle>
          <DialogDescription>
            {isEditing
              ? t("plugins:editInstanceDesc", { defaultValue: "Configure this instance" })
              : pluginType.description
            }
          </DialogDescription>
        </DialogHeader>

        <DialogContentBody className="flex-1 overflow-y-auto px-4 pt-6 pb-4 sm:px-6">
          {/* Instance Name Field (only for create mode) */}
          {!isEditing && (
            <div className="mb-4">
              <Label htmlFor="instance-name">
                {t("plugins:instanceName", { defaultValue: "Instance Name" })}
              </Label>
              <Input
                id="instance-name"
                value={newInstanceName}
                onChange={(e) => setNewInstanceName(e.target.value)}
                placeholder={t("plugins:instanceNamePlaceholder", { defaultValue: "My Instance" })}
                disabled={saving}
                autoFocus
              />
            </div>
          )}

          {/* Edit Mode: Show instance info */}
          {isEditing && (
            <div className="flex items-center justify-between mb-4 p-3 bg-muted/30 rounded-lg">
              <div>
                <h3 className="font-medium">{editingInstance.name}</h3>
                <div className="flex items-center gap-2 mt-1">
                  <Badge variant={getInstanceStatus(editingInstance) ? "default" : "secondary"}>
                    {getInstanceStatus(editingInstance)
                      ? t("plugins:active", { defaultValue: "Active" })
                      : t("plugins:inactive", { defaultValue: "Inactive" })
                    }
                  </Badge>
                </div>
              </div>
            </div>
          )}

          {/* Ollama endpoint configuration (only for create mode) */}
          {!isEditing && isOllamaBackend && (
            <div className="mb-4">
              <Label htmlFor="ollama-endpoint">
                {t("plugins:llm.endpoint", { defaultValue: "Ollama Endpoint" })}
              </Label>
              <div className="flex items-center gap-2 mt-1">
                <Input
                  id="ollama-endpoint"
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
            </div>
          )}

          {/* Ollama model selector */}
          {isOllamaBackend && ollamaModels.length > 0 && (
            <div className="mb-4">
              <Label htmlFor="model-select">
                {t("plugins:llm.selectModel", { defaultValue: "Select Model" })}
              </Label>
              <div className="flex items-center gap-2 mt-1">
                <Select value={selectedModel} onValueChange={handleModelChange}>
                  <SelectTrigger id="model-select" className="flex-1">
                    <SelectValue placeholder={t("plugins:llm.selectModelPlaceholder", { defaultValue: "Select a model..." })} />
                  </SelectTrigger>
                  <SelectContent>
                    {ollamaModels.map((model) => (
                      <SelectItem
                        key={model.name}
                        value={model.name}
                        icon={getModelIcon(model)}
                      >
                        {model.name}
                      </SelectItem>
                    ))}
                  </SelectContent>
                </Select>
                {!isEditing && (
                  <Button
                    variant="outline"
                    size="icon"
                    onClick={() => fetchOllamaModels(ollamaEndpoint)}
                    disabled={loadingModels}
                  >
                    <RefreshCw className={cn("h-4 w-4", loadingModels && "animate-spin")} />
                  </Button>
                )}
              </div>
              {selectedModel && renderCapabilityBadges()}
            </div>
          )}

          {/* No models message for Ollama */}
          {isOllamaBackend && ollamaModels.length === 0 && !loadingModels && (
            <div className="mb-4 p-3 bg-muted/30 rounded-lg text-sm text-muted-foreground">
              {t("plugins:llm.noModelsFound", { defaultValue: "No models found. Click refresh to fetch from Ollama." })}
            </div>
          )}

          {/* Capability display for non-Ollama LLM backends */}
          {pluginType.type === "llm_backend" && !isOllamaBackend && (
            <div className="mb-4">
              {renderCapabilityBadges()}
            </div>
          )}

          {/* Config Form */}
          <ConfigFormBuilder
            schema={schema}
            onSubmit={isEditing ? handleUpdate : (values) => handleCreate(values)}
            loading={saving}
            submitLabel={isEditing
              ? t("common:save", { defaultValue: "Save" })
              : t("common:create", { defaultValue: "Create" })
            }
          />
        </DialogContentBody>
      </DialogContent>
    </Dialog>
  )
}
