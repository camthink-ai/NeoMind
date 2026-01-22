import { useState, useEffect, useCallback } from 'react'
import { useTranslation } from 'react-i18next'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Switch } from '@/components/ui/switch'
import { Badge } from '@/components/ui/badge'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'
import {
  Plus,
  X,
  Zap,
  Bell,
  FileText,
  Trash2,
  Lightbulb,
  Info,
  Clock,
  Globe,
  AlertTriangle,
  Timer,
} from 'lucide-react'
import type { Rule, RuleTrigger, RuleCondition, RuleAction, DeviceType } from '@/types'
import {
  FullScreenBuilder,
  BuilderSection,
  TipCard,
} from './FullScreenBuilder'
import { cn } from '@/lib/utils'

interface RuleBuilderProps {
  open: boolean
  onOpenChange: (open: boolean) => void
  rule?: Rule
  onSave: (rule: Partial<Rule>) => Promise<void>
  resources?: {
    devices: Array<{ id: string; name: string; device_type?: string }>
    deviceTypes?: DeviceType[]
  }
}

// Comparison operators matching backend DSL
const COMPARISON_OPERATORS = [
  { value: '>', label: '大于', symbol: '>' },
  { value: '<', label: '小于', symbol: '<' },
  { value: '>=', label: '大于等于', symbol: '≥' },
  { value: '<=', label: '小于等于', symbol: '≤' },
  { value: '==', label: '等于', symbol: '=' },
  { value: '!=', label: '不等于', symbol: '≠' },
]

// Condition types
type ConditionType = 'simple' | 'range' | 'and' | 'or' | 'not'

// Extended condition with UI support
interface UICondition {
  id: string
  type: ConditionType
  // For simple conditions
  device_id?: string
  metric?: string
  operator?: string
  threshold?: number
  // For range conditions
  range_min?: number
  range_max?: number
  // For logical conditions
  conditions?: UICondition[]
  // FOR clause (duration)
  for_duration?: number
  for_unit?: 'seconds' | 'minutes' | 'hours'
}

// ============================================================================
// Helper Functions
// ============================================================================

function getDeviceType(
  deviceId: string,
  devices: Array<{ id: string; name: string; device_type?: string }>,
  deviceTypes?: DeviceType[]
): string {
  const device = devices.find(d => d.id === deviceId)
  return device?.device_type || deviceTypes?.[0]?.device_type || ''
}

function getDeviceMetrics(
  deviceId: string,
  devices: Array<{ id: string; name: string; device_type?: string }>,
  deviceTypes?: DeviceType[]
): Array<{ name: string; display_name?: string }> {
  const deviceTypeName = getDeviceType(deviceId, devices, deviceTypes)
  const deviceType = deviceTypes?.find(t => t.device_type === deviceTypeName)
  return deviceType?.metrics || []
}

function getDeviceCommands(
  deviceId: string,
  devices: Array<{ id: string; name: string; device_type?: string }>,
  deviceTypes?: DeviceType[]
): Array<{ name: string; display_name?: string }> {
  const deviceTypeName = getDeviceType(deviceId, devices, deviceTypes)
  const deviceType = deviceTypes?.find(t => t.device_type === deviceTypeName)
  return deviceType?.commands || []
}

// Convert UI condition to RuleCondition
function uiConditionToRuleCondition(cond: UICondition): RuleCondition {
  switch (cond.type) {
    case 'simple':
      return {
        device_id: cond.device_id || '',
        metric: cond.metric || 'value',
        operator: cond.operator || '>',
        threshold: cond.threshold || 0,
      }
    case 'range':
      return {
        device_id: cond.device_id || '',
        metric: cond.metric || 'value',
        operator: 'between',
        threshold: cond.range_max || 0,
        range_min: cond.range_min,
      } as RuleCondition
    case 'and':
      return {
        operator: 'and',
        conditions: cond.conditions?.map(uiConditionToRuleCondition) || [],
      } as RuleCondition
    case 'or':
      return {
        operator: 'or',
        conditions: cond.conditions?.map(uiConditionToRuleCondition) || [],
      } as RuleCondition
    case 'not':
      return {
        operator: 'not',
        conditions: cond.conditions?.map(uiConditionToRuleCondition) || [],
      } as RuleCondition
    default:
      return {
        device_id: '',
        metric: 'value',
        operator: '>',
        threshold: 0,
      }
  }
}

