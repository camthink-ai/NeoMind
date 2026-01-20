import { useState, useEffect, useCallback } from 'react'
import { useTranslation } from 'react-i18next'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Textarea } from '@/components/ui/textarea'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'
import { Switch } from '@/components/ui/switch'
import { Badge } from '@/components/ui/badge'
import { Sparkles, Loader2, Play } from 'lucide-react'
import { api } from '@/lib/api'
import type { TransformAutomation, TransformScope } from '@/types'
import {
  SplitPaneBuilder,
  FormSection,
} from './SplitPaneBuilder'
import { cn } from '@/lib/utils'

interface TransformBuilderProps {
  open: boolean
  onOpenChange: (open: boolean) => void
  transform?: TransformAutomation | null
  devices: Array<{ id: string; name: string; device_type?: string }>
  onSave: (data: Partial<TransformAutomation>) => void
}

type ScopeType = 'global' | 'device_type' | 'device' | 'user'

// Example intents for quick start
const EXAMPLE_INTENTS = [
  { zh: '统计 detections 数组中每个 cls 的数量', en: 'Count detections by class' },
  { zh: '计算数组中所有值的平均值', en: 'Calculate average of array values' },
  { zh: '过滤置信度低于 0.5 的检测', en: 'Filter detections with confidence < 0.5' },
  { zh: '16进制字符串转 JSON', en: 'Convert hex string to JSON' },
  { zh: '提取嵌套字段到根级别', en: 'Extract nested fields to root level' },
]

// Simple flowchart visualization for Transform
function TransformVisualization({
  scope,
  hasCode
}: {
  scope: TransformScope | undefined
  hasCode: boolean
}) {
  const getScopeIcon = () => {
    if (!scope) return '🌐'
    switch (scope.type) {
      case 'device_type': return '📦'
      case 'device': return '🔌'
      case 'user': return '👤'
      default: return '🌐'
    }
  }

  const getScopeLabel = () => {
    if (!scope) return 'Global'
    switch (scope.type) {
      case 'device_type': return scope.device_type || 'Device Type'
      case 'device': return scope.device_id || 'Device'
      case 'user': return scope.user_id || 'User'
      default: return 'Global'
    }
  }

  return (
    <div className="h-full flex items-center justify-center p-8">
      <div className="max-w-md w-full">
        {/* Flow diagram */}
        <div className="flex flex-col items-center gap-4">
          {/* Input node */}
          <div className="flex items-center gap-3">
            <div className="w-16 h-16 rounded-full bg-blue-100 dark:bg-blue-900/30 border-2 border-blue-300 dark:border-blue-700 flex items-center justify-center text-2xl">
              📥
            </div>
            <div className="text-sm">
              <div className="font-medium">输入数据</div>
              <div className="text-xs text-muted-foreground">input</div>
            </div>
          </div>

          {/* Arrow */}
          <div className="w-0.5 h-8 bg-gradient-to-b from-blue-300 to-purple-300 dark:from-blue-700 dark:to-purple-700" />

          {/* Transform node */}
          <div className="relative">
            <div className="w-24 h-24 rounded-xl bg-gradient-to-br from-purple-100 to-pink-100 dark:from-purple-900/30 dark:to-pink-900/30 border-2 border-purple-300 dark:border-purple-700 flex items-center justify-center flex-col">
              <span className="text-2xl mb-1">⚡</span>
              <span className="text-xs font-medium">Transform</span>
            </div>
            {/* Scope badge */}
            <div className="absolute -bottom-2 left-1/2 -translate-x-1/2">
              <Badge variant="outline" className="text-xs bg-background">
                {getScopeIcon()} {getScopeLabel()}
              </Badge>
            </div>
          </div>

          {/* Arrow */}
          <div className="w-0.5 h-8 bg-gradient-to-b from-purple-300 to-green-300 dark:from-purple-700 dark:to-green-700" />

          {/* Output node */}
          <div className={cn(
            "flex items-center gap-3 transition-opacity duration-300",
            !hasCode && "opacity-40"
          )}>
            <div className="text-sm text-right">
              <div className="font-medium">输出数据</div>
              <div className="text-xs text-muted-foreground">output</div>
            </div>
            <div className={cn(
              "w-16 h-16 rounded-full border-2 flex items-center justify-center text-2xl transition-colors duration-300",
              hasCode
                ? "bg-green-100 dark:bg-green-900/30 border-green-300 dark:border-green-700"
                : "bg-gray-100 dark:bg-gray-800 border-gray-300 dark:border-gray-600"
            )}>
              📤
            </div>
          </div>
        </div>

        {/* Status text */}
        <div className="mt-8 text-center">
          <p className="text-sm text-muted-foreground">
            {hasCode
              ? '✨ Transform 配置完成，数据将按定义规则转换'
              : '⚠️ 请先配置 Transform 规则或使用 AI 生成'
            }
          </p>
        </div>
      </div>
    </div>
  )
}

