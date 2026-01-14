import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import {
  Plus,
  Loader2,
  MoreVertical,
  Edit,
  Trash2,
  TestTube,
  ArrowLeft,
  Server,
} from 'lucide-react'
import { Card, CardContent, CardDescription, CardHeader, CardTitle, CardFooter } from '@/components/ui/card'
import { Badge } from '@/components/ui/badge'
import { Switch } from '@/components/ui/switch'
import { Button } from '@/components/ui/button'
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu'
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { cn } from '@/lib/utils'
import { fetchAPI } from '@/lib/api'
import { toast } from '@/components/ui/use-toast'
import type {
  LlmBackendInstance,
  BackendTypeDefinition,
  BackendTestResult,
  CreateLlmBackendRequest,
  UpdateLlmBackendRequest,
} from '@/types'

type View = 'list' | 'detail'

interface LlmBackendsTabProps {
  onCreateBackend: (data: CreateLlmBackendRequest) => Promise<string>
  onUpdateBackend: (id: string, data: UpdateLlmBackendRequest) => Promise<boolean>
  onDeleteBackend: (id: string) => Promise<boolean>
  onTestBackend: (id: string) => Promise<BackendTestResult>
}

// LLM Provider info function
function getLlmProviderInfo(t: (key: string) => string): Record<string, {
  name: string
  icon: React.ReactNode
  iconBg: string
  iconColor: string
  description: string
}> {
  return {
    ollama: {
      name: 'Ollama',
      icon: <Server className="h-6 w-6" />,
      iconBg: 'bg-green-100 text-green-700 dark:bg-green-900/20 dark:text-green-400',
      iconColor: 'text-green-600',
      description: t('plugins:llm.localOpenSource'),
    },
    openai: {
      name: 'OpenAI',
      icon: <Server className="h-6 w-6" />,
      iconBg: 'bg-emerald-100 text-emerald-700 dark:bg-emerald-900/20 dark:text-emerald-400',
      iconColor: 'text-emerald-600',
      description: t('plugins:llm.openaiGpt'),
    },
    anthropic: {
      name: 'Anthropic',
      icon: <Server className="h-6 w-6" />,
      iconBg: 'bg-orange-100 text-orange-700 dark:bg-orange-900/20 dark:text-orange-400',
      iconColor: 'text-orange-600',
      description: t('plugins:llm.anthropicClaude'),
    },
    google: {
      name: 'Google',
      icon: <Server className="h-6 w-6" />,
      iconBg: 'bg-blue-100 text-blue-700 dark:bg-blue-900/20 dark:text-blue-400',
      iconColor: 'text-blue-600',
      description: t('plugins:llm.googleGemini'),
    },
    xai: {
      name: 'xAI',
      icon: <Server className="h-6 w-6" />,
      iconBg: 'bg-gray-100 text-gray-700 dark:bg-gray-900/20 dark:text-gray-400',
      iconColor: 'text-gray-600',
      description: t('plugins:llm.xaiGrok'),
    },
  }
}

