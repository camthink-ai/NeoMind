import { useEffect, useState } from 'react'
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

// LLM Provider info
const LLM_PROVIDER_INFO: Record<string, {
  name: string
  icon: React.ReactNode
  iconBg: string
  iconColor: string
  description: string
}> = {
  ollama: {
    name: 'Ollama',
    icon: <Server className="h-6 w-6" />,
    iconBg: 'bg-green-100 text-green-700 dark:bg-green-900/20 dark:text-green-400',
    iconColor: 'text-green-600',
    description: '本地开源大模型',
  },
  openai: {
    name: 'OpenAI',
    icon: <Server className="h-6 w-6" />,
    iconBg: 'bg-emerald-100 text-emerald-700 dark:bg-emerald-900/20 dark:text-emerald-400',
    iconColor: 'text-emerald-600',
    description: 'GPT 系列 API',
  },
  anthropic: {
    name: 'Anthropic',
    icon: <Server className="h-6 w-6" />,
    iconBg: 'bg-orange-100 text-orange-700 dark:bg-orange-900/20 dark:text-orange-400',
    iconColor: 'text-orange-600',
    description: 'Claude 系列 API',
  },
  google: {
    name: 'Google',
    icon: <Server className="h-6 w-6" />,
    iconBg: 'bg-blue-100 text-blue-700 dark:bg-blue-900/20 dark:text-blue-400',
    iconColor: 'text-blue-600',
    description: 'Gemini 系列 API',
  },
  xai: {
    name: 'xAI',
    icon: <Server className="h-6 w-6" />,
    iconBg: 'bg-gray-100 text-gray-700 dark:bg-gray-900/20 dark:text-gray-400',
    iconColor: 'text-gray-600',
    description: 'Grok 系列 API',
  },
}