export function TransformBuilder({
  open,
  onOpenChange,
  transform,
  devices,
  onSave,
}: TransformBuilderProps) {
  const { t } = useTranslation(['automation', 'common'])

  // Form state
  const [name, setName] = useState('')
  const [description, setDescription] = useState('')
  const [enabled, setEnabled] = useState(true)
  const [scopeType, setScopeType] = useState<ScopeType>('global')
  const [scopeValue, setScopeValue] = useState('')
  const [outputPrefix, setOutputPrefix] = useState('transform')

  // AI Generation state
  const [intent, setIntent] = useState('')
  const [generatedCode, setGeneratedCode] = useState('')
  const [generating, setGenerating] = useState(false)
  const [errorMessage, setErrorMessage] = useState('')

  // Test state
  const [testInput, setTestInput] = useState('')
  const [testOutput, setTestOutput] = useState('')
  const [testRunning, setTestRunning] = useState(false)

  // Reset form when transform changes
  useEffect(() => {
    if (open && transform) {
      setName(transform.name)
      setDescription(transform.description)
      setEnabled(transform.enabled)
      setScopeType(transform.scope.type as ScopeType)
      setOutputPrefix(transform.output_prefix || 'transform')
      setIntent(transform.intent || '')
      setGeneratedCode(transform.js_code || '')

      if (transform.scope.type === 'device_type') {
        setScopeValue(transform.scope.device_type)
      } else if (transform.scope.type === 'device') {
        setScopeValue(transform.scope.device_id)
      } else if (transform.scope.type === 'user') {
        setScopeValue(transform.scope.user_id)
      }
    } else if (open) {
      resetForm()
    }
  }, [transform, open])

  const resetForm = useCallback(() => {
    setName('')
    setDescription('')
    setEnabled(true)
    setScopeType('global')
    setScopeValue('')
    setOutputPrefix('transform')
    setIntent('')
    setGeneratedCode('')
    setGenerating(false)
    setErrorMessage('')
    setTestInput('')
    setTestOutput('')
  }, [])

  // Generate code
  const handleGenerateCode = useCallback(async () => {
    if (!intent.trim()) return

    setGenerating(true)
    setErrorMessage('')

    try {
      const result = await api.generateTransformCode({
        intent,
        language: t('common:lang', { defaultValue: 'en' }),
      })

      setGeneratedCode(result.js_code)
      setGenerating(false)

      // Auto-fill name if empty
      if (!name.trim()) {
        setName(result.suggested_name || intent.slice(0, 50))
      }

      // Auto-fill output prefix if default
      if (outputPrefix === 'transform') {
        setOutputPrefix(result.output_prefix || 'transform')
      }
    } catch (err) {
      setErrorMessage(err instanceof Error ? err.message : String(err))
      setGenerating(false)
    }
  }, [intent, name, outputPrefix, t])

  // Test code
  const handleTestCode = useCallback(async () => {
    setTestRunning(true)
    setTestOutput('')

    try {
      const inputData = testInput.trim()
        ? JSON.parse(testInput)
        : { detections: [{ cls: 'fish' }, { cls: 'fish' }, { cls: 'shrimp' }] }

      // Create a function from the generated code
      const fn = new Function('input', generatedCode)
      const result = fn(inputData)
      setTestOutput(JSON.stringify(result, null, 2))
    } catch (err) {
      setTestOutput(`Error: ${err instanceof Error ? err.message : String(err)}`)
    } finally {
      setTestRunning(false)
    }
  }, [generatedCode, testInput])

  // Save
  const handleSave = useCallback(() => {
    if (!name.trim()) return

    const scope: TransformScope = (() => {
      switch (scopeType) {
        case 'global':
          return { type: 'global' }
        case 'device_type':
          return { type: 'device_type', device_type: scopeValue }
        case 'device':
          return { type: 'device', device_id: scopeValue }
        case 'user':
          return { type: 'user', user_id: scopeValue }
      }
    })()

    onSave({
      name,
      description: description || intent,
      enabled,
      scope,
      intent,
      js_code: generatedCode,
      output_prefix: outputPrefix,
      complexity: generatedCode.split('\n').length > 10 ? 3 : 2,
    })
  }, [name, description, enabled, scopeType, scopeValue, intent, generatedCode, outputPrefix, onSave])

  // Validation
  const isValid = Boolean(name.trim() && generatedCode.trim())

  // Get scope for visualization
  const currentScope: TransformScope | undefined = (() => {
    if (!generatedCode) return undefined
    switch (scopeType) {
      case 'global':
        return { type: 'global' }
      case 'device_type':
        return { type: 'device_type', device_type: scopeValue }
      case 'device':
        return { type: 'device', device_id: scopeValue }
      case 'user':
        return { type: 'user', user_id: scopeValue }
    }
  })()

  // Get unique device types
  const deviceTypes = Array.from(new Set(devices.map((d) => d.device_type).filter(Boolean)))

  // Left panel form content
  const leftPanelContent = (
    <div className="space-y-4">
      {/* Basic Info */}
      <FormSection title="基本信息">
        <div className="space-y-3">
          <div>
            <Label htmlFor="transform-name">名称 *</Label>
            <Input
              id="transform-name"
              value={name}
              onChange={e => setName(e.target.value)}
              placeholder="例如：统计检测数量"
              className="mt-1.5"
            />
          </div>
          <div className="flex items-center gap-2">
            <Switch checked={enabled} onCheckedChange={setEnabled} id="transform-enabled" />
            <Label htmlFor="transform-enabled" className="text-sm cursor-pointer">
              启用转换
            </Label>
          </div>
          <div>
            <Label htmlFor="transform-description">描述</Label>
            <Input
              id="transform-description"
              value={description}
              onChange={e => setDescription(e.target.value)}
              placeholder="描述这个转换的功能"
              className="mt-1.5"
            />
          </div>
        </div>
      </FormSection>

      {/* Scope Configuration */}
      <FormSection title="作用域配置" description="限定 Transform 只处理特定数据">
        <div className="space-y-3">
          <div>
            <Label>作用域类型</Label>
            <Select value={scopeType} onValueChange={(v: ScopeType) => setScopeType(v)}>
              <SelectTrigger className="mt-1.5">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value="global">全局 - 处理所有设备数据</SelectItem>
                <SelectItem value="device_type">设备类型 - 只处理特定类型</SelectItem>
                <SelectItem value="device">设备 - 只处理特定设备</SelectItem>
                <SelectItem value="user">用户 - 只处理特定用户</SelectItem>
              </SelectContent>
            </Select>
          </div>

          {scopeType === 'device_type' && (
            <div>
              <Label>选择设备类型</Label>
              <Select value={scopeValue} onValueChange={setScopeValue}>
                <SelectTrigger className="mt-1.5">
                  <SelectValue placeholder="选择设备类型" />
                </SelectTrigger>
                <SelectContent>
                  {deviceTypes.map(dt => (
                    <SelectItem key={dt} value={dt || ''}>{dt}</SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </div>
          )}

          {scopeType === 'device' && (
            <div>
              <Label>选择设备</Label>
              <Select value={scopeValue} onValueChange={setScopeValue}>
                <SelectTrigger className="mt-1.5">
                  <SelectValue placeholder="选择设备" />
                </SelectTrigger>
                <SelectContent>
                  {devices.map(d => (
                    <SelectItem key={d.id} value={d.id}>{d.name}</SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </div>
          )}

          {scopeType === 'user' && (
            <div>
              <Label>用户 ID</Label>
              <Input
                value={scopeValue}
                onChange={e => setScopeValue(e.target.value)}
                placeholder="输入用户 ID"
                className="mt-1.5"
              />
            </div>
          )}

          <div>
            <Label htmlFor="output-prefix">输出前缀</Label>
            <Input
              id="output-prefix"
              value={outputPrefix}
              onChange={e => setOutputPrefix(e.target.value)}
              placeholder="transform"
              className="font-mono text-sm mt-1.5"
            />
          </div>
        </div>
      </FormSection>

      {/* AI Generation */}
      <FormSection title="AI 生成" description="用自然语言描述转换规则，AI 自动生成代码" defaultExpanded={true}>
        <div className="space-y-3">
          <div>
            <Label htmlFor="intent">转换意图</Label>
            <Textarea
              id="intent"
              value={intent}
              onChange={e => setIntent(e.target.value)}
              placeholder="例如：统计 detections 数组中每个 cls 的数量"
              rows={4}
              className="resize-none mt-1.5"
            />
          </div>

          <div className="space-y-2">
            <Label className="text-sm text-muted-foreground">快速选择</Label>
            <div className="flex flex-wrap gap-2">
              {EXAMPLE_INTENTS.map((ex, i) => (
                <Button
                  key={i}
                  variant="outline"
                  size="sm"
                  type="button"
                  onClick={() => setIntent(t('common:lang') === 'zh' ? ex.zh : ex.en)}
                  className="h-8 text-xs"
                >
                  {t('common:lang') === 'zh' ? ex.zh.slice(0, 12) + '...' : ex.en.slice(0, 20) + '...'}
                </Button>
              ))}
            </div>
          </div>

          {errorMessage && (
            <div className="p-3 bg-destructive/10 border border-destructive/20 rounded-md text-sm text-destructive">
              {errorMessage}
            </div>
          )}

          {/* Test section */}
          {generatedCode && (
            <div className="pt-2 border-t">
              <Label>测试代码</Label>
              <div className="mt-2 space-y-2">
                <Textarea
                  value={testInput}
                  onChange={e => setTestInput(e.target.value)}
                  placeholder='测试输入: {"detections": [{"cls": "fish"}, {"cls": "shrimp"}]}'
                  rows={3}
                  className="font-mono text-sm resize-none"
                />
                <div className="flex gap-2">
                  <Button
                    size="sm"
                    onClick={handleTestCode}
                    disabled={!generatedCode || testRunning}
                    className="flex-1"
                  >
                    {testRunning ? (
                      <Loader2 className="h-3.5 w-3.5 mr-1.5 animate-spin" />
                    ) : (
                      <Play className="h-3.5 w-3.5 mr-1.5" />
                    )}
                    {testRunning ? '测试中...' : '运行测试'}
                  </Button>
                  {testOutput && (
                    <Button
                      size="sm"
                      variant="outline"
                      onClick={() => setTestOutput('')}
                    >
                      清除结果
                    </Button>
                  )}
                </div>
                {testOutput && (
                  <div className="mt-2 p-2 bg-muted rounded text-xs font-mono overflow-auto max-h-24">
                    {testOutput}
                  </div>
                )}
              </div>
            </div>
          )}
        </div>
      </FormSection>
    </div>
  )

  return (
    <SplitPaneBuilder
      open={open}
      onClose={() => onOpenChange(false)}
      title={transform ? '编辑数据转换' : '创建数据转换'}
      description="定义如何处理设备数据，提取有用信息或转换数据格式"
      icon={<Sparkles className="h-5 w-5 text-purple-500" />}
      badge={
        <Badge variant={enabled ? 'default' : 'secondary'} className="text-xs">
          {enabled ? '启用' : '禁用'}
        </Badge>
      }
      leftPanel={{
        title: '配置',
        content: leftPanelContent,
      }}
      rightPanel={{
        visualization: <TransformVisualization scope={currentScope} hasCode={!!generatedCode} />,
        code: generatedCode || undefined,
        codeLanguage: 'javascript',
        loading: generating,
        error: errorMessage,
      }}
      isValid={isValid}
      isSaving={false}
      saveLabel="保存"
      onSave={handleSave}
      onGenerate={handleGenerateCode}
      generating={generating}
      generateLabel="AI 生成代码"
    />
  )
}