// Convert RuleCondition to UI condition
function ruleConditionToUiCondition(ruleCond?: RuleCondition): UICondition {
  if (!ruleCond) {
    return {
      id: crypto.randomUUID(),
      type: 'simple',
      device_id: '',
      metric: 'value',
      operator: '>',
      threshold: 0,
    }
  }

  // Check for logical operators first
  if ('operator' in ruleCond) {
    const op = (ruleCond as any).operator
    if (op === 'and' || op === 'or') {
      return {
        id: crypto.randomUUID(),
        type: op,
        conditions: ((ruleCond as any).conditions || []).map(ruleConditionToUiCondition),
      }
    }
    if (op === 'not') {
      return {
        id: crypto.randomUUID(),
        type: 'not',
        conditions: [(ruleCond as any).conditions?.[0]].map(ruleConditionToUiCondition).filter(Boolean),
      }
    }
  }

  // Check for range condition
  if ('range_min' in ruleCond && (ruleCond as any).range_min !== undefined) {
    return {
      id: crypto.randomUUID(),
      type: 'range',
      device_id: ruleCond.device_id,
      metric: ruleCond.metric,
      range_min: (ruleCond as any).range_min,
      range_max: ruleCond.threshold,
    }
  }

  // Simple condition
  return {
    id: crypto.randomUUID(),
    type: 'simple',
    device_id: ruleCond.device_id,
    metric: ruleCond.metric,
    operator: ruleCond.operator,
    threshold: ruleCond.threshold,
  }
}

// ============================================================================
// Main Component
// ============================================================================

