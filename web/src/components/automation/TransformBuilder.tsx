import { useState, useEffect, useCallback } from 'react'
import { useTranslation } from 'react-i18next'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Textarea } from '@/components/ui/textarea'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'
import { Switch } from '@/components/ui/switch'
import { Badge } from '@/components/ui/badge'
import { Tabs, TabsList, TabsTrigger, TabsContent } from '@/components/ui/tabs'
import {
  Loader2,
  Sparkles,
  Code,
  Play,
  CheckCircle2,
  Info,
  TestTube,
} from 'lucide-react'
import { api } from '@/lib/api'
import type { TransformAutomation, TransformScope } from '@/types'
import {
  FullScreenBuilder,
  BuilderSection,
  FormGrid,
  TipCard,
} from './FullScreenBuilder'

interface TransformBuilderProps {
  open: boolean
  onOpenChange: (open: boolean) => void
  transform?: TransformAutomation | null
  devices: Array<{ id: string; name: string; device_type?: string }>
  onSave: (data: Partial<TransformAutomation>) => void
}

type ScopeType = 'global' | 'device_type' | 'device' | 'user'
type GenerationState = 'idle' | 'generating' | 'success' | 'error'
type Tab = 'ai' | 'code' | 'test'

// Example intents for quick start
const EXAMPLE_INTENTS = [
  { zh: '统计 detections 数组中每个 cls 的数量', en: 'Count detections by class' },
  { zh: '计算数组中所有值的平均值', en: 'Calculate average of array values' },
  { zh: '过滤置信度低于 0.5 的检测', en: 'Filter detections with confidence < 0.5' },
  { zh: '16进制字符串转 JSON', en: 'Convert hex string to JSON' },
  { zh: '提取嵌套字段到根级别', en: 'Extract nested fields to root level' },
]

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
  const [generationState, setGenerationState] = useState<GenerationState>('idle')
  const [errorMessage, setErrorMessage] = useState('')

  // Test state
  const [testInput, setTestInput] = useState('')
  const [testOutput, setTestOutput] = useState('')
  const [testRunning, setTestRunning] = useState(false)

  // Tab state
  const [activeTab, setActiveTab] = useState<Tab>('ai')

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
    setGenerationState('idle')
    setErrorMessage('')
    setTestInput('')
    setTestOutput('')
    setActiveTab('ai')
  }, [])

  // Generate code
  const handleGenerateCode = useCallback(async () => {
    if (!intent.trim()) return

    setGenerationState('generating')
    setErrorMessage('')

    try {
      const result = await api.generateTransformCode({
        intent,
        language: t('common:lang', { defaultValue: 'en' }),
      })

      setGeneratedCode(result.js_code)
      setGenerationState('success')

      // Auto-fill name if empty
      if (!name.trim()) {
        setName(result.suggested_name || intent.slice(0, 50))
      }

      // Auto-fill output prefix if default
      if (outputPrefix === 'transform') {
        setOutputPrefix(result.output_prefix || 'transform')
      }

      // Switch to code tab to see the result
      setActiveTab('code')
    } catch (err) {
      setErrorMessage(err instanceof Error ? err.message : String(err))
      setGenerationState('error')
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
  const getValidationMessage = () => {
    if (!name.trim()) return t('automation:validation.nameRequired', { defaultValue: '请输入名称' })
    if (!generatedCode.trim()) return t('automation:validation.codeRequired', { defaultValue: '请生成或输入代码' })
    if (scopeType !== 'global' && !scopeValue.trim()) {
      return t('automation:validation.scopeValueRequired', { defaultValue: '请输入作用域值' })
    }
    return ''
  }

  // Get unique device types
  const deviceTypes = Array.from(new Set(devices.map((d) => d.device_type).filter(Boolean)))

  // Side panel content
  const sidePanelContent = (
    <div className="space-y-4">
      <TipCard
        title={t('automation:tips.transformTitle', { defaultValue: '关于数据转换' })}
        variant="info"
      >
        {t('automation:tips.transformDesc', {
          defaultValue: 'Transform 用于处理设备原始数据，提取有用指标或转换数据格式。',
        })}
      </TipCard>

      {activeTab === 'ai' && (
        <>
          <TipCard
            title={t('automation:tips.aiTitle', { defaultValue: 'AI 生成提示' })}
            variant="success"
          >
            {t('automation:tips.aiTransformDesc', {
              defaultValue: '用自然语言描述你想要的数据转换，AI 会自动生成 JavaScript 代码。',
            })}
          </TipCard>

          <TipCard
            title={t('automation:tips.scopeTitle', { defaultValue: '作用域说明' })}
            variant="info"
          >
            {t('automation:tips.scopeDesc', {
              defaultValue: '选择作用域可以限定 Transform 只处理特定设备或设备类型的数据。',
            })}
          </TipCard>
        </>
      )}

      {activeTab === 'code' && (
        <TipCard
          title={t('automation:tips.codeTitle', { defaultValue: '代码说明' })}
          variant="info"
        >
          {t('automation:tips.codeDesc', {
            defaultValue: 'JavaScript 函数，接收 input 参数，返回处理后的数据。使用 output_prefix 作为输出指标前缀。',
          })}
        </TipCard>
      )}

      {activeTab === 'test' && (
        <TipCard
          title={t('automation:tips.testTitle', { defaultValue: '测试说明' })}
          variant="warning"
        >
          {t('automation:tips.testDesc', {
            defaultValue: '使用示例数据测试你的 Transform 代码，确保输出格式正确。',
          })}
        </TipCard>
      )}
    </div>
  )

  return (
    <FullScreenBuilder
      open={open}
      onClose={() => onOpenChange(false)}
      title={transform
        ? t('automation:editTransform', { defaultValue: '编辑数据转换' })
        : t('automation:createTransform', { defaultValue: '创建数据转换' })
      }
      description={t('automation:transformBuilderDesc', {
        defaultValue: '定义如何处理设备数据，提取有用信息或转换数据格式',
      })}
      icon={<Sparkles className="h-5 w-5 text-purple-500" />}
      headerActions={
        <Badge variant={enabled ? 'default' : 'secondary'} className="text-xs">
          {enabled ? t('common:enabled', { defaultValue: '启用' }) : t('common:disabled', { defaultValue: '禁用' })}
        </Badge>
      }
      sidePanel={{ content: sidePanelContent, title: t('automation:tips', { defaultValue: '提示' }) }}
      isValid={isValid}
      isDirty={true}
      isSaving={generationState === 'generating'}
      saveLabel={t('common:save', { defaultValue: '保存' })}
      onSave={handleSave}
      validationMessage={getValidationMessage()}
    >
      <div className="space-y-6">
        {/* Basic Info Section */}
        <BuilderSection
          title={t('automation:basicInfo', { defaultValue: '基本信息' })}
          icon={<Info className="h-4 w-4 text-muted-foreground" />}
        >
          <FormGrid columns={2}>
            <div className="space-y-2">
              <Label htmlFor="transform-name">{t('automation:name', { defaultValue: '名称' })} *</Label>
              <Input
                id="transform-name"
                value={name}
                onChange={e => setName(e.target.value)}
                placeholder={t('automation:transformNamePlaceholder', { defaultValue: '例如：统计检测数量' })}
              />
            </div>
            <div className="flex items-center gap-2">
              <Switch checked={enabled} onCheckedChange={setEnabled} id="transform-enabled" />
              <Label htmlFor="transform-enabled" className="text-sm cursor-pointer">
                {t('automation:enableTransform', { defaultValue: '启用转换' })}
              </Label>
            </div>
          </FormGrid>

          <div className="space-y-2">
            <Label htmlFor="transform-description">{t('common:description', { defaultValue: '描述' })}</Label>
            <Input
              id="transform-description"
              value={description}
              onChange={e => setDescription(e.target.value)}
              placeholder={t('automation:transformDescPlaceholder', { defaultValue: '描述这个转换的功能' })}
            />
          </div>

          {/* Scope Configuration */}
          <div className="space-y-3 pt-2">
            <Label>{t('automation:scope', { defaultValue: '作用域' })}</Label>
            <div className="flex gap-2">
              <div className="w-40">
                <Select value={scopeType} onValueChange={(v: ScopeType) => setScopeType(v)}>
                  <SelectTrigger>
                    <SelectValue />
                  </SelectTrigger>
                  <SelectContent>
                    <SelectItem value="global">{t('automation:scopes.global', { defaultValue: '全局' })}</SelectItem>
                    <SelectItem value="device_type">{t('automation:scopes.deviceType', { defaultValue: '设备类型' })}</SelectItem>
                    <SelectItem value="device">{t('automation:scopes.device', { defaultValue: '设备' })}</SelectItem>
                    <SelectItem value="user">{t('automation:scopes.user', { defaultValue: '用户' })}</SelectItem>
                  </SelectContent>
                </Select>
              </div>

              {scopeType === 'device_type' && (
                <div className="flex-1">
                  <Select value={scopeValue} onValueChange={setScopeValue}>
                    <SelectTrigger>
                      <SelectValue placeholder={t('automation:selectDeviceType', { defaultValue: '选择设备类型' })} />
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
                <div className="flex-1">
                  <Select value={scopeValue} onValueChange={setScopeValue}>
                    <SelectTrigger>
                      <SelectValue placeholder={t('automation:selectDevice', { defaultValue: '选择设备' })} />
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
                <Input
                  value={scopeValue}
                  onChange={e => setScopeValue(e.target.value)}
                  placeholder={t('automation:userId', { defaultValue: '用户 ID' })}
                  className="flex-1"
                />
              )}

              {scopeType === 'global' && (
                <div className="flex-1 text-sm text-muted-foreground">
                  {t('automation:scopes.globalDesc', { defaultValue: '应用于所有设备数据' })}
                </div>
              )}
            </div>

            <div className="space-y-2">
              <Label htmlFor="output-prefix">{t('automation:outputPrefix', { defaultValue: '输出前缀' })}</Label>
              <Input
                id="output-prefix"
                value={outputPrefix}
                onChange={e => setOutputPrefix(e.target.value)}
                placeholder="transform"
                className="font-mono text-sm"
              />
            </div>
          </div>
        </BuilderSection>

        {/* Main Tabs */}
        <Tabs value={activeTab} onValueChange={(v) => setActiveTab(v as Tab)} className="w-full">
          <TabsList className="grid w-full grid-cols-3">
            <TabsTrigger value="ai" className="gap-2">
              <Sparkles className="h-4 w-4" />
              <span>{t('automation:aiGenerate', { defaultValue: 'AI 生成' })}</span>
            </TabsTrigger>
            <TabsTrigger value="code" className="gap-2">
              <Code className="h-4 w-4" />
              <span>{t('automation:code', { defaultValue: '代码' })}</span>
            </TabsTrigger>
            <TabsTrigger value="test" className="gap-2">
              <TestTube className="h-4 w-4" />
              <span>{t('automation:test', { defaultValue: '测试' })}</span>
            </TabsTrigger>
          </TabsList>

          {/* AI Generate Tab */}
          <TabsContent value="ai" className="mt-6">
            <BuilderSection
              title={t('automation:aiGenerate', { defaultValue: 'AI 智能生成' })}
              description={t('automation:aiTransformDesc', {
                defaultValue: '描述你想要的数据转换，AI 会自动生成代码',
              })}
              icon={<Sparkles className="h-4 w-4 text-purple-500" />}
            >
              <div className="space-y-4">
                <Textarea
                  value={intent}
                  onChange={e => setIntent(e.target.value)}
                  placeholder={t('automation:aiIntentPlaceholder', {
                    defaultValue: '例如：统计 detections 数组中每个 cls 的数量',
                  })}
                  rows={4}
                  className="resize-none"
                />

                <div className="space-y-2">
                  <Label className="text-sm text-muted-foreground">
                    {t('automation:quickSelect', { defaultValue: '快速选择' })}
                  </Label>
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
                        {t('common:lang') === 'zh' ? ex.zh : ex.en}
                      </Button>
                    ))}
                  </div>
                </div>

                <Button
                  className="w-full"
                  onClick={handleGenerateCode}
                  disabled={!intent.trim() || generationState === 'generating'}
                >
                  {generationState === 'generating' ? (
                    <>
                      <Loader2 className="h-4 w-4 mr-2 animate-spin" />
                      {t('automation:generating', { defaultValue: '生成中...' })}
                    </>
                  ) : (
                    <>
                      <Sparkles className="h-4 w-4 mr-2" />
                      {t('automation:generateCode', { defaultValue: '生成代码' })}
                    </>
                  )}
                </Button>

                {errorMessage && (
                  <div className="p-3 bg-destructive/10 border border-destructive/20 rounded-md text-sm text-destructive">
                    {errorMessage}
                  </div>
                )}

                {generationState === 'success' && (
                  <div className="p-3 bg-green-50 dark:bg-green-950 border border-green-200 dark:border-green-800 rounded-md text-sm text-green-900 dark:text-green-100 flex items-center gap-2">
                    <CheckCircle2 className="h-4 w-4" />
                    {t('automation:codeGenerated', { defaultValue: '代码生成成功！切换到"代码"标签页查看和编辑' })}
                  </div>
                )}
              </div>
            </BuilderSection>
          </TabsContent>

          {/* Code Tab */}
          <TabsContent value="code" className="mt-6">
            <BuilderSection
              title={t('automation:transformCode', { defaultValue: '转换代码' })}
              description={t('automation:transformCodeDesc', {
                defaultValue: 'JavaScript 函数，接收 input 参数并返回处理后的数据',
              })}
              icon={<Code className="h-4 w-4 text-muted-foreground" />}
            >
              <div className="space-y-3">
                <Textarea
                  value={generatedCode}
                  onChange={e => setGeneratedCode(e.target.value)}
                  placeholder={`// 示例：统计数组长度
return (input.items || input.detections || []).length;`}
                  rows={16}
                  className="font-mono text-sm"
                  spellCheck={false}
                />

                <div className="flex items-center justify-between text-xs text-muted-foreground">
                  <span>{t('automation:availableVars', { defaultValue: '可用变量: input' })}</span>
                  <span>{generatedCode.split('\n').length} {t('automation:lines', { defaultValue: '行' })}</span>
                </div>
              </div>
            </BuilderSection>
          </TabsContent>

          {/* Test Tab */}
          <TabsContent value="test" className="mt-6">
            <BuilderSection
              title={t('automation:testTransform', { defaultValue: '测试转换' })}
              description={t('automation:testTransformDesc', {
                defaultValue: '使用示例数据测试你的转换代码',
              })}
              icon={<TestTube className="h-4 w-4 text-muted-foreground" />}
            >
              <div className="space-y-4">
                <div className="grid grid-cols-2 gap-4">
                  <div className="space-y-2">
                    <Label>{t('automation:inputData', { defaultValue: '输入数据 (JSON)' })}</Label>
                    <Textarea
                      value={testInput}
                      onChange={e => setTestInput(e.target.value)}
                      placeholder='{"detections": [{"cls": "fish"}, {"cls": "fish"}, {"cls": "shrimp"}]}'
                      rows={8}
                      className="font-mono text-sm"
                    />
                  </div>

                  <div className="space-y-2">
                    <div className="flex items-center justify-between">
                      <Label>{t('automation:outputData', { defaultValue: '输出结果' })}</Label>
                      <Button
                        size="sm"
                        variant="outline"
                        onClick={handleTestCode}
                        disabled={!generatedCode || testRunning}
                      >
                        {testRunning ? (
                          <Loader2 className="h-4 w-4 animate-spin" />
                        ) : (
                          <Play className="h-4 w-4" />
                        )}
                      </Button>
                    </div>
                    <Textarea
                      readOnly
                      value={testOutput || t('automation:testOutputPlaceholder', {
                        defaultValue: '点击运行按钮查看结果...',
                      })}
                      rows={8}
                      className="font-mono text-sm bg-muted/30"
                    />
                  </div>
                </div>
              </div>
            </BuilderSection>
          </TabsContent>
        </Tabs>
      </div>
    </FullScreenBuilder>
  )
}