export function LLMBackendsTab({
  onCreateBackend,
  onUpdateBackend,
  onDeleteBackend,
  onTestBackend,
}: LlmBackendsTabProps) {
  const { t } = useTranslation(['plugins', 'common'])
  const LLM_PROVIDER_INFO = getLlmProviderInfo(t)
  const [view, setView] = useState<View>('list')
  const [loading, setLoading] = useState(true)
  const [backendTypes, setBackendTypes] = useState<BackendTypeDefinition[]>([])
  const [instances, setInstances] = useState<LlmBackendInstance[]>([])
  const [testResults, setTestResults] = useState<Record<string, BackendTestResult>>({})
  const [activeBackendId, setActiveBackendId] = useState<string | null>(null)
  const [selectedType, setSelectedType] = useState<BackendTypeDefinition | null>(null)

  // Config dialog state
  const [configDialogOpen, setConfigDialogOpen] = useState(false)
  const [editingInstance, setEditingInstance] = useState<LlmBackendInstance | null>(null)
  const [saving, setSaving] = useState(false)

  useEffect(() => {
    loadData()
  }, [])

  const loadData = async () => {
    setLoading(true)
    try {
      const typesResponse = await fetchAPI<{ types: BackendTypeDefinition[] }>('/llm-backends/types')
      setBackendTypes(typesResponse.types || [])

      const instancesResponse = await fetchAPI<{
        backends: LlmBackendInstance[]
        count: number
        active_id: string | null
      }>('/llm-backends')
      setInstances(instancesResponse.backends || [])
      setActiveBackendId(instancesResponse.active_id || null)
    } catch (error) {
      console.error('Failed to load LLM data:', error)
      setBackendTypes([])
      setInstances([])
      setActiveBackendId(null)
    } finally {
      setLoading(false)
    }
  }

  const getInstancesForType = (typeId: string) => {
    return instances.filter(i => i.backend_type === typeId)
  }

  const handleCreate = () => {
    setEditingInstance(null)
    setConfigDialogOpen(true)
  }

  const handleEdit = (instance: LlmBackendInstance) => {
    setEditingInstance(instance)
    setConfigDialogOpen(true)
  }

  const handleActivate = async (id: string) => {
    try {
      await fetchAPI(`/llm-backends/${id}/activate`, { method: 'POST' })
      await loadData()
      toast({ title: t('plugins:llm.activated'), description: t('plugins:llm.activatedDesc') })
    } catch (error) {
      toast({ title: t('plugins:llm.activateFailed'), description: (error as Error).message, variant: 'destructive' })
    }
  }

  const handleDelete = async (id: string) => {
    if (!confirm(t('plugins:llm.deleteConfirm2'))) return
    try {
      await onDeleteBackend(id)
      await loadData()
      toast({ title: t('plugins:llm.deleted'), description: t('plugins:llm.deletedDesc') })
    } catch (error) {
      toast({ title: t('plugins:llm.deleteFailed'), description: (error as Error).message, variant: 'destructive' })
    }
  }

  const handleTest = async (id: string) => {
    try {
      const result = await onTestBackend(id)
      setTestResults(prev => ({ ...prev, [id]: result }))
      if (result.success) {
        toast({ title: t('plugins:llm.connectionSuccess2', { latency: result.latency_ms?.toFixed(0) || '0' }), description: `${t('plugins:llm.latency')}: ${result.latency_ms?.toFixed(0)}ms` })
      } else {
        toast({ title: t('plugins:llm.connectionFailed2', { error: result.error }), description: result.error, variant: 'destructive' })
      }
    } catch (error) {
      const result: BackendTestResult = { success: false, error: (error as Error).message }
      setTestResults(prev => ({ ...prev, [id]: result }))
    }
  }

  if (loading) {
    return (
      <div className="flex items-center justify-center py-12">
        <Loader2 className="h-8 w-8 animate-spin text-muted-foreground" />
      </div>
    )
  }

  // ========== LIST VIEW ==========
  if (view === 'list') {
    return (
      <>
        {/* Header */}
        <div className="flex items-center justify-between mb-4">
          <div>
            <h2 className="text-2xl font-bold tracking-tight">{t('plugins:llmBackends')}</h2>
            <p className="text-muted-foreground text-sm">
              {t('plugins:llm.manageBackends')}
            </p>
          </div>
        </div>

        {/* Provider Cards Grid */}
        <div className="grid gap-4 grid-cols-1 sm:grid-cols-2 lg:grid-cols-3 xl:grid-cols-4">
          {backendTypes.map((type) => {
            const typeInstances = getInstancesForType(type.id)
            const info = LLM_PROVIDER_INFO[type.id] || LLM_PROVIDER_INFO.ollama
            const activeInstance = typeInstances.find(i => i.id === activeBackendId)
            const hasActive = !!activeInstance

            return (
              <Card
                key={type.id}
                className={cn(
                  "cursor-pointer transition-all duration-200 hover:shadow-md",
                  hasActive && "border-green-500 border-2"
                )}
                onClick={() => {
                  setSelectedType(type)
                  setView('detail')
                }}
              >
                <CardHeader className="pb-3">
                  <div className={cn("flex items-center justify-center w-12 h-12 rounded-lg", info.iconBg)}>
                    {info.icon}
                  </div>
                  <CardTitle className="text-base mt-3">{info.name}</CardTitle>
                  <CardDescription className="mt-1 text-xs">
                    {info.description}
                  </CardDescription>
                </CardHeader>
                <CardContent className="text-sm">
                  <div className="flex justify-between items-center">
                    <span className="text-muted-foreground">{t('plugins:llm.status')}:</span>
                    <span className={hasActive ? "text-success font-medium" : "text-muted-foreground font-medium"}>
                      {hasActive ? t('plugins:llm.running') : t('plugins:llm.notConfigured')}
                    </span>
                  </div>
                  <div className="flex justify-between items-center mt-2">
                    <span className="text-muted-foreground">{t('plugins:llm.instances')}:</span>
                    <span className="font-medium">{t('plugins:llm.instancesCount', { count: typeInstances.length })}</span>
                  </div>
                </CardContent>
              </Card>
            )
          })}
        </div>

        {/* Config Dialog */}
        {configDialogOpen && (
          <LLMConfigDialog
            open={configDialogOpen}
            onClose={() => {
              setConfigDialogOpen(false)
              setEditingInstance(null)
            }}
            type={backendTypes[0]}
            editing={editingInstance}
            onSave={async (isEdit, data) => {
              setSaving(true)
              try {
                if (isEdit && editingInstance) {
                  await onUpdateBackend(editingInstance.id, data as UpdateLlmBackendRequest)
                } else {
                  await onCreateBackend(data as CreateLlmBackendRequest)
                }
                await loadData()
                setConfigDialogOpen(false)
                setEditingInstance(null)
                toast({ title: t('plugins:llm.saveSuccess'), description: t('plugins:llm.saveSuccessDesc') })
              } catch (error) {
                toast({ title: t('plugins:llm.saveFailed'), description: (error as Error).message, variant: 'destructive' })
              } finally {
                setSaving(false)
              }
            }}
            saving={saving}
          />
        )}
      </>
    )
  }

  // ========== DETAIL VIEW ==========
  if (view === 'detail' && selectedType) {
    const typeInstances = getInstancesForType(selectedType.id)
    const info = LLM_PROVIDER_INFO[selectedType.id] || LLM_PROVIDER_INFO.ollama

    return (
      <>
        {/* Header with back button */}
        <div className="flex items-center gap-4 mb-4">
          <Button variant="ghost" size="sm" onClick={() => setView('list')} className="gap-1">
            <ArrowLeft className="h-4 w-4" />
            {t('plugins:llm.back')}
          </Button>
          <div className="flex items-center gap-3">
            <div className={cn("flex items-center justify-center w-10 h-10 rounded-lg", info.iconBg)}>
              {info.icon}
            </div>
            <div>
              <h2 className="text-2xl font-bold">{info.name}</h2>
              <p className="text-sm text-muted-foreground">{info.description}</p>
            </div>
          </div>
          <div className="ml-auto">
            <Button onClick={handleCreate}>
              <Plus className="mr-2 h-4 w-4" />
              {t('plugins:llm.addInstance')}
            </Button>
          </div>
        </div>

        {/* Type Info Badges */}
        <div className="flex flex-wrap gap-2 text-sm mb-4">
          {selectedType.requires_api_key && (
            <Badge variant="outline" className="text-warning border-warning">
              {t('plugins:llm.requiresApiKey')}
            </Badge>
          )}
          {selectedType.supports_streaming && (
            <Badge variant="outline">{t('plugins:llm.streamingOutput')}</Badge>
          )}
          {selectedType.supports_thinking && (
            <Badge variant="outline">{t('plugins:llm.thinking')}</Badge>
          )}
          {selectedType.supports_multimodal && (
            <Badge variant="outline">{t('plugins:llm.multimodal')}</Badge>
          )}
          <Badge variant="outline" className="text-muted-foreground">
            {t('plugins:llm.defaultModel')}: {selectedType.default_model}
          </Badge>
        </div>

        {/* Instances List */}
        {typeInstances.length === 0 ? (
          <Card className="border-dashed">
            <CardContent className="flex flex-col items-center justify-center py-12">
              <div className={cn("flex items-center justify-center w-16 h-16 rounded-lg mb-4", info.iconBg)}>
                {info.icon}
              </div>
              <h3 className="text-lg font-semibold mb-1">{t('plugins:llm.noInstanceYet', { name: info.name })}</h3>
              <p className="text-sm text-muted-foreground mb-4">
                {t('plugins:llm.configureToStart', { name: info.name })}
              </p>
              <Button onClick={handleCreate}>
                <Plus className="mr-2 h-4 w-4" />
                {t('plugins:llm.addInstance2', { name: info.name })}
              </Button>
            </CardContent>
          </Card>
        ) : (
          <div className="grid gap-4 md:grid-cols-2">
            {typeInstances.map((instance) => {
              const isActive = instance.id === activeBackendId
              const testResult = testResults[instance.id]

              return (
                <Card
                  key={instance.id}
                  className={cn(
                    "transition-all duration-200",
                    isActive && "border-green-500"
                  )}
                >
                  <CardHeader className="pb-3">
                    <div className="flex items-start justify-between">
                      <div className="flex-1">
                        <div className="flex items-center gap-2 mb-1">
                          <CardTitle className="text-base">{instance.name}</CardTitle>
                          {isActive && <Badge variant="default" className="text-xs">{t('plugins:llm.active')}</Badge>}
                        </div>
                        <CardDescription className="font-mono text-xs">{instance.model}</CardDescription>
                      </div>
                      <DropdownMenu>
                        <DropdownMenuTrigger asChild>
                          <Button variant="ghost" size="sm" className="h-8 w-8 p-0">
                            <MoreVertical className="h-4 w-4" />
                          </Button>
                        </DropdownMenuTrigger>
                        <DropdownMenuContent align="end">
                          <DropdownMenuItem onClick={() => handleEdit(instance)}>
                            <Edit className="mr-2 h-4 w-4" />
                            {t('plugins:llm.edit')}
                          </DropdownMenuItem>
                          <DropdownMenuItem onClick={() => handleTest(instance.id)}>
                            <TestTube className="mr-2 h-4 w-4" />
                            {t('plugins:llm.testConnection2')}
                          </DropdownMenuItem>
                          <DropdownMenuSeparator />
                          <DropdownMenuItem
                            onClick={() => handleDelete(instance.id)}
                            disabled={isActive}
                            className="text-destructive focus:text-destructive"
                          >
                            <Trash2 className="mr-2 h-4 w-4" />
                            {t('plugins:llm.delete')}
                          </DropdownMenuItem>
                        </DropdownMenuContent>
                      </DropdownMenu>
                    </div>
                  </CardHeader>

                  <CardContent className="pb-3">
                    <div className="space-y-2 text-sm">
                      {instance.endpoint && (
                        <div className="flex items-center justify-between">
                          <span className="text-muted-foreground">{t('plugins:llm.endpoint')}:</span>
                          <span className="font-mono text-xs truncate max-w-[200px]">{instance.endpoint}</span>
                        </div>
                      )}
                      <div className="flex items-center justify-between">
                        <span className="text-muted-foreground">API Key:</span>
                        <span className={cn(
                          "text-xs",
                          instance.api_key_configured ? "text-green-600" : "text-muted-foreground"
                        )}>
                          {instance.api_key_configured ? t('plugins:llm.configured') : t('plugins:llm.notConfigured2')}
                        </span>
                      </div>
                      <div className="flex gap-1 flex-wrap mt-2">
                        {instance.capabilities.supports_streaming && (
                          <Badge variant="outline" className="text-xs">{t('plugins:llm.streaming')}</Badge>
                        )}
                        {instance.capabilities.supports_thinking && (
                          <Badge variant="outline" className="text-xs">{t('plugins:llm.thinking')}</Badge>
                        )}
                        {instance.capabilities.supports_multimodal && (
                          <Badge variant="outline" className="text-xs">{t('plugins:llm.multimodal')}</Badge>
                        )}
                      </div>
                      {testResult && (
                        <div className={cn(
                          "text-xs p-2 rounded mt-2",
                          testResult.success
                            ? "bg-green-50 text-green-700 dark:bg-green-900 dark:text-green-300"
                            : "bg-red-50 text-red-700 dark:bg-red-900 dark:text-red-300"
                        )}>
                          {testResult.success
                            ? t('plugins:llm.connectionSuccess2', { latency: testResult.latency_ms?.toFixed(0) || '0' })
                            : t('plugins:llm.connectionFailed2', { error: testResult.error })}
                        </div>
                      )}
                    </div>
                  </CardContent>

                  <CardFooter className="pt-3 border-t">
                    <div className="flex items-center gap-2">
                      <Switch
                        checked={isActive}
                        onCheckedChange={() => handleActivate(instance.id)}
                        disabled={isActive}
                      />
                      <span className="text-sm text-muted-foreground">
                        {isActive ? t('plugins:llm.currentActive') : t('plugins:llm.setAsActive')}
                      </span>
                    </div>
                  </CardFooter>
                </Card>
              )
            })}
          </div>
        )}

        {/* Config Dialog */}
        {configDialogOpen && (
          <LLMConfigDialog
            open={configDialogOpen}
            onClose={() => {
              setConfigDialogOpen(false)
              setEditingInstance(null)
            }}
            type={selectedType}
            editing={editingInstance}
            onSave={async (isEdit, data) => {
              setSaving(true)
              try {
                if (isEdit && editingInstance) {
                  await onUpdateBackend(editingInstance.id, data as UpdateLlmBackendRequest)
                } else {
                  await onCreateBackend(data as CreateLlmBackendRequest)
                }
                await loadData()
                setConfigDialogOpen(false)
                setEditingInstance(null)
                toast({ title: t('plugins:llm.saveSuccess'), description: t('plugins:llm.saveSuccessDesc') })
              } catch (error) {
                toast({ title: t('plugins:llm.saveFailed'), description: (error as Error).message, variant: 'destructive' })
              } finally {
                setSaving(false)
              }
            }}
            saving={saving}
          />
        )}
      </>
    )
  }

  return null
}