export function SimpleRuleBuilder({
  open,
  onOpenChange,
  rule,
  onSave,
  resources = { devices: [], deviceTypes: [] },
}: RuleBuilderProps) {
  const { t } = useTranslation(['automation', 'common'])

  // Basic info
  const [name, setName] = useState('')
  const [description, setDescription] = useState('')
  const [enabled, setEnabled] = useState(true)

  // Condition state
  const [condition, setCondition] = useState<UICondition | null>(null)
  const [forDuration, setForDuration] = useState<number>(0)
  const [forUnit, setForUnit] = useState<'seconds' | 'minutes' | 'hours'>('minutes')

  // Actions state
  const [actions, setActions] = useState<RuleAction[]>([])

  // Saving state
  const [saving, setSaving] = useState(false)

  // ============================================================================
  // Initialize form from rule
  // ============================================================================
  useEffect(() => {
    if (open && rule) {
      setName(rule.name || '')
      setDescription(rule.description || '')
      setEnabled(rule.enabled ?? true)

      if (rule.condition) {
        setCondition(ruleConditionToUiCondition(rule.condition))
      } else {
        setCondition(null)
      }

      if (rule.actions && rule.actions.length > 0) {
        setActions(rule.actions)
      } else {
        setActions([])
      }
    } else if (open) {
      resetForm()
    }
  }, [open, rule])

  const resetForm = useCallback(() => {
    setName('')
    setDescription('')
    setEnabled(true)
    setCondition(null)
    setForDuration(0)
    setForUnit('minutes')
    setActions([{ type: 'Log', level: 'info', message: t('automation:logMessage', { defaultValue: '规则已触发' }) }])
  }, [t])

  // ============================================================================
  // Condition helpers
  // ============================================================================

  const createDefaultCondition = useCallback((): UICondition => {
    const firstDevice = resources.devices[0]
    if (!firstDevice) {
      return {
        id: crypto.randomUUID(),
        type: 'simple',
        device_id: '',
        metric: 'value',
        operator: '>',
        threshold: 0,
      }
    }
    const metrics = getDeviceMetrics(firstDevice.id, resources.devices, resources.deviceTypes)
    return {
      id: crypto.randomUUID(),
      type: 'simple',
      device_id: firstDevice.id,
      metric: metrics[0]?.name || 'value',
      operator: '>',
      threshold: 0,
    }
  }, [resources.devices, resources.deviceTypes])

  const addCondition = useCallback((type: ConditionType = 'simple') => {
    const newCond: UICondition = {
      id: crypto.randomUUID(),
      type,
      ...(type === 'simple' ? {
        device_id: resources.devices[0]?.id || '',
        metric: getDeviceMetrics(resources.devices[0]?.id || '', resources.devices, resources.deviceTypes)[0]?.name || 'value',
        operator: '>',
        threshold: 0,
      } : type === 'range' ? {
        device_id: resources.devices[0]?.id || '',
        metric: getDeviceMetrics(resources.devices[0]?.id || '', resources.devices, resources.deviceTypes)[0]?.name || 'value',
        range_min: 0,
        range_max: 100,
      } : type === 'and' || type === 'or' ? {
        conditions: [createDefaultCondition(), createDefaultCondition()],
      } : {
        conditions: [createDefaultCondition()],
      }),
    }
    setCondition(newCond)
  }, [resources.devices, resources.deviceTypes, createDefaultCondition])

  const updateCondition = useCallback((updates: Partial<UICondition>) => {
    setCondition(prev => prev ? { ...prev, ...updates } : null)
  }, [])

  const updateNestedCondition = useCallback((path: number[], updates: Partial<UICondition>) => {
    setCondition(prev => {
      if (!prev) return prev

      const updateAtPath = (cond: UICondition, idx: number[]): UICondition => {
        if (idx.length === 0) {
          return { ...cond, ...updates }
        }
        const [first, ...rest] = idx
        if (cond.conditions) {
          return {
            ...cond,
            conditions: cond.conditions.map((c, i) => i === first ? updateAtPath(c, rest) : c),
          }
        }
        return cond
      }

      return updateAtPath(prev, path)
    })
  }, [])

  // ============================================================================
  // Action helpers
  // ============================================================================

  const addAction = useCallback((type: 'Notify' | 'Execute' | 'Log' | 'Set' | 'Delay' | 'CreateAlert' | 'HttpRequest') => {
    setActions(prev => {
      let newAction: RuleAction
      if (type === 'Notify') {
        newAction = { type: 'Notify', message: '' }
      } else if (type === 'Execute') {
        const firstDevice = resources.devices[0]
        const commands = firstDevice ? getDeviceCommands(firstDevice.id, resources.devices, resources.deviceTypes) : []
        newAction = {
          type: 'Execute',
          device_id: firstDevice?.id || '',
          command: commands[0]?.name || 'turn_on',
          params: {},
        }
      } else if (type === 'Set') {
        newAction = {
          type: 'Set',
          device_id: resources.devices[0]?.id || '',
          property: 'state',
          value: true,
        }
      } else if (type === 'Delay') {
        newAction = { type: 'Delay', duration: 5000 }
      } else if (type === 'CreateAlert') {
        newAction = { type: 'CreateAlert', title: '', message: '', severity: 'info' }
      } else if (type === 'HttpRequest') {
        newAction = { type: 'HttpRequest', method: 'GET', url: '' }
      } else {
        newAction = { type: 'Log', level: 'info', message: '' }
      }
      return [...prev, newAction]
    })
  }, [resources.devices, resources.deviceTypes])

  const updateAction = useCallback((index: number, data: Partial<RuleAction>) => {
    setActions(prev => prev.map((a, i) => {
      if (i !== index) return a
      return { ...a, ...data } as RuleAction
    }))
  }, [])

  const removeAction = useCallback((index: number) => {
    setActions(prev => prev.filter((_, i) => i !== index))
  }, [])

  // ============================================================================
  // Validation
  // ============================================================================

  const isValid = Boolean(name.trim() && condition)

  const getValidationMessage = () => {
    if (!name.trim()) return t('automation:validation.nameRequired', { defaultValue: '请输入规则名称' })
    if (!condition) return t('automation:validation.conditionRequired', { defaultValue: '请添加触发条件' })
    return ''
  }

  // ============================================================================
  // Save
  // ============================================================================

  const handleSave = async () => {
    if (!isValid || !condition) return

    setSaving(true)
    try {
      const finalCondition = uiConditionToRuleCondition(condition)

      await onSave({
        name,
        description,
        enabled,
        // For backward compatibility with trigger-based API
        trigger: { type: 'device_state' } as RuleTrigger,
        condition: finalCondition,
        actions: actions.length > 0 ? actions : undefined,
      })
    } finally {
      setSaving(false)
    }
  }

  // ============================================================================
  // Render
  // ============================================================================
  return (
    <FullScreenBuilder
      open={open}
      onClose={() => onOpenChange(false)}
      title={rule
        ? t('automation:editRule', { defaultValue: '编辑规则' })
        : t('automation:createRule', { defaultValue: '创建自动化规则' })
      }
      description={t('automation:ruleBuilderDesc', {
        defaultValue: '定义触发条件和执行动作，当条件满足时自动执行',
      })}
      icon={<Zap className="h-5 w-5 text-purple-500" />}
      sidePanel={{
        content: (
          <div className="space-y-4">
            <TipCard
              title={t('automation:tips.ruleTitle', { defaultValue: '关于规则' })}
              variant="info"
            >
              {t('automation:tips.ruleDesc', {
                defaultValue: '规则基于设备状态或指标触发。当条件满足时，自动执行配置的动作。',
              })}
            </TipCard>
          </div>
        ),
        title: t('automation:tips', { defaultValue: '提示' }),
      }}
      isValid={isValid}
      isDirty={true}
      isSaving={saving}
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
          <div className="space-y-2">
            <Label htmlFor="rule-name">{t('automation:ruleName', { defaultValue: '规则名称' })} *</Label>
            <Input
              id="rule-name"
              value={name}
              onChange={e => setName(e.target.value)}
              placeholder={t('automation:ruleNamePlaceholder', { defaultValue: '例如：温度过高自动开空调' })}
            />
          </div>
          <div className="space-y-2">
            <Label htmlFor="rule-description">{t('common:description', { defaultValue: '描述' })}</Label>
            <Input
              id="rule-description"
              value={description}
              onChange={e => setDescription(e.target.value)}
              placeholder={t('automation:ruleDescPlaceholder', { defaultValue: '规则描述（可选）' })}
            />
          </div>
          <div className="flex items-center gap-2 pt-2">
            <Switch checked={enabled} onCheckedChange={setEnabled} id="rule-enabled" />
            <Label htmlFor="rule-enabled" className="text-sm cursor-pointer">
              {enabled ? t('common:enabled', { defaultValue: '启用' }) : t('common:disabled', { defaultValue: '禁用' })}
            </Label>
          </div>
        </BuilderSection>

        {/* Visual Mode (no tabs, always visual) */}
        <div className="space-y-6">
            {/* Condition Section */}
            <BuilderSection
              title={t('automation:conditions', { defaultValue: '触发条件' })}
              description={t('automation:conditionsDesc', { defaultValue: '满足条件时执行动作' })}
              icon={<Lightbulb className="h-4 w-4 text-yellow-500" />}
            >
              {!condition ? (
                <div className="text-center py-8">
                  <p className="text-sm text-muted-foreground mb-4">
                    {t('automation:noCondition', { defaultValue: '请选择条件类型' })}
                  </p>
                  <div className="flex flex-wrap gap-2 justify-center">
                    <Button onClick={() => addCondition('simple')} variant="outline" size="sm">
                      {t('automation:simpleCondition', { defaultValue: '简单条件' })}
                    </Button>
                    <Button onClick={() => addCondition('range')} variant="outline" size="sm">
                      {t('automation:rangeCondition', { defaultValue: '范围条件' })}
                    </Button>
                    <Button onClick={() => addCondition('and')} variant="outline" size="sm">
                      AND {t('automation:combination', { defaultValue: '组合' })}
                    </Button>
                    <Button onClick={() => addCondition('or')} variant="outline" size="sm">
                      OR {t('automation:combination', { defaultValue: '组合' })}
                    </Button>
                    <Button onClick={() => addCondition('not')} variant="outline" size="sm">
                      NOT {t('automation:condition', { defaultValue: '条件' })}
                    </Button>
                  </div>
                </div>
              ) : (
                <div className="space-y-3">
                  {/* Condition Renderer */}
                  <ConditionEditor
                    condition={condition}
                    devices={resources.devices}
                    deviceTypes={resources.deviceTypes}
                    onUpdate={updateCondition}
                    onNestedUpdate={updateNestedCondition}
                    onReset={() => setCondition(null)}
                  />

                  {/* FOR Clause */}
                  <div className="flex items-center gap-3 p-3 bg-blue-500/10 rounded-md border border-blue-500/20">
                    <Clock className="h-4 w-4 text-blue-500" />
                    <Label className="text-sm">
                      {t('automation:forDuration', { defaultValue: '持续时间' })}
                    </Label>
                    <Input
                      type="number"
                      min={0}
                      value={forDuration}
                      onChange={e => setForDuration(parseInt(e.target.value) || 0)}
                      className="w-20 h-8"
                    />
                    <Select value={forUnit} onValueChange={(v: any) => setForUnit(v)}>
                      <SelectTrigger className="w-24 h-8">
                        <SelectValue />
                      </SelectTrigger>
                      <SelectContent>
                        <SelectItem value="seconds">{t('automation:units.seconds', { defaultValue: '秒' })}</SelectItem>
                        <SelectItem value="minutes">{t('automation:units.minutes', { defaultValue: '分钟' })}</SelectItem>
                        <SelectItem value="hours">{t('automation:units.hours', { defaultValue: '小时' })}</SelectItem>
                      </SelectContent>
                    </Select>
                    <span className="text-xs text-muted-foreground">
                      {forDuration > 0 && t('automation:forDurationHint', { defaultValue: '条件需持续满足此时间才触发' })}
                    </span>
                  </div>
                </div>
              )}
            </BuilderSection>

            {/* Actions Section */}
            <BuilderSection
              title={t('automation:actions', { defaultValue: '执行动作' })}
              description={t('automation:actionsDesc', { defaultValue: '触发时执行的操作' })}
              icon={<Zap className="h-4 w-4 text-green-500" />}
            >
              {/* Add Action Buttons */}
              <div className="flex flex-wrap gap-2 mb-4">
                <Button onClick={() => addAction('Execute')} variant="outline" size="sm">
                  <Zap className="h-4 w-4 mr-1" />
                  {t('automation:executeCommand', { defaultValue: '执行命令' })}
                </Button>
                <Button onClick={() => addAction('Notify')} variant="outline" size="sm">
                  <Bell className="h-4 w-4 mr-1" />
                  {t('automation:sendNotification', { defaultValue: '发送通知' })}
                </Button>
                <Button onClick={() => addAction('Log')} variant="outline" size="sm">
                  <FileText className="h-4 w-4 mr-1" />
                  {t('automation:log', { defaultValue: '记录日志' })}
                </Button>
                <Button onClick={() => addAction('Set')} variant="outline" size="sm">
                  <Globe className="h-4 w-4 mr-1" />
                  {t('automation:setProperty', { defaultValue: '设置属性' })}
                </Button>
                <Button onClick={() => addAction('Delay')} variant="outline" size="sm">
                  <Timer className="h-4 w-4 mr-1" />
                  {t('automation:delay', { defaultValue: '延迟' })}
                </Button>
                <Button onClick={() => addAction('CreateAlert')} variant="outline" size="sm">
                  <AlertTriangle className="h-4 w-4 mr-1" />
                  {t('automation:createAlert', { defaultValue: '创建告警' })}
                </Button>
                <Button onClick={() => addAction('HttpRequest')} variant="outline" size="sm">
                  <Globe className="h-4 w-4 mr-1" />
                  HTTP
                </Button>
              </div>

              {actions.length === 0 ? (
                <div className="text-center py-8 text-muted-foreground">
                  <Zap className="h-8 w-8 mx-auto mb-2 opacity-50" />
                  <p className="text-sm">{t('automation:noActions', { defaultValue: '暂无动作，点击上方按钮添加' })}</p>
                </div>
              ) : (
                <div className="space-y-2">
                  {actions.map((action, i) => (
                    <ActionEditor
                      key={i}
                      action={action}
                      index={i}
                      devices={resources.devices}
                      deviceTypes={resources.deviceTypes}
                      onUpdate={(data) => updateAction(i, data)}
                      onRemove={() => removeAction(i)}
                    />
                  ))}
                </div>
              )}
            </BuilderSection>
        </div>
      </div>
    </FullScreenBuilder>
  )
}

