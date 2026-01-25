import { useState, useEffect, useCallback } from "react"
import { useTranslation } from "react-i18next"
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog"
import { Button } from "@/components/ui/button"
import { Input } from "@/components/ui/input"
import { Label } from "@/components/ui/label"
import { Badge } from "@/components/ui/badge"
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select"
import { TestTube, Check, X, Plus, Trash2, RefreshCw, Eye, Brain, Wrench } from "lucide-react"
import { ConfigFormBuilder } from "@/components/plugins/ConfigFormBuilder"
import { useToast } from "@/hooks/use-toast"
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

export function UniversalPluginConfigDialog({
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
}: UniversalPluginConfigDialogProps) {
  const { t } = useTranslation(["common", "plugins", "devices"])
  const { toast } = useToast()

  const [saving, setSaving] = useState(false)
  const [testingId, setTestingId] = useState<string | null>(null)
  const [showCreateForm, setShowCreateForm] = useState(false)
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
      console.error("Failed to fetch Ollama models:", error)
      // Don't show toast for this error - it's optional functionality
    } finally {
      setLoadingModels(false)
    }
  }, [isOllamaBackend, editingInstance])

  // Reset form when dialog opens or plugin type changes
  useEffect(() => {
    if (open) {
      setShowCreateForm(false)
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
      setShowCreateForm(false)
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

  // Handle delete instance
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

  // Handle test connection
  const handleTest = async (instance: PluginInstance) => {
    if (!onTest) return

    setTestingId(instance.id)
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
    } finally {
      setTestingId(null)
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
      <DialogContent className="max-w-2xl max-h-[90vh] overflow-y-auto">
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

        <div className="space-y-4">
          {/* Edit Mode */}
          {isEditing ? (
            <div>
              <div className="flex items-center justify-between mb-4">
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
                {onTest && (
                  <Button
                    variant="outline"
                    size="sm"
                    onClick={() => handleTest(editingInstance)}
                    disabled={testingId === editingInstance.id}
                  >
                    <TestTube className="h-4 w-4 mr-1" />
                    {testingId === editingInstance.id
                      ? t("common:testing", { defaultValue: "Testing..." })
                      : t("plugins:test", { defaultValue: "Test" })
                    }
                  </Button>
                )}
              </div>

              {/* Show test result if available */}
              {testResults[editingInstance.id] && (
                <div className={cn(
                  "text-xs p-2 rounded mb-4 flex items-center gap-1",
                  testResults[editingInstance.id].success
                    ? "bg-green-50 text-green-700 dark:bg-green-900 dark:text-green-300"
                    : "bg-red-50 text-red-700 dark:bg-red-900 dark:text-red-300"
                )}>
                  {testResults[editingInstance.id].success
                    ? <Check className="h-3 w-3" />
                    : <X className="h-3 w-3" />
                  }
                  {testResults[editingInstance.id].message}
                </div>
              )}

              {/* Ollama model selector for editing */}
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
                </div>
              )}

              {/* Capability display for non-Ollama LLM backends */}
              {pluginType.type === "llm_backend" && !isOllamaBackend && renderCapabilityBadges()}

              <ConfigFormBuilder
                schema={schema}
                onSubmit={handleUpdate}
                loading={saving}
                submitLabel={t("common:save", { defaultValue: "Save" })}
              />
            </div>
          ) : (
            <>
              {/* Instance List Mode */}
              <div>
                <div className="flex items-center justify-between mb-2">
                  <h3 className="font-medium">
                    {t("plugins:instances", { defaultValue: "Instances" })} ({instances.length})
                  </h3>
                  {pluginType.can_add_multiple ? (
                    <Button
                      size="sm"
                      variant="outline"
                      onClick={() => setShowCreateForm(!showCreateForm)}
                    >
                      <Plus className="mr-2 h-4 w-4" />
                      {t("plugins:addInstance", { defaultValue: "Add Instance" })}
                    </Button>
                  ) : instances.length === 0 ? (
                    <Button
                      size="sm"
                      variant="outline"
                      onClick={() => setShowCreateForm(!showCreateForm)}
                    >
                      <Plus className="mr-2 h-4 w-4" />
                      {t("plugins:configure", { defaultValue: "Configure" })}
                    </Button>
                  ) : null}
                </div>

                {instances.length === 0 ? (
                  <div className="text-center py-8 border rounded-lg bg-muted/30">
                    <p className="text-sm text-muted-foreground">
                      {t("plugins:noInstances", { defaultValue: "No instances configured" })}
                    </p>
                  </div>
                ) : (
                  <div className="space-y-2">
                    {instances.map((instance) => {
                      const testResult = testResults[instance.id]
                      const isActive = getInstanceStatus(instance)
                      const instanceCaps = (instance.config as any)?.capabilities

                      return (
                        <div
                          key={instance.id}
                          className="flex items-center justify-between p-3 border rounded-lg bg-background"
                        >
                          <div className="flex-1">
                            <div className="flex items-center gap-2">
                              <span className="font-medium">{instance.name}</span>
                              <Badge variant={isActive ? "default" : "secondary"}>
                                {isActive
                                  ? t("plugins:active", { defaultValue: "Active" })
                                  : t("plugins:inactive", { defaultValue: "Inactive" })
                                }
                              </Badge>
                            </div>
                            {testResult && (
                              <div className={cn(
                                "text-xs mt-1 flex items-center gap-1",
                                testResult.success ? "text-green-500" : "text-red-500"
                              )}>
                                {testResult.success ? <Check className="h-3 w-3" /> : <X className="h-3 w-3" />}
                                {testResult.message}
                              </div>
                            )}
                            {instance.config?.model != null && (
                              <div className="text-xs text-muted-foreground mt-1">
                                Model: {String(instance.config.model)}
                              </div>
                            )}
                            {/* Show instance capabilities */}
                            {instanceCaps && (
                              <div className="flex flex-wrap gap-1 mt-1">
                                {instanceCaps.supports_multimodal && (
                                  <Badge variant="outline" className="text-xs h-5 px-1">
                                    <Eye className="h-2.5 w-2.5 mr-0.5" />
                                    Vision
                                  </Badge>
                                )}
                                {instanceCaps.supports_thinking && (
                                  <Badge variant="outline" className="text-xs h-5 px-1">
                                    <Brain className="h-2.5 w-2.5 mr-0.5" />
                                    Thinking
                                  </Badge>
                                )}
                              </div>
                            )}
                          </div>
                          <div className="flex items-center gap-2">
                            {onTest && (
                              <Button
                                variant="ghost"
                                size="sm"
                                onClick={() => handleTest(instance)}
                                disabled={testingId === instance.id}
                              >
                                <TestTube className="h-4 w-4" />
                              </Button>
                            )}
                            {onDelete && (
                              <Button
                                variant="ghost"
                                size="icon"
                                className="text-destructive hover:text-destructive"
                                onClick={() => handleDelete(instance)}
                              >
                                <Trash2 className="h-4 w-4" />
                              </Button>
                            )}
                          </div>
                        </div>
                      )
                    })}
                  </div>
                )}
              </div>

              {/* Create New Instance Form */}
              {showCreateForm && (
                <div className="border-t pt-4 space-y-4">
                  <div>
                    <Label htmlFor="instance-name">
                      {t("plugins:instanceName", { defaultValue: "Instance Name" })}
                    </Label>
                    <Input
                      id="instance-name"
                      value={newInstanceName}
                      onChange={(e) => setNewInstanceName(e.target.value)}
                      placeholder={t("plugins:instanceNamePlaceholder", { defaultValue: "My Instance" })}
                      disabled={saving}
                    />
                  </div>

                  {/* Ollama model selector */}
                  {isOllamaBackend && (
                    <>
                      <div>
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

                      {ollamaModels.length > 0 && (
                        <div>
                          <Label htmlFor="model-select-create">
                            {t("plugins:llm.selectModel", { defaultValue: "Select Model" })}
                          </Label>
                          <Select value={selectedModel} onValueChange={handleModelChange}>
                            <SelectTrigger id="model-select-create" className="mt-1">
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
                          {selectedModel && renderCapabilityBadges()}
                        </div>
                      )}

                      {ollamaModels.length === 0 && !loadingModels && (
                        <p className="text-xs text-muted-foreground">
                          {t("plugins:llm.noModelsFound", { defaultValue: "No models found. Click refresh to fetch from Ollama." })}
                        </p>
                      )}
                    </>
                  )}

                  {/* Capability display for non-Ollama LLM backends */}
                  {pluginType.type === "llm_backend" && !isOllamaBackend && renderCapabilityBadges()}

                  <ConfigFormBuilder
                    schema={schema}
                    onSubmit={handleCreate}
                    loading={saving}
                    submitLabel={t("common:create", { defaultValue: "Create" })}
                  />
                </div>
              )}
            </>
          )}
        </div>

        <DialogFooter>
          <Button variant="outline" onClick={() => onOpenChange(false)}>
            {t("common:close", { defaultValue: "Close" })}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}