// LLM Config Dialog
interface LLMConfigDialogProps {
  open: boolean
  onClose: () => void
  type: BackendTypeDefinition
  editing: LlmBackendInstance | null
  onSave: (isEdit: boolean, data: CreateLlmBackendRequest | UpdateLlmBackendRequest) => Promise<void>
  saving: boolean
}

function LLMConfigDialog({
  open,
  onClose,
  type,
  editing,
  onSave,
  saving,
}: LLMConfigDialogProps) {
  const { t } = useTranslation(['plugins', 'common'])
  const LLM_PROVIDER_INFO = getLlmProviderInfo(t)

  const [name, setName] = useState(editing?.name || '')
  const [endpoint, setEndpoint] = useState(editing?.endpoint || type.default_endpoint || '')
  const [model, setModel] = useState(editing?.model || type.default_model || '')
  const [apiKey, setApiKey] = useState('')
  const [temperature, setTemperature] = useState(editing?.temperature?.toString() || '0.7')
  const [topP, setTopP] = useState(editing?.top_p?.toString() || '0.9')
  const [maxTokens, setMaxTokens] = useState(editing?.max_tokens?.toString() || '')

  useEffect(() => {
    if (open) {
      setName(editing?.name || '')
      setEndpoint(editing?.endpoint || type.default_endpoint || '')
      setModel(editing?.model || type.default_model || '')
      setApiKey('')
      setTemperature(editing?.temperature?.toString() || '0.7')
      setTopP(editing?.top_p?.toString() || '0.9')
      setMaxTokens(editing?.max_tokens?.toString() || '')
    }
  }, [open, editing, type])

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault()

    const isEdit = !!editing

    if (isEdit) {
      const updateData: UpdateLlmBackendRequest = {
        name: name.trim(),
        endpoint: endpoint.trim() || undefined,
        model: model.trim(),
        api_key: apiKey.trim() || undefined,
        temperature: parseFloat(temperature),
        top_p: parseFloat(topP),
        max_tokens: maxTokens ? parseInt(maxTokens) : undefined,
      }
      await onSave(true, updateData)
    } else {
      const createData: CreateLlmBackendRequest = {
        name: name.trim(),
        backend_type: type.id as any,
        endpoint: endpoint.trim() || undefined,
        model: model.trim(),
        api_key: apiKey.trim() || undefined,
        temperature: parseFloat(temperature),
        top_p: parseFloat(topP),
        max_tokens: maxTokens ? parseInt(maxTokens) : undefined,
      }
      await onSave(false, createData)
    }
  }

  const info = LLM_PROVIDER_INFO[type.id] || LLM_PROVIDER_INFO.ollama

  if (!open) return null

  return (
    <Dialog open={open} onOpenChange={onClose}>
      <DialogContent className="max-w-md">
        <DialogHeader>
          <DialogTitle className="flex items-center gap-2">
            <div className={cn("p-2 rounded-lg", info.iconBg)}>
              {info.icon}
            </div>
            {t('plugins:llm.editInstance', { name: info.name })}
          </DialogTitle>
          <DialogDescription>
            {t('plugins:llm.configureInstance', { name: info.name })}
          </DialogDescription>
        </DialogHeader>

        <form onSubmit={handleSubmit} className="space-y-4">
          <div>
            <Label htmlFor="name">{t('plugins:llm.displayName')} *</Label>
            <Input
              id="name"
              value={name}
              onChange={(e) => setName(e.target.value)}
              placeholder={t('plugins:llm.myName', { name: info.name })}
              required
            />
          </div>

          <div>
            <Label htmlFor="endpoint">
              {t('plugins:llm.apiEndpoint')} {!type.requires_api_key && t('plugins:llm.endpointOptional')}
            </Label>
            <Input
              id="endpoint"
              value={endpoint}
              onChange={(e) => setEndpoint(e.target.value)}
              placeholder={type.default_endpoint}
            />
          </div>

          <div>
            <Label htmlFor="model">{t('plugins:llm.modelName')} *</Label>
            <Input
              id="model"
              value={model}
              onChange={(e) => setModel(e.target.value)}
              placeholder={type.default_model}
              required
            />
          </div>

          {type.requires_api_key && (
            <div>
              <Label htmlFor="apiKey">{t('plugins:llm.apiKey')} {editing && t('plugins:llm.apiKeyKeep2')}</Label>
              <Input
                id="apiKey"
                type="password"
                value={apiKey}
                onChange={(e) => setApiKey(e.target.value)}
                placeholder="sk-..."
              />
            </div>
          )}

          <details className="pt-2">
            <summary className="cursor-pointer text-sm text-muted-foreground hover:text-foreground">
              {t('plugins:llm.advancedSettings')}
            </summary>
            <div className="mt-3 space-y-3">
              <div>
                <Label htmlFor="temperature">{t('plugins:llm.temperature')} (0-2)</Label>
                <Input
                  id="temperature"
                  type="number"
                  step="0.1"
                  min="0"
                  max="2"
                  value={temperature}
                  onChange={(e) => setTemperature(e.target.value)}
                />
              </div>
              <div>
                <Label htmlFor="topP">{t('plugins:llm.topP')} (0-1)</Label>
                <Input
                  id="topP"
                  type="number"
                  step="0.1"
                  min="0"
                  max="1"
                  value={topP}
                  onChange={(e) => setTopP(e.target.value)}
                />
              </div>
              <div>
                <Label htmlFor="maxTokens">{t('plugins:llm.maxTokens')}</Label>
                <Input
                  id="maxTokens"
                  type="number"
                  min="1"
                  value={maxTokens}
                  onChange={(e) => setMaxTokens(e.target.value)}
                />
              </div>
            </div>
          </details>

          <DialogFooter>
            <Button type="button" variant="outline" onClick={onClose} disabled={saving}>
              {t('plugins:llm.cancel')}
            </Button>
            <Button type="submit" disabled={saving || !name.trim() || !model.trim()}>
              {saving ? t('plugins:llm.saving') : t('plugins:llm.save')}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  )
}