// ============================================================================
// Condition Editor Component
// ============================================================================

interface ConditionEditorProps {
  condition: UICondition
  devices: Array<{ id: string; name: string; device_type?: string }>
  deviceTypes?: DeviceType[]
  onUpdate: (updates: Partial<UICondition>) => void
  onNestedUpdate: (path: number[], updates: Partial<UICondition>) => void
  onReset: () => void
  path?: number[]
}

function ConditionEditor({
  condition,
  devices,
  deviceTypes,
  onUpdate,
  onNestedUpdate,
  onReset,
  path = [],
}: ConditionEditorProps) {
  const deviceOptions = devices.map(d => ({ value: d.id, label: d.name }))

  const renderSimpleCondition = (cond: UICondition, currentPath: number[]) => {
    const metrics = getDeviceMetrics(cond.device_id || '', devices, deviceTypes)

    return (
      <div className="flex items-center gap-2 p-3 bg-muted/40 rounded-md">
        {path.length > 0 && currentPath.length === 0 && (
          <Badge variant="outline" className="text-xs">
            {condition.type.toUpperCase()}
          </Badge>
        )}
        <Select
          value={cond.device_id}
          onValueChange={(v) => {
            const newMetrics = getDeviceMetrics(v, devices, deviceTypes)
            currentPath.length === 0
              ? onUpdate({ device_id: v, metric: newMetrics[0]?.name || 'value' })
              : onNestedUpdate(currentPath, { device_id: v, metric: newMetrics[0]?.name || 'value' })
          }}
        >
          <SelectTrigger className="w-32">
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            {deviceOptions.map(d => (
              <SelectItem key={d.value} value={d.value}>{d.label}</SelectItem>
            ))}
          </SelectContent>
        </Select>
        <span className="text-xs text-muted-foreground">.</span>
        <Select
          value={cond.metric}
          onValueChange={(v) => {
            currentPath.length === 0
              ? onUpdate({ metric: v })
              : onNestedUpdate(currentPath, { metric: v })
          }}
        >
          <SelectTrigger className="w-24">
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            {metrics.map(m => (
              <SelectItem key={m.name} value={m.name}>{m.display_name || m.name}</SelectItem>
            ))}
          </SelectContent>
        </Select>
        <Select
          value={cond.operator}
          onValueChange={(v) => {
            currentPath.length === 0
              ? onUpdate({ operator: v })
              : onNestedUpdate(currentPath, { operator: v })
          }}
        >
          <SelectTrigger className="w-16">
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            {COMPARISON_OPERATORS.map(o => (
              <SelectItem key={o.value} value={o.value}>{o.symbol}</SelectItem>
            ))}
          </SelectContent>
        </Select>
        <Input
          type="number"
          value={cond.threshold}
          onChange={(e) => {
            currentPath.length === 0
              ? onUpdate({ threshold: parseFloat(e.target.value) || 0 })
              : onNestedUpdate(currentPath, { threshold: parseFloat(e.target.value) || 0 })
          }}
          className="w-20 h-9"
        />
        {currentPath.length === 0 && (
          <Button variant="ghost" size="icon" className="h-8 w-8 ml-auto" onClick={onReset}>
            <X className="h-4 w-4" />
          </Button>
        )}
      </div>
    )
  }

  const renderRangeCondition = (cond: UICondition, currentPath: number[]) => {
    const metrics = getDeviceMetrics(cond.device_id || '', devices, deviceTypes)

    return (
      <div className="flex items-center gap-2 p-3 bg-muted/40 rounded-md">
        <Badge variant="outline" className="text-xs bg-blue-500/10 text-blue-500">
          BETWEEN
        </Badge>
        <Select
          value={cond.device_id}
          onValueChange={(v) => {
            const newMetrics = getDeviceMetrics(v, devices, deviceTypes)
            currentPath.length === 0
              ? onUpdate({ device_id: v, metric: newMetrics[0]?.name || 'value' })
              : onNestedUpdate(currentPath, { device_id: v, metric: newMetrics[0]?.name || 'value' })
          }}
        >
          <SelectTrigger className="w-32">
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            {deviceOptions.map(d => (
              <SelectItem key={d.value} value={d.value}>{d.label}</SelectItem>
            ))}
          </SelectContent>
        </Select>
        <span className="text-xs text-muted-foreground">.</span>
        <Select
          value={cond.metric}
          onValueChange={(v) => {
            currentPath.length === 0
              ? onUpdate({ metric: v })
              : onNestedUpdate(currentPath, { metric: v })
          }}
        >
          <SelectTrigger className="w-24">
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            {metrics.map(m => (
              <SelectItem key={m.name} value={m.name}>{m.display_name || m.name}</SelectItem>
            ))}
          </SelectContent>
        </Select>
        <span className="text-xs text-muted-foreground">BETWEEN</span>
        <Input
          type="number"
          value={cond.range_min}
          onChange={(e) => {
            currentPath.length === 0
              ? onUpdate({ range_min: parseFloat(e.target.value) || 0 })
              : onNestedUpdate(currentPath, { range_min: parseFloat(e.target.value) || 0 })
          }}
          className="w-16 h-9"
          placeholder="Min"
        />
        <span className="text-xs text-muted-foreground">AND</span>
        <Input
          type="number"
          value={cond.range_max}
          onChange={(e) => {
            currentPath.length === 0
              ? onUpdate({ range_max: parseFloat(e.target.value) || 0 })
              : onNestedUpdate(currentPath, { range_max: parseFloat(e.target.value) || 0 })
          }}
          className="w-16 h-9"
          placeholder="Max"
        />
        {currentPath.length === 0 && (
          <Button variant="ghost" size="icon" className="h-8 w-8 ml-auto" onClick={onReset}>
            <X className="h-4 w-4" />
          </Button>
        )}
      </div>
    )
  }

  const renderLogicalCondition = (cond: UICondition, currentPath: number[]) => {
    const label = cond.type.toUpperCase()
    const badgeClass = cond.type === 'and'
      ? 'bg-green-500/10 text-green-500 border-green-500/30'
      : cond.type === 'or'
      ? 'bg-amber-500/10 text-amber-500 border-amber-500/30'
      : 'bg-red-500/10 text-red-500 border-red-500/30'

    const connectorText = cond.type === 'and' ? 'AND' : cond.type === 'or' ? 'OR' : 'NOT'

    return (
      <div className="space-y-2">
        {/* Header with operator badge and controls */}
        <div className="flex items-center gap-2 p-2 bg-muted/30 rounded-t-md border border-muted">
          <Badge variant="outline" className={cn('text-xs px-2 py-0.5', badgeClass)}>
            {label}
          </Badge>
          <span className="text-xs text-muted-foreground">
            {cond.type === 'and' ? '所有条件都要满足' : cond.type === 'or' ? '任一条件满足' : '条件不满足时'}
          </span>
          {currentPath.length === 0 && (
            <Button variant="ghost" size="icon" className="h-6 w-6 ml-auto" onClick={onReset}>
              <X className="h-3 w-3" />
            </Button>
          )}
        </div>

        {/* Conditions container with proper styling */}
        <div className="p-3 bg-background border-x border-b border-muted rounded-b-md space-y-3">
          {cond.conditions?.map((subCond, i) => (
            <div key={subCond.id} className="relative group">
              {/* Connector line before each condition (except first) */}
              {i > 0 && (
                <div className="flex items-center justify-start -mb-2 mt-1">
                  <span className={cn(
                    "text-xs font-medium px-2 py-0.5 rounded-full",
                    cond.type === 'and' ? "bg-green-100 text-green-700 dark:bg-green-900/30 dark:text-green-400" :
                    cond.type === 'or' ? "bg-amber-100 text-amber-700 dark:bg-amber-900/30 dark:text-amber-400" :
                    "bg-red-100 text-red-700 dark:bg-red-900/30 dark:text-red-400"
                  )}>
                    {connectorText}
                  </span>
                </div>
              )}

              {/* Condition editor with wrapper for delete button */}
              <div className="relative pr-8">
                <div className={cn(
                  "rounded-md",
                  subCond.type === 'and' || subCond.type === 'or' || subCond.type === 'not'
                    ? "bg-muted/50 border border-muted"
                    : ""
                )}>
                  {currentPath.length === 0 ? (
                    <ConditionEditor
                      condition={subCond}
                      devices={devices}
                      deviceTypes={deviceTypes}
                      onUpdate={(updates) => {
                        const newConditions = [...(cond.conditions || [])]
                        newConditions[i] = { ...newConditions[i], ...updates }
                        onUpdate({ conditions: newConditions })
                      }}
                      onNestedUpdate={(nestedPath, updates) => {
                        onNestedUpdate([i, ...nestedPath], updates)
                      }}
                      onReset={() => {
                        const newConditions = cond.conditions?.filter((_, idx) => idx !== i) || []
                        onUpdate({ conditions: newConditions })
                      }}
                      path={[i]}
                    />
                  ) : (
                    <ConditionEditor
                      condition={subCond}
                      devices={devices}
                      deviceTypes={deviceTypes}
                      onUpdate={() => {}}
                      onNestedUpdate={(nestedPath, updates) => {
                        onNestedUpdate([i, ...nestedPath], updates)
                      }}
                      onReset={() => {}}
                      path={[...currentPath, i]}
                    />
                  )}
                </div>

                {/* Delete button for nested conditions */}
                {currentPath.length === 0 && cond.conditions && cond.conditions.length > 1 && (
                  <Button
                    variant="ghost"
                    size="icon"
                    className="h-6 w-6 absolute right-0 top-2 opacity-0 group-hover:opacity-100 transition-opacity"
                    onClick={() => {
                      const newConditions = cond.conditions?.filter((_, idx) => idx !== i) || []
                      onUpdate({ conditions: newConditions })
                    }}
                  >
                    <X className="h-3 w-3" />
                  </Button>
                )}
              </div>
            </div>
          ))}

          {/* Add condition button */}
          <div className="pt-2 border-t border-muted/50">
            <Button
              variant="outline"
              size="sm"
              className="w-full border-dashed"
              onClick={() => {
                const newCond: UICondition = {
                  id: crypto.randomUUID(),
                  type: 'simple',
                  device_id: devices[0]?.id || '',
                  metric: getDeviceMetrics(devices[0]?.id || '', devices, deviceTypes)[0]?.name || 'value',
                  operator: '>',
                  threshold: 0,
                }
                const newConditions = [...(cond.conditions || []), newCond]
                currentPath.length === 0
                  ? onUpdate({ conditions: newConditions })
                  : onNestedUpdate(currentPath, { conditions: newConditions })
              }}
            >
              <Plus className="h-3 w-3 mr-1" />
              {t('automation:addCondition', { defaultValue: '添加条件' })}
            </Button>
          </div>
        </div>
      </div>
    )
  }

  function t(key: string, defaultValue: any) {
    return defaultValue || key
  }

  switch (condition.type) {
    case 'simple':
      return renderSimpleCondition(condition, path)
    case 'range':
      return renderRangeCondition(condition, path)
    case 'and':
    case 'or':
    case 'not':
      return renderLogicalCondition(condition, path)
    default:
      return null
  }
}