export function LLMBackendsTab({
  onCreateBackend,
  onUpdateBackend,
  onDeleteBackend,
  onTestBackend,
}: LlmBackendsTabProps) {
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
      toast({ title: '已激活', description: 'LLM 后端已设为活跃' })
    } catch (error) {
      toast({ title: '激活失败', description: (error as Error).message, variant: 'destructive' })
    }
  }

  const handleDelete = async (id: string) => {
    if (!confirm('确定要删除这个 LLM 后端实例吗？')) return
    try {
      await onDeleteBackend(id)
      await loadData()
      toast({ title: '已删除', description: 'LLM 后端实例已删除' })
    } catch (error) {
      toast({ title: '删除失败', description: (error as Error).message, variant: 'destructive' })
    }
  }

  const handleTest = async (id: string) => {
    try {
      const result = await onTestBackend(id)
      setTestResults(prev => ({ ...prev, [id]: result }))
      if (result.success) {
        toast({ title: '连接成功', description: `延迟: ${result.latency_ms?.toFixed(0)}ms` })
      } else {
        toast({ title: '连接失败', description: result.error, variant: 'destructive' })
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
            <h2 className="text-2xl font-bold tracking-tight">LLM 后端</h2>
            <p className="text-muted-foreground text-sm">
              管理多个 LLM 后端，支持运行时切换
            </p>
          </div>
        </div>

        {/* Provider Cards Grid */}
        <div className="grid gap-4 md:grid-cols-4">
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
                    <span className="text-muted-foreground">状态:</span>
                    <span className={hasActive ? "text-green-600 font-medium" : "text-muted-foreground font-medium"}>
                      {hasActive ? "运行中" : "未配置"}
                    </span>
                  </div>
                  <div className="flex justify-between items-center mt-2">
                    <span className="text-muted-foreground">实例:</span>
                    <span className="font-medium">{typeInstances.length} 个</span>
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
                toast({ title: '保存成功', description: 'LLM 后端配置已保存' })
              } catch (error) {
                toast({ title: '保存失败', description: (error as Error).message, variant: 'destructive' })
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
            返回
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
              添加实例
            </Button>
          </div>
        </div>

        {/* Type Info Badges */}
        <div className="flex flex-wrap gap-2 text-sm mb-4">
          {selectedType.requires_api_key && (
            <Badge variant="outline" className="text-orange-600 border-orange-600">
              需要 API Key
            </Badge>
          )}
          {selectedType.supports_streaming && (
            <Badge variant="outline">流式输出</Badge>
          )}
          {selectedType.supports_thinking && (
            <Badge variant="outline">思维链</Badge>
          )}
          {selectedType.supports_multimodal && (
            <Badge variant="outline">多模态</Badge>
          )}
          <Badge variant="outline" className="text-muted-foreground">
            默认模型: {selectedType.default_model}
          </Badge>
        </div>

        {/* Instances List */}
        {typeInstances.length === 0 ? (
          <Card className="border-dashed">
            <CardContent className="flex flex-col items-center justify-center py-12">
              <div className={cn("flex items-center justify-center w-16 h-16 rounded-lg mb-4", info.iconBg)}>
                {info.icon}
              </div>
              <h3 className="text-lg font-semibold mb-1">暂无 {info.name} 实例</h3>
              <p className="text-sm text-muted-foreground mb-4">
                配置 {info.name} 连接以开始使用
              </p>
              <Button onClick={handleCreate}>
                <Plus className="mr-2 h-4 w-4" />
                添加 {info.name} 实例
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
                          {isActive && <Badge variant="default" className="text-xs">活跃</Badge>}
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
                            编辑
                          </DropdownMenuItem>
                          <DropdownMenuItem onClick={() => handleTest(instance.id)}>
                            <TestTube className="mr-2 h-4 w-4" />
                            测试连接
                          </DropdownMenuItem>
                          <DropdownMenuSeparator />
                          <DropdownMenuItem
                            onClick={() => handleDelete(instance.id)}
                            disabled={isActive}
                            className="text-destructive focus:text-destructive"
                          >
                            <Trash2 className="mr-2 h-4 w-4" />
                            删除
                          </DropdownMenuItem>
                        </DropdownMenuContent>
                      </DropdownMenu>
                    </div>
                  </CardHeader>

                  <CardContent className="pb-3">
                    <div className="space-y-2 text-sm">
                      {instance.endpoint && (
                        <div className="flex items-center justify-between">
                          <span className="text-muted-foreground">端点:</span>
                          <span className="font-mono text-xs truncate max-w-[200px]">{instance.endpoint}</span>
                        </div>
                      )}
                      <div className="flex items-center justify-between">
                        <span className="text-muted-foreground">API Key:</span>
                        <span className={cn(
                          "text-xs",
                          instance.api_key_configured ? "text-green-600" : "text-muted-foreground"
                        )}>
                          {instance.api_key_configured ? "已配置" : "未配置"}
                        </span>
                      </div>
                      <div className="flex gap-1 flex-wrap mt-2">
                        {instance.capabilities.supports_streaming && (
                          <Badge variant="outline" className="text-xs">流式</Badge>
                        )}
                        {instance.capabilities.supports_thinking && (
                          <Badge variant="outline" className="text-xs">思维链</Badge>
                        )}
                        {instance.capabilities.supports_multimodal && (
                          <Badge variant="outline" className="text-xs">多模态</Badge>
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
                            ? `连接成功 (${testResult.latency_ms?.toFixed(0)}ms)`
                            : `连接失败: ${testResult.error}`}
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
                        {isActive ? "当前活跃" : "设为活跃"}
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
                toast({ title: '保存成功', description: 'LLM 后端配置已保存' })
              } catch (error) {
                toast({ title: '保存失败', description: (error as Error).message, variant: 'destructive' })
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
  const [name, setName] = useState(editing?.name || '')
  const [endpoint, setEndpoint] = useState(editing?.endpoint || type.default_endpoint || '')
  const [model, setModel] = useState(editing?.model || type.default_model || '')
  const [apiKey, setApiKey] = useState('')
  const [temperature, setTemperature] = useState(editing?.temperature?.toString() || '0.7')
  const [topP, setTopP] = useState(editing?.top_p?.toString() || '0.9')
  const [maxTokens, setMaxTokens] = useState(editing?.max_tokens?.toString() || '2048')

  useEffect(() => {
    if (open) {
      setName(editing?.name || '')
      setEndpoint(editing?.endpoint || type.default_endpoint || '')
      setModel(editing?.model || type.default_model || '')
      setApiKey('')
      setTemperature(editing?.temperature?.toString() || '0.7')
      setTopP(editing?.top_p?.toString() || '0.9')
      setMaxTokens(editing?.max_tokens?.toString() || '2048')
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
        max_tokens: parseInt(maxTokens),
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
        max_tokens: parseInt(maxTokens),
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
            {editing ? "编辑" : "添加"} {info.name} 实例
          </DialogTitle>
          <DialogDescription>
            {editing ? "修改" : "配置"} {info.name} LLM 后端连接信息
          </DialogDescription>
        </DialogHeader>

        <form onSubmit={handleSubmit} className="space-y-4">
          <div>
            <Label htmlFor="name">显示名称 *</Label>
            <Input
              id="name"
              value={name}
              onChange={(e) => setName(e.target.value)}
              placeholder={`我的 ${info.name}`}
              required
            />
          </div>

          <div>
            <Label htmlFor="endpoint">
              API 端点 {!type.requires_api_key && "(可选)"}
            </Label>
            <Input
              id="endpoint"
              value={endpoint}
              onChange={(e) => setEndpoint(e.target.value)}
              placeholder={type.default_endpoint}
            />
          </div>

          <div>
            <Label htmlFor="model">模型名称 *</Label>
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
              <Label htmlFor="apiKey">API Key {editing && "(留空保持不变)"}</Label>
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
              高级设置
            </summary>
            <div className="mt-3 space-y-3">
              <div>
                <Label htmlFor="temperature">Temperature (0-2)</Label>
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
                <Label htmlFor="topP">Top-P (0-1)</Label>
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
                <Label htmlFor="maxTokens">最大 Token 数</Label>
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
              取消
            </Button>
            <Button type="submit" disabled={saving || !name.trim() || !model.trim()}>
              {saving ? "保存中..." : "保存"}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  )
}