// ============================================================================
// Action Editor Component
// ============================================================================

interface ActionEditorProps {
  action: RuleAction
  index: number
  devices: Array<{ id: string; name: string; device_type?: string }>
  deviceTypes?: DeviceType[]
  onUpdate: (data: Partial<RuleAction>) => void
  onRemove: () => void
}

function ActionEditor({ action, index: _index, devices, deviceTypes, onUpdate, onRemove }: ActionEditorProps) {
  const deviceOptions = devices.map(d => ({ value: d.id, label: d.name }))

  const getActionIcon = () => {
    switch (action.type) {
      case 'Execute': return <Zap className="h-4 w-4 text-yellow-500" />
      case 'Notify': return <Bell className="h-4 w-4 text-blue-500" />
      case 'Log': return <FileText className="h-4 w-4 text-gray-500" />
      case 'Set': return <Globe className="h-4 w-4 text-purple-500" />
      case 'Delay': return <Timer className="h-4 w-4 text-orange-500" />
      case 'CreateAlert': return <AlertTriangle className="h-4 w-4 text-red-500" />
      case 'HttpRequest': return <Globe className="h-4 w-4 text-green-500" />
      default: return <Zap className="h-4 w-4" />
    }
  }

  const getActionLabel = (): string => {
    const actionType: string = (action as any).type
    switch (action.type) {
      case 'Execute': return '执行'
      case 'Notify': return '通知'
      case 'Log': return '日志'
      case 'Set': return '设置'
      case 'Delay': return '延迟'
      case 'CreateAlert': return '告警'
      case 'HttpRequest': return 'HTTP'
    }
    return actionType
  }

  return (
    <div className="flex items-start gap-2 p-3 bg-muted/40 rounded-md">
      {getActionIcon()}
      <span className="text-xs px-2 py-1 bg-background rounded">
        {getActionLabel()}
      </span>

      {/* Execute Action */}
      {action.type === 'Execute' && (
        <>
          <Select
            value={action.device_id}
            onValueChange={(v) => {
              const commands = getDeviceCommands(v, devices, deviceTypes)
              onUpdate({
                device_id: v,
                command: commands[0]?.name || 'turn_on',
              })
            }}
          >
            <SelectTrigger className="w-28">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              {deviceOptions.map(d => (
                <SelectItem key={d.value} value={d.value}>{d.label}</SelectItem>
              ))}
            </SelectContent>
          </Select>
          <span className="text-xs text-muted-foreground">.</span>
          <Select
            value={action.command}
            onValueChange={(v) => onUpdate({ command: v })}
          >
            <SelectTrigger className="w-24">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              {getDeviceCommands(action.device_id, devices, deviceTypes).map(c => (
                <SelectItem key={c.name} value={c.name}>{c.display_name || c.name}</SelectItem>
              ))}
            </SelectContent>
          </Select>
        </>
      )}

      {/* Notify Action */}
      {action.type === 'Notify' && (
        <Input
          value={action.message}
          onChange={(e) => onUpdate({ message: e.target.value })}
          placeholder="通知内容"
          className="flex-1"
        />
      )}

      {/* Log Action */}
      {action.type === 'Log' && (
        <>
          <Select
            value={action.level}
            onValueChange={(v: any) => onUpdate({ level: v })}
          >
            <SelectTrigger className="w-16">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              <SelectItem value="debug">DEBUG</SelectItem>
              <SelectItem value="info">INFO</SelectItem>
              <SelectItem value="warn">WARN</SelectItem>
              <SelectItem value="error">ERROR</SelectItem>
            </SelectContent>
          </Select>
          <Input
            value={action.message}
            onChange={(e) => onUpdate({ message: e.target.value })}
            placeholder="日志内容"
            className="flex-1"
          />
        </>
      )}

      {/* Set Action */}
      {action.type === 'Set' && (
        <>
          <Select
            value={action.device_id}
            onValueChange={(v) => onUpdate({ device_id: v })}
          >
            <SelectTrigger className="w-24">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              {deviceOptions.map(d => (
                <SelectItem key={d.value} value={d.value}>{d.label}</SelectItem>
              ))}
            </SelectContent>
          </Select>
          <Input
            value={action.property}
            onChange={(e) => onUpdate({ property: e.target.value })}
            placeholder="属性名"
            className="w-20"
          />
          <span className="text-xs text-muted-foreground">=</span>
          <Input
            value={String(action.value ?? '')}
            onChange={(e) => onUpdate({ value: e.target.value })}
            placeholder="值"
            className="w-20"
          />
        </>
      )}

      {/* Delay Action */}
      {action.type === 'Delay' && (
        <>
          <Input
            type="number"
            value={(action.duration || 0) / 1000}
            onChange={(e) => onUpdate({ duration: (parseInt(e.target.value) || 0) * 1000 })}
            className="w-20"
          />
          <span className="text-xs text-muted-foreground">秒</span>
        </>
      )}

      {/* CreateAlert Action */}
      {action.type === 'CreateAlert' && (
        <>
          <Input
            value={action.title}
            onChange={(e) => onUpdate({ title: e.target.value })}
            placeholder="告警标题"
            className="w-32"
          />
          <Input
            value={action.message}
            onChange={(e) => onUpdate({ message: e.target.value })}
            placeholder="告警消息"
            className="flex-1"
          />
          <Select
            value={action.severity}
            onValueChange={(v: any) => onUpdate({ severity: v })}
          >
            <SelectTrigger className="w-20">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              <SelectItem value="info">Info</SelectItem>
              <SelectItem value="warning">Warning</SelectItem>
              <SelectItem value="error">Error</SelectItem>
              <SelectItem value="critical">Critical</SelectItem>
            </SelectContent>
          </Select>
        </>
      )}

      {/* HttpRequest Action */}
      {action.type === 'HttpRequest' && (
        <>
          <Select
            value={action.method}
            onValueChange={(v: any) => onUpdate({ method: v })}
          >
            <SelectTrigger className="w-20">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              <SelectItem value="GET">GET</SelectItem>
              <SelectItem value="POST">POST</SelectItem>
              <SelectItem value="PUT">PUT</SelectItem>
              <SelectItem value="DELETE">DELETE</SelectItem>
              <SelectItem value="PATCH">PATCH</SelectItem>
            </SelectContent>
          </Select>
          <Input
            value={action.url}
            onChange={(e) => onUpdate({ url: e.target.value })}
            placeholder="https://example.com/api"
            className="flex-1"
          />
        </>
      )}

      <Button variant="ghost" size="icon" className="h-8 w-8 ml-auto" onClick={onRemove}>
        <Trash2 className="h-4 w-4" />
      </Button>
    </div>
  )
}