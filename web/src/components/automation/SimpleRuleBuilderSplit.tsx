import { useState, useEffect, useCallback } from 'react'
import { useTranslation } from 'react-i18next'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Badge } from '@/components/ui/badge'
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
  DialogDescription,
} from '@/components/ui/dialog'
import {
  Plus,
  X,
  Zap,
  Bell,
  FileText,
  Save,
  Lightbulb,
  Clock,
  AlertTriangle,
  Timer,
  Globe,
  ChevronRight,
} from 'lucide-react'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'
import type { Rule, RuleTrigger, RuleCondition, RuleAction, DeviceType } from '@/types'
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

type ConditionType = 'simple' | 'range' | 'and' | 'or' | 'not'

// Numeric comparison operators
const NUMERIC_OPERATORS = [
  { value: '>', label: 'Greater than', symbol: '>' },
  { value: '<', label: 'Less than', symbol: '<' },
  { value: '>=', label: 'Greater or equal', symbol: '≥' },
  { value: '<=', label: 'Less or equal', symbol: '≤' },
]

// String comparison operators
const STRING_OPERATORS = [
  { value: '==', label: 'Equals', symbol: '=' },
  { value: '!=', label: 'Not equals', symbol: '≠' },
  { value: 'contains', label: 'Contains', symbol: '∋' },
  { value: 'starts_with', label: 'Starts with', symbol: 'a*' },
  { value: 'ends_with', label: 'Ends with', symbol: '*z' },
  { value: 'regex', label: 'Regex', symbol: '.*' },
]

// Boolean comparison operators
const BOOLEAN_OPERATORS = [
  { value: '==', label: 'Equals', symbol: '=' },
  { value: '!=', label: 'Not equals', symbol: '≠' },
]

const getComparisonOperators = (_t: (key: string) => string, dataType?: string) => {
  if (dataType === 'string') {
    return [...NUMERIC_OPERATORS, ...STRING_OPERATORS]
  }
  if (dataType === 'boolean') {
    return BOOLEAN_OPERATORS
  }
  // Numeric types (integer, float) or unknown
  return [...NUMERIC_OPERATORS, { value: '==', label: 'Equals', symbol: '=' }, { value: '!=', label: 'Not equals', symbol: '≠' }]
}

// UI condition type
interface UICondition {
  id: string
  type: ConditionType
  device_id?: string
  metric?: string
  operator?: string
  threshold?: number
  threshold_value?: string  // For string/boolean values
  range_min?: number
  range_max?: number
  conditions?: UICondition[]
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
): Array<{ name: string; display_name?: string; data_type?: string }> {
  const deviceTypeName = getDeviceType(deviceId, devices, deviceTypes)
  const deviceType = deviceTypes?.find(t => t.device_type === deviceTypeName)
  return deviceType?.metrics || []
}

function getMetricDataType(
  metricName: string,
  deviceId: string,
  devices: Array<{ id: string; name: string; device_type?: string }>,
  deviceTypes?: DeviceType[]
): string {
  const metrics = getDeviceMetrics(deviceId, devices, deviceTypes)
  const metric = metrics.find(m => m.name === metricName)
  return metric?.data_type || 'float'
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

function uiConditionToRuleCondition(cond: UICondition): RuleCondition {
  switch (cond.type) {
    case 'simple':
      // Determine threshold value - try to parse as number first
      let thresholdValue: number | string
      if (cond.threshold_value !== undefined) {
        // Try to parse as number for non-string operators
        const parsed = Number(cond.threshold_value)
        if (!isNaN(parsed) && cond.operator !== 'contains' && cond.operator !== 'starts_with' && cond.operator !== 'ends_with' && cond.operator !== 'regex') {
          thresholdValue = parsed
        } else {
          thresholdValue = cond.threshold_value
        }
      } else {
        thresholdValue = cond.threshold ?? 0
      }

      return {
        device_id: cond.device_id || '',
        metric: cond.metric || 'value',
        operator: cond.operator || '>',
        threshold: thresholdValue,
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

  if ('range_min' in ruleCond && (ruleCond as any).range_min !== undefined) {
    return {
      id: crypto.randomUUID(),
      type: 'range',
      device_id: ruleCond.device_id,
      metric: ruleCond.metric,
      range_min: (ruleCond as any).range_min,
      range_max: typeof ruleCond.threshold === 'number' ? ruleCond.threshold : 0,
    }
  }

  // Handle threshold value - can be number or string
  const thresholdValue = ruleCond.threshold
  const isStringThreshold = typeof thresholdValue === 'string'

  return {
    id: crypto.randomUUID(),
    type: 'simple',
    device_id: ruleCond.device_id,
    metric: ruleCond.metric,
    operator: ruleCond.operator,
    threshold: isStringThreshold ? undefined : thresholdValue as number,
    threshold_value: isStringThreshold ? thresholdValue : undefined,
  }
}

// Generate DSL from rule condition and actions
function generateRuleDSL(
  name: string,
  condition: RuleCondition,
  actions: RuleAction[],
  forDuration?: number,
  forUnit?: 'seconds' | 'minutes' | 'hours'
): string {
  const lines: string[] = []

  // Header
  lines.push(`RULE "${name}"`)

  // WHEN clause - generate condition DSL
  lines.push(`WHEN ${conditionToDSL(condition)}`)

  // FOR clause (duration)
  if (forDuration && forDuration > 0) {
    const unit = forUnit === 'seconds' ? 'seconds' : forUnit === 'hours' ? 'hours' : 'minutes'
    lines.push(`FOR ${forDuration} ${unit}`)
  }

  // DO clause - generate actions DSL
  lines.push('DO')
  for (const action of actions) {
    lines.push(`    ${actionToDSL(action)}`)
  }
  lines.push('END')

  return lines.join('\n')
}

// Parse FOR clause from DSL to get duration and unit
function parseForClauseFromDSL(dsl?: string): { duration: number; unit: 'seconds' | 'minutes' | 'hours' } | null {
  if (!dsl) return null

  // Match FOR clause: FOR <number> <unit>
  const forMatch = dsl.match(/^FOR\s+(\d+)\s+(seconds|minutes|hours)$/m)
  if (forMatch) {
    const duration = parseInt(forMatch[1], 10)
    const unit = forMatch[2] as 'seconds' | 'minutes' | 'hours'
    return { duration, unit }
  }

  return null
}

// Convert RuleCondition to DSL string
function conditionToDSL(cond: RuleCondition): string {
  // Check for logical operators (and/or/not)
  const op = (cond as any).operator
  if (op === 'and' || op === 'or') {
    const subConds = ((cond as any).conditions || []) as RuleCondition[]
    if (subConds.length === 0) return 'true'
    const parts = subConds.map(c => conditionToDSL(c))
    return `(${parts.join(`) ${op.toUpperCase()} (`)})`
  }
  if (op === 'not') {
    const subConds = ((cond as any).conditions || []) as RuleCondition[]
    if (subConds.length === 0) return 'false'
    return `NOT (${conditionToDSL(subConds[0])})`
  }

  // Range condition
  if ('range_min' in cond && (cond as any).range_min !== undefined) {
    const deviceId = cond.device_id || 'device'
    const metric = cond.metric || 'value'
    const min = (cond as any).range_min
    const max = typeof cond.threshold === 'number' ? cond.threshold : 100
    return `${deviceId}.${metric} BETWEEN ${min} AND ${max}`
  }

  // Simple condition
  const deviceId = cond.device_id || 'device'
  const metric = cond.metric || 'value'
  const operator = cond.operator || '>'
  let threshold = cond.threshold ?? 0

  // Handle string thresholds - quote them
  if (typeof threshold === 'string') {
    threshold = `"${threshold}"`
  }

  return `${deviceId}.${metric} ${operator} ${threshold}`
}

// Convert RuleAction to DSL string
function actionToDSL(action: RuleAction): string {
  switch (action.type) {
    case 'Notify':
      return `NOTIFY "${action.message}"`
    case 'Execute':
      const params = action.params && Object.keys(action.params).length > 0
        ? Object.entries(action.params).map(([k, v]) => `${k}=${v}`).join(', ')
        : ''
      return params
        ? `EXECUTE ${action.device_id}.${action.command}(${params})`
        : `EXECUTE ${action.device_id}.${action.command}`
    case 'Log':
      return `LOG ${action.level || 'info'}, "${action.message}"`
    case 'Set':
      const value = typeof action.value === 'string' ? `"${action.value}"` : String(action.value)
      return `SET ${action.device_id}.${action.property} = ${value}`
    case 'Delay':
      return `DELAY ${action.duration}ms`
    case 'CreateAlert':
      return `ALERT "${action.title}" "${action.message}" ${action.severity || 'info'}`
    case 'HttpRequest':
      return `HTTP ${action.method} ${action.url}`
    default:
      return '// Unknown action'
  }
}

// ============================================================================
// Main Component
// ============================================================================

export function SimpleRuleBuilderSplit({
  open,
  onOpenChange,
  rule,
  onSave,
  resources = { devices: [], deviceTypes: [] },
}: RuleBuilderProps) {
  const { t } = useTranslation(['automation', 'common'])
  const [name, setName] = useState('')
  const [description, setDescription] = useState('')
  const [enabled, setEnabled] = useState(true)
  const [condition, setCondition] = useState<UICondition | null>(null)
  const [forDuration, setForDuration] = useState<number>(0)
  const [forUnit, setForUnit] = useState<'seconds' | 'minutes' | 'hours'>('minutes')
  const [actions, setActions] = useState<RuleAction[]>([])
  const [saving, setSaving] = useState(false)
  const [validationErrors, setValidationErrors] = useState<string[]>([])

  useEffect(() => {
    if (open && rule) {
      setName(rule.name || '')
      setDescription(rule.description || '')
      setEnabled(rule.enabled ?? true)
      setValidationErrors([])
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
      // Parse FOR clause from DSL
      const forClause = parseForClauseFromDSL(rule.dsl)
      if (forClause) {
        setForDuration(forClause.duration)
        setForUnit(forClause.unit)
      } else {
        setForDuration(0)
        setForUnit('minutes')
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
    setActions([{ type: 'Log', level: 'info', message: t('automation:ruleTriggered') }])
    setValidationErrors([])
  }, [t])

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

  // Validation
  const validateCondition = (cond: UICondition, path: string[] = []): string[] => {
    const errors: string[] = []
    const currentPath = path.join(' > ')

    if (cond.type === 'simple' || cond.type === 'range') {
      if (!cond.device_id) {
        errors.push(currentPath ? `${currentPath}: ${t('automation:ruleBuilder.selectDevice')}` : t('automation:ruleBuilder.selectDevice'))
      }
      if (!cond.metric) {
        errors.push(currentPath ? `${currentPath}: ${t('automation:ruleBuilder.selectMetric')}` : t('automation:ruleBuilder.selectMetric'))
      }
      if (cond.type === 'simple') {
        // Check for threshold value - can be numeric or string
        const hasValue = cond.threshold !== undefined && cond.threshold !== null
          || cond.threshold_value !== undefined && cond.threshold_value !== ''
        if (!hasValue) {
          errors.push(currentPath ? `${currentPath}: ${t('automation:ruleBuilder.enterThreshold')}` : t('automation:ruleBuilder.enterThreshold'))
        }
      }
      if (cond.type === 'range') {
        if (cond.range_min === undefined || cond.range_min === null) {
          errors.push(currentPath ? `${currentPath}: ${t('automation:ruleBuilder.enterMinValue')}` : t('automation:ruleBuilder.enterMinValue'))
        }
        if (cond.range_max === undefined || cond.range_max === null) {
          errors.push(currentPath ? `${currentPath}: ${t('automation:ruleBuilder.enterMaxValue')}` : t('automation:ruleBuilder.enterMaxValue'))
        }
        if (cond.range_min !== undefined && cond.range_max !== undefined && cond.range_min >= cond.range_max) {
          errors.push(currentPath ? `${currentPath}: ${t('automation:ruleBuilder.minMustBeLessThanMax')}` : t('automation:ruleBuilder.minMustBeLessThanMax'))
        }
      }
    } else if (cond.type === 'and' || cond.type === 'or' || cond.type === 'not') {
      if (!cond.conditions || cond.conditions.length === 0) {
        errors.push(currentPath ? `${currentPath}: ${t('automation:ruleBuilder.addSubConditions')}` : t('automation:ruleBuilder.addSubConditions'))
      } else {
        cond.conditions.forEach((subCond, i) => {
          errors.push(...validateCondition(subCond, [...path, `${cond.type.toUpperCase()}[${i + 1}]`]))
        })
      }
    }

    return errors
  }

  const validateActions = (): string[] => {
    const errors: string[] = []

    if (actions.length === 0) {
      errors.push(t('automation:ruleBuilder.atLeastOneAction'))
      return errors
    }

    actions.forEach((action, i) => {
      const prefix = t('automation:ruleBuilder.actionPrefix', { defaultValue: `Action ${i + 1}` })

      switch (action.type) {
        case 'Execute':
          if (!action.device_id) {
            errors.push(`${prefix}: ${t('automation:ruleBuilder.selectDeviceForAction')}`)
          }
          if (!action.command) {
            errors.push(`${prefix}: ${t('automation:ruleBuilder.enterCommandName')}`)
          }
          break
        case 'Notify':
          if (!action.message || action.message.trim() === '') {
            errors.push(`${prefix}: ${t('automation:ruleBuilder.enterNotificationMessage')}`)
          }
          break
        case 'Log':
          if (!action.message || action.message.trim() === '') {
            errors.push(`${prefix}: ${t('automation:ruleBuilder.enterLogMessage')}`)
          }
          break
        case 'Set':
          if (!action.device_id) {
            errors.push(`${prefix}: ${t('automation:ruleBuilder.selectDeviceForSet')}`)
          }
          if (!action.property) {
            errors.push(`${prefix}: ${t('automation:ruleBuilder.enterPropertyName')}`)
          }
          break
        case 'Delay':
          if (!action.duration || action.duration <= 0) {
            errors.push(`${prefix}: ${t('automation:ruleBuilder.enterDelayDuration')}`)
          }
          break
        case 'CreateAlert':
          if (!action.title || action.title.trim() === '') {
            errors.push(`${prefix}: ${t('automation:ruleBuilder.enterAlertTitle')}`)
          }
          if (!action.message || action.message.trim() === '') {
            errors.push(`${prefix}: ${t('automation:ruleBuilder.enterAlertMessage')}`)
          }
          break
        case 'HttpRequest':
          if (!action.url || action.url.trim() === '') {
            errors.push(`${prefix}: ${t('automation:ruleBuilder.enterRequestUrl')}`)
          }
          if (!action.method) {
            errors.push(`${prefix}: ${t('automation:ruleBuilder.selectRequestMethod')}`)
          }
          break
      }
    })

    return errors
  }

  const handleSave = async () => {
    // Clear previous errors
    const errors: string[] = []

    // Validate name
    if (!name.trim()) {
      errors.push(t('automation:ruleBuilder.enterRuleName'))
    }

    // Validate condition
    if (!condition) {
      errors.push(t('automation:ruleBuilder.addTriggerCondition'))
    } else {
      errors.push(...validateCondition(condition))
    }

    // Validate actions
    errors.push(...validateActions())

    // Set all errors
    setValidationErrors(errors)

    if (errors.length > 0) {
      return
    }

    setSaving(true)
    try {
      // Condition is guaranteed to be non-null here due to validation above
      const finalCondition = uiConditionToRuleCondition(condition!)

      // Generate DSL from condition and actions
      const dsl = generateRuleDSL(name, finalCondition, actions, forDuration, forUnit)

      const ruleData: Partial<Rule> = {
        name,
        description,
        enabled,
        trigger: { type: 'device_state' } as RuleTrigger,
        condition: finalCondition,
        actions: actions.length > 0 ? actions : undefined,
        dsl,
      }
      // Only include id when editing existing rule
      if (rule?.id) {
        ruleData.id = rule.id
      }
      await onSave(ruleData)
    } finally {
      setSaving(false)
    }
  }

  // ============================================================================
  // Render
  // ============================================================================
  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-w-6xl h-[92vh] p-0 flex flex-col">
        <DialogHeader className="px-6 py-4 border-b">
          <div className="flex items-center gap-3">
            <Zap className="h-5 w-5 text-purple-500" />
            <div>
              <DialogTitle className="text-lg font-semibold">
                {rule ? t('automation:editRule') : t('automation:createRule')}
              </DialogTitle>
              <DialogDescription className="text-sm">
                {t('automation:ruleBuilder.description')}
              </DialogDescription>
            </div>
          </div>
        </DialogHeader>

        {/* Form Section - Basic Info */}
        <div className="border-b px-6 py-4 bg-muted/20 flex-shrink-0">
          <div className="grid grid-cols-2 gap-4">
            <div>
              <Label htmlFor="rule-name" className="text-xs font-medium">{t('automation:ruleBuilder.ruleName')} *</Label>
              <Input
                id="rule-name"
                value={name}
                onChange={e => setName(e.target.value)}
                placeholder={t('automation:ruleBuilder.ruleNamePlaceholder')}
                className="mt-1.5 h-9"
              />
            </div>
            <div>
              <Label htmlFor="rule-desc" className="text-xs font-medium">{t('automation:ruleDescription')}</Label>
              <Input
                id="rule-desc"
                value={description}
                onChange={e => setDescription(e.target.value)}
                placeholder={t('automation:ruleBuilder.descriptionPlaceholder')}
                className="mt-1.5 h-9"
              />
            </div>
          </div>
        </div>

        {/* Validation Errors */}
        {validationErrors.length > 0 && (
          <div className="px-6 py-3 bg-destructive/10 border-b border-destructive/20">
            <div className="flex items-center gap-2 text-destructive font-medium text-sm mb-2">
              <AlertTriangle className="h-4 w-4" />
              <span>{t('automation:ruleBuilder.fixErrors')}</span>
            </div>
            <ul className="text-sm text-destructive space-y-1 list-disc list-inside">
              {validationErrors.map((error, i) => (
                <li key={i}>{error}</li>
              ))}
            </ul>
          </div>
        )}

        {/* Main Content - Split Layout: Conditions (Left) | Actions (Right) */}
        <div className="flex-1 min-h-0 flex overflow-hidden">
          {/* Left Panel - Trigger Conditions */}
          <div className="w-1/2 border-r overflow-y-auto p-5">
            <div className="flex items-center gap-2 mb-4">
              <div className="p-1.5 bg-amber-500/20 rounded-md">
                <Lightbulb className="h-4 w-4 text-amber-500" />
              </div>
              <h3 className="font-semibold text-sm">{t('automation:ruleBuilder.triggerConditions')}</h3>
            </div>

            {!condition ? (
              <div className="border-2 border-dashed border-muted-foreground/25 rounded-xl p-8 text-center">
                <Lightbulb className="h-10 w-10 mx-auto mb-4 text-muted-foreground/50" />
                <p className="text-sm text-muted-foreground mb-5">{t('automation:ruleBuilder.selectConditionType')}</p>
                <div className="grid grid-cols-2 gap-2 max-w-xs mx-auto">
                  <Button onClick={() => addCondition('simple')} variant="outline" size="sm" className="h-auto py-2.5 px-3">
                    <span className="text-xs font-medium">{t('automation:ruleBuilder.simpleCondition')}</span>
                  </Button>
                  <Button onClick={() => addCondition('range')} variant="outline" size="sm" className="h-auto py-2.5 px-3">
                    <span className="text-xs font-medium">{t('automation:ruleBuilder.rangeCondition')}</span>
                  </Button>
                  <Button onClick={() => addCondition('and')} variant="outline" size="sm" className="h-auto py-2.5 px-3">
                    <span className="text-xs font-medium">{t('automation:ruleBuilder.andCombination')}</span>
                  </Button>
                  <Button onClick={() => addCondition('or')} variant="outline" size="sm" className="h-auto py-2.5 px-3">
                    <span className="text-xs font-medium">{t('automation:ruleBuilder.orCombination')}</span>
                  </Button>
                  <Button onClick={() => addCondition('not')} variant="outline" size="sm" className="h-auto py-2.5 px-3 col-span-2">
                    <span className="text-xs font-medium">{t('automation:ruleBuilder.notCondition')}</span>
                  </Button>
                </div>
              </div>
            ) : (
              <div className="space-y-4">
                <ConditionEditor
                  condition={condition}
                  devices={resources.devices}
                  deviceTypes={resources.deviceTypes}
                  onUpdate={updateCondition}
                  onNestedUpdate={updateNestedCondition}
                  onReset={() => setCondition(null)}
                />

                {/* FOR clause */}
                <div className="flex items-center gap-3 p-3 bg-blue-500/10 rounded-lg border border-blue-500/20">
                  <Clock className="h-4 w-4 text-blue-500 flex-shrink-0" />
                  <Label className="text-sm font-medium">{t('automation:ruleBuilder.duration')}</Label>
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
                      <SelectItem value="seconds">{t('automation:ruleBuilder.seconds')}</SelectItem>
                      <SelectItem value="minutes">{t('automation:ruleBuilder.minutes')}</SelectItem>
                      <SelectItem value="hours">{t('automation:ruleBuilder.hours')}</SelectItem>
                    </SelectContent>
                  </Select>
                  {forDuration > 0 && (
                    <span className="text-xs text-muted-foreground">{t('automation:ruleBuilder.durationHint')}</span>
                  )}
                </div>
              </div>
            )}
          </div>

          {/* Center Arrow */}
          <div className="w-10 flex-shrink-0 flex items-center justify-center bg-muted/30">
            <ChevronRight className="h-6 w-6 text-muted-foreground" />
          </div>

          {/* Right Panel - Execute Actions */}
          <div className="w-1/2 overflow-y-auto p-5">
            <div className="flex items-center gap-2 mb-4">
              <div className="p-1.5 bg-green-500/20 rounded-md">
                <Zap className="h-4 w-4 text-green-500" />
              </div>
              <h3 className="font-semibold text-sm">{t('automation:ruleBuilder.executeActions')}</h3>
            </div>

            <div className="flex flex-wrap gap-2 mb-4">
              <Button onClick={() => addAction('Execute')} variant="outline" size="sm" className="h-8">
                <Zap className="h-3.5 w-3.5 mr-1.5" />{t('automation:ruleBuilder.executeCommand')}
              </Button>
              <Button onClick={() => addAction('Notify')} variant="outline" size="sm" className="h-8">
                <Bell className="h-3.5 w-3.5 mr-1.5" />{t('automation:ruleBuilder.sendNotification')}
              </Button>
              <Button onClick={() => addAction('Log')} variant="outline" size="sm" className="h-8">
                <FileText className="h-3.5 w-3.5 mr-1.5" />{t('automation:ruleBuilder.logRecord')}
              </Button>
              <Button onClick={() => addAction('Set')} variant="outline" size="sm" className="h-8">
                <Globe className="h-3.5 w-3.5 mr-1.5" />{t('automation:ruleBuilder.writeValue')}
              </Button>
              <Button onClick={() => addAction('Delay')} variant="outline" size="sm" className="h-8">
                <Timer className="h-3.5 w-3.5 mr-1.5" />{t('automation:ruleBuilder.delay')}
              </Button>
              <Button onClick={() => addAction('CreateAlert')} variant="outline" size="sm" className="h-8">
                <AlertTriangle className="h-3.5 w-3.5 mr-1.5" />{t('automation:ruleBuilder.createAlert')}
              </Button>
              <Button onClick={() => addAction('HttpRequest')} variant="outline" size="sm" className="h-8">
                <Globe className="h-3.5 w-3.5 mr-1.5" />{t('automation:ruleBuilder.httpRequest')}
              </Button>
            </div>

            {actions.length === 0 ? (
              <div className="border-2 border-dashed border-muted-foreground/25 rounded-xl p-8 text-center">
                <Zap className="h-10 w-10 mx-auto mb-4 text-muted-foreground/50" />
                <p className="text-sm text-muted-foreground">{t('automation:ruleBuilder.noActions')}</p>
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
          </div>
        </div>

        {/* Footer */}
        <div className="flex items-center justify-between px-6 py-3 border-t bg-muted/20 flex-shrink-0">
          <div className="flex gap-2 ml-auto">
            <Button variant="outline" onClick={() => onOpenChange(false)}>
              {t('automation:ruleBuilder.cancel')}
            </Button>
            <Button onClick={handleSave} disabled={saving} className="min-w-[100px]">
              <Save className="h-4 w-4 mr-1.5" />
              {saving ? t('automation:ruleBuilder.saving') : t('automation:ruleBuilder.save')}
            </Button>
          </div>
        </div>
      </DialogContent>
    </Dialog>
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
  const { t } = useTranslation(['automation'])
  const deviceOptions = devices.map(d => ({ value: d.id, label: d.name }))

  const renderSimpleCondition = (cond: UICondition, currentPath: number[]) => {
    const metrics = getDeviceMetrics(cond.device_id || '', devices, deviceTypes)
    const hasDevice = Boolean(cond.device_id)
    const hasMetrics = metrics.length > 0

    // Get the data type of the selected metric
    const metricDataType = cond.metric && hasDevice && cond.device_id
      ? getMetricDataType(cond.metric, cond.device_id, devices, deviceTypes)
      : 'float'
    const isStringType = metricDataType === 'string'
    const isBooleanType = metricDataType === 'boolean'
    const isNumericType = ['integer', 'float'].includes(metricDataType)

    // Render value input based on data type
    const renderValueInput = () => {
      if (isBooleanType) {
        return (
          <Select
            value={cond.threshold_value ?? String(cond.threshold ?? '')}
            onValueChange={(v) => {
              const boolVal = v === 'true'
              currentPath.length === 0
                ? onUpdate({ threshold: boolVal ? 1 : 0, threshold_value: v })
                : onNestedUpdate(currentPath, { threshold: boolVal ? 1 : 0, threshold_value: v })
            }}
          >
            <SelectTrigger className="w-20 h-9"><SelectValue /></SelectTrigger>
            <SelectContent>
              <SelectItem value="true">true</SelectItem>
              <SelectItem value="false">false</SelectItem>
            </SelectContent>
          </Select>
        )
      }

      if (isStringType || !isNumericType) {
        return (
          <Input
            type="text"
            value={cond.threshold_value ?? String(cond.threshold ?? '')}
            onChange={(e) => {
              currentPath.length === 0
                ? onUpdate({ threshold_value: e.target.value })
                : onNestedUpdate(currentPath, { threshold_value: e.target.value })
            }}
            className="w-28 h-9"
            disabled={!hasDevice}
            placeholder="值..."
          />
        )
      }

      // Numeric input
      return (
        <Input
          type="number"
          value={cond.threshold ?? ''}
          onChange={(e) => {
            currentPath.length === 0
              ? onUpdate({ threshold: parseFloat(e.target.value) || 0 })
              : onNestedUpdate(currentPath, { threshold: parseFloat(e.target.value) || 0 })
          }}
          className="w-24 h-9"
          disabled={!hasDevice}
        />
      )
    }

    return (
      <div className="p-2.5 bg-gradient-to-r from-purple-500/10 to-transparent rounded-lg border border-purple-500/20">
        <div className="flex flex-wrap items-center gap-2">
          <Select
            value={cond.device_id}
            onValueChange={(v) => {
              const newMetrics = getDeviceMetrics(v, devices, deviceTypes)
              currentPath.length === 0
                ? onUpdate({ device_id: v, metric: newMetrics[0]?.name || 'value' })
                : onNestedUpdate(currentPath, { device_id: v, metric: newMetrics[0]?.name || 'value' })
            }}
          >
            <SelectTrigger className="w-36 h-8 text-sm flex-shrink-0"><SelectValue placeholder={t('automation:ruleBuilder.selectDevice')} /></SelectTrigger>
            <SelectContent>
              {deviceOptions.map(d => <SelectItem key={d.value} value={d.value}>{d.label}</SelectItem>)}
            </SelectContent>
          </Select>
          {hasDevice && hasMetrics ? (
            <Select
              value={cond.metric}
              onValueChange={(v) => {
                currentPath.length === 0 ? onUpdate({ metric: v }) : onNestedUpdate(currentPath, { metric: v })
              }}
            >
              <SelectTrigger className="w-32 h-8 text-sm flex-shrink-0"><SelectValue /></SelectTrigger>
              <SelectContent>
                {metrics.map(m => (
                  <SelectItem key={m.name} value={m.name}>
                    {m.display_name || m.name}
                    <span className="ml-1.5 text-xs text-muted-foreground/60">({m.data_type || 'unknown'})</span>
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          ) : hasDevice && !hasMetrics ? (
            <span className="text-xs text-muted-foreground italic">{t('automation:ruleBuilder.deviceNoMetrics')}</span>
          ) : (
            <span className="text-xs text-muted-foreground italic">{t('automation:ruleBuilder.selectDeviceFirst')}</span>
          )}
          <Select
            value={cond.operator}
            onValueChange={(v) => {
              currentPath.length === 0 ? onUpdate({ operator: v }) : onNestedUpdate(currentPath, { operator: v })
            }}
            disabled={!hasDevice}
          >
            <SelectTrigger className="w-20 h-8 text-sm flex-shrink-0"><SelectValue /></SelectTrigger>
            <SelectContent>
              {getComparisonOperators(t, metricDataType).map(o => <SelectItem key={o.value} value={o.value}>{o.symbol}</SelectItem>)}
            </SelectContent>
          </Select>
          {renderValueInput()}
          {currentPath.length === 0 && (
            <Button variant="ghost" size="icon" className="h-7 w-7 ml-auto flex-shrink-0" onClick={onReset}>
              <X className="h-3.5 w-3.5" />
            </Button>
          )}
        </div>
      </div>
    )
  }

  const renderRangeCondition = (cond: UICondition, currentPath: number[]) => {
    const metrics = getDeviceMetrics(cond.device_id || '', devices, deviceTypes)
    const hasDevice = Boolean(cond.device_id)
    const hasMetrics = metrics.length > 0

    return (
      <div className="p-2.5 bg-gradient-to-r from-blue-500/10 to-transparent rounded-lg border border-blue-500/20">
        <div className="flex flex-wrap items-center gap-2">
          <Badge variant="outline" className="text-xs bg-blue-500/20 text-blue-500 border-blue-500/30 flex-shrink-0">BETWEEN</Badge>
          <Select
            value={cond.device_id}
            onValueChange={(v) => {
              const newMetrics = getDeviceMetrics(v, devices, deviceTypes)
              currentPath.length === 0
                ? onUpdate({ device_id: v, metric: newMetrics[0]?.name || 'value' })
                : onNestedUpdate(currentPath, { device_id: v, metric: newMetrics[0]?.name || 'value' })
            }}
          >
            <SelectTrigger className="w-36 h-8 text-sm flex-shrink-0"><SelectValue placeholder={t('automation:ruleBuilder.selectDevice')} /></SelectTrigger>
            <SelectContent>
              {deviceOptions.map(d => <SelectItem key={d.value} value={d.value}>{d.label}</SelectItem>)}
            </SelectContent>
          </Select>
          {hasDevice && hasMetrics ? (
            <Select
              value={cond.metric}
              onValueChange={(v) => {
                currentPath.length === 0 ? onUpdate({ metric: v }) : onNestedUpdate(currentPath, { metric: v })
              }}
            >
              <SelectTrigger className="w-32 h-8 text-sm flex-shrink-0"><SelectValue /></SelectTrigger>
              <SelectContent>
                {metrics.map(m => <SelectItem key={m.name} value={m.name}>{m.display_name || m.name}</SelectItem>)}
              </SelectContent>
            </Select>
          ) : hasDevice && !hasMetrics ? (
            <span className="text-xs text-muted-foreground italic">{t('automation:ruleBuilder.deviceNoMetrics')}</span>
          ) : (
            <span className="text-xs text-muted-foreground italic">{t('automation:ruleBuilder.selectDeviceFirst')}</span>
          )}
          <span className="text-xs font-medium text-muted-foreground px-1">BETWEEN</span>
          <Input
            type="number"
            value={cond.range_min}
            onChange={(e) => {
              currentPath.length === 0
                ? onUpdate({ range_min: parseFloat(e.target.value) || 0 })
                : onNestedUpdate(currentPath, { range_min: parseFloat(e.target.value) || 0 })
            }}
            className="w-20 h-8 flex-shrink-0"
            placeholder="Min"
            disabled={!hasDevice}
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
            className="w-20 h-8 flex-shrink-0"
            placeholder="Max"
            disabled={!hasDevice}
          />
          {currentPath.length === 0 && (
            <Button variant="ghost" size="icon" className="h-7 w-7 ml-auto flex-shrink-0" onClick={onReset}>
              <X className="h-3.5 w-3.5" />
            </Button>
          )}
        </div>
      </div>
    )
  }

  const renderLogicalCondition = (cond: UICondition, currentPath: number[]) => {
    const label = cond.type.toUpperCase()
    const badgeClass = cond.type === 'and'
      ? 'bg-green-500/20 text-green-500 border-green-500/30'
      : cond.type === 'or'
      ? 'bg-amber-500/20 text-amber-500 border-amber-500/30'
      : 'bg-red-500/20 text-red-500 border-red-500/30'

    const connectorBadgeClass = cond.type === 'and'
      ? 'bg-green-100 text-green-700 dark:bg-green-900/30 dark:text-green-400'
      : cond.type === 'or'
      ? 'bg-amber-100 text-amber-700 dark:bg-amber-900/30 dark:text-amber-400'
      : 'bg-red-100 text-red-700 dark:bg-red-900/30 dark:text-red-400'

    const connectorText = cond.type === 'and' ? 'AND' : cond.type === 'or' ? 'OR' : 'NOT'

    return (
      <div className="space-y-3">
        {/* Header with operator badge and controls */}
        <div className="flex items-center gap-2 p-2.5 bg-muted/40 rounded-t-lg border border-border">
          <Badge variant="outline" className={cn('text-xs px-2.5 py-1', badgeClass)}>
            {label}
          </Badge>
          <span className="text-xs text-muted-foreground flex-1">
            {cond.type === 'and' ? t('automation:ruleBuilder.allConditionsMustMeet') : cond.type === 'or' ? t('automation:ruleBuilder.anyConditionMustMeet') : t('automation:ruleBuilder.conditionNotMet')}
          </span>
          {currentPath.length === 0 && (
            <Button variant="ghost" size="icon" className="h-6 w-6" onClick={onReset}>
              <X className="h-3 w-3" />
            </Button>
          )}
        </div>

        {/* Conditions container */}
        <div className="p-3 bg-background border border-t-0 border-border rounded-b-lg space-y-3">
          {cond.conditions?.map((subCond, i) => (
            <div key={subCond.id} className="relative group">
              {/* Connector badge before each condition (except first) */}
              {i > 0 && (
                <div className="flex items-center justify-start -mb-2 mt-1">
                  <span className={cn(
                    "text-xs font-semibold px-2.5 py-1 rounded-full",
                    connectorBadgeClass
                  )}>
                    {connectorText}
                  </span>
                </div>
              )}

              {/* Condition editor with wrapper */}
              <div className="relative pr-8">
                <div className={cn(
                  "rounded-lg",
                  subCond.type === 'and' || subCond.type === 'or' || subCond.type === 'not'
                    ? "bg-muted/30"
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
          <div className="pt-2 border-t border-border/50">
            <Button
              variant="outline"
              size="sm"
              className="w-full border-dashed h-9"
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
              <Plus className="h-3.5 w-3.5 mr-1" />{t('automation:ruleBuilder.addCondition')}
            </Button>
          </div>
        </div>
      </div>
    )
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

function ActionEditor({ action, devices, deviceTypes, onUpdate, onRemove }: ActionEditorProps) {
  const { t } = useTranslation(['automation'])
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

  const getActionBadgeClass = (): string => {
    switch (action.type) {
      case 'Execute': return 'bg-yellow-500/20 text-yellow-500 border-yellow-500/30'
      case 'Notify': return 'bg-blue-500/20 text-blue-500 border-blue-500/30'
      case 'Log': return 'bg-gray-500/20 text-gray-500 border-gray-500/30'
      case 'Set': return 'bg-purple-500/20 text-purple-500 border-purple-500/30'
      case 'Delay': return 'bg-orange-500/20 text-orange-500 border-orange-500/30'
      case 'CreateAlert': return 'bg-red-500/20 text-red-500 border-red-500/30'
      case 'HttpRequest': return 'bg-green-500/20 text-green-500 border-green-500/30'
      default: return 'bg-muted'
    }
  }

  const getActionLabel = (): string => {
    const actionType: string = (action as any).type
    switch (action.type) {
      case 'Execute': return t('automation:ruleBuilder.actionType.execute')
      case 'Notify': return t('automation:ruleBuilder.actionType.notify')
      case 'Log': return t('automation:ruleBuilder.actionType.log')
      case 'Set': return t('automation:ruleBuilder.actionType.set')
      case 'Delay': return t('automation:ruleBuilder.actionType.delay')
      case 'CreateAlert': return t('automation:ruleBuilder.actionType.createAlert')
      case 'HttpRequest': return 'HTTP'
    }
    return actionType
  }

  return (
    <div className="group flex items-center gap-2.5 p-2.5 bg-gradient-to-r from-green-500/10 to-transparent rounded-lg border border-green-500/20 hover:border-green-500/40 transition-colors">
      <div className="p-1.5 bg-background rounded shadow-sm flex-shrink-0">
        {getActionIcon()}
      </div>
      <Badge variant="outline" className={cn('text-xs px-2 py-0.5 flex-shrink-0', getActionBadgeClass())}>
        {getActionLabel()}
      </Badge>

      <div className="flex items-center gap-2 flex-wrap flex-1 min-w-0">
        {action.type === 'Execute' && (
          <>
            <Select
              value={action.device_id}
              onValueChange={(v) => {
                const commands = getDeviceCommands(v, devices, deviceTypes)
                onUpdate({ device_id: v, command: commands[0]?.name || 'turn_on' })
              }}
            >
              <SelectTrigger className="w-32 h-8 text-sm flex-shrink-0"><SelectValue /></SelectTrigger>
              <SelectContent>
                {deviceOptions.map(d => <SelectItem key={d.value} value={d.value}>{d.label}</SelectItem>)}
              </SelectContent>
            </Select>
            <Select
              value={action.command}
              onValueChange={(v) => onUpdate({ command: v })}
            >
              <SelectTrigger className="w-28 h-8 text-sm flex-shrink-0"><SelectValue /></SelectTrigger>
              <SelectContent>
                {getDeviceCommands(action.device_id, devices, deviceTypes).map(c => (
                  <SelectItem key={c.name} value={c.name}>{c.display_name || c.name}</SelectItem>
                ))}
              </SelectContent>
            </Select>
          </>
        )}

        {action.type === 'Notify' && (
          <Input
            value={action.message}
            onChange={(e) => onUpdate({ message: e.target.value })}
            placeholder={t('automation:ruleBuilder.notificationMessagePlaceholder')}
            className="flex-1 min-w-[120px] h-8 text-sm"
          />
        )}

        {action.type === 'Log' && (
          <>
            <Select value={action.level} onValueChange={(v: any) => onUpdate({ level: v })}>
              <SelectTrigger className="w-16 h-8 text-sm flex-shrink-0"><SelectValue /></SelectTrigger>
              <SelectContent>
                <SelectItem value="debug">{t('automation:logLevels.debug')}</SelectItem>
                <SelectItem value="info">{t('automation:logLevels.info')}</SelectItem>
                <SelectItem value="warn">{t('automation:logLevels.warn')}</SelectItem>
                <SelectItem value="error">{t('automation:logLevels.error')}</SelectItem>
              </SelectContent>
            </Select>
            <Input
              value={action.message}
              onChange={(e) => onUpdate({ message: e.target.value })}
              placeholder={t('automation:ruleBuilder.logContentPlaceholder')}
              className="flex-1 min-w-[120px] h-8 text-sm"
            />
          </>
        )}

        {action.type === 'Set' && (
          <>
            <Select value={action.device_id} onValueChange={(v) => onUpdate({ device_id: v })}>
              <SelectTrigger className="w-32 h-8 text-sm flex-shrink-0"><SelectValue /></SelectTrigger>
              <SelectContent>
                {deviceOptions.map(d => <SelectItem key={d.value} value={d.value}>{d.label}</SelectItem>)}
              </SelectContent>
            </Select>
            <Input
              value={action.property}
              onChange={(e) => onUpdate({ property: e.target.value })}
              placeholder={t('automation:ruleBuilder.propertyNamePlaceholder')}
              className="w-24 h-8 text-sm flex-shrink-0"
            />
            <span className="text-muted-foreground text-sm flex-shrink-0">=</span>
            <Input
              value={String(action.value ?? '')}
              onChange={(e) => onUpdate({ value: e.target.value })}
              placeholder={t('automation:ruleBuilder.valuePlaceholder')}
              className="w-24 h-8 text-sm flex-shrink-0"
            />
          </>
        )}

        {action.type === 'Delay' && (
          <>
            <Input
              type="number"
              value={(action.duration || 0) / 1000}
              onChange={(e) => onUpdate({ duration: (parseInt(e.target.value) || 0) * 1000 })}
              className="w-16 h-8 text-sm flex-shrink-0"
            />
            <span className="text-xs text-muted-foreground flex-shrink-0">{t('automation:ruleBuilder.seconds')}</span>
          </>
        )}

        {action.type === 'CreateAlert' && (
          <>
            <Input
              value={action.title}
              onChange={(e) => onUpdate({ title: e.target.value })}
              placeholder={t('automation:ruleBuilder.alertTitlePlaceholder')}
              className="w-28 h-8 text-sm flex-shrink-0"
            />
            <Input
              value={action.message}
              onChange={(e) => onUpdate({ message: e.target.value })}
              placeholder={t('automation:ruleBuilder.alertMessagePlaceholder')}
              className="flex-1 min-w-[80px] h-8 text-sm"
            />
            <Select value={action.severity} onValueChange={(v: any) => onUpdate({ severity: v })}>
              <SelectTrigger className="w-20 h-8 text-sm flex-shrink-0"><SelectValue /></SelectTrigger>
              <SelectContent>
                <SelectItem value="info">Info</SelectItem>
                <SelectItem value="warning">Warning</SelectItem>
                <SelectItem value="error">Error</SelectItem>
                <SelectItem value="critical">Critical</SelectItem>
              </SelectContent>
            </Select>
          </>
        )}

        {action.type === 'HttpRequest' && (
          <>
            <Select value={action.method} onValueChange={(v: any) => onUpdate({ method: v })}>
              <SelectTrigger className="w-20 h-8 text-sm flex-shrink-0"><SelectValue /></SelectTrigger>
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
              className="flex-1 min-w-[100px] h-8 text-sm"
            />
          </>
        )}
      </div>

      <Button variant="ghost" size="icon" className="h-7 w-7 flex-shrink-0 opacity-0 group-hover:opacity-100 transition-opacity" onClick={onRemove}>
        <X className="h-3.5 w-3.5" />
      </Button>
    </div>
  )
}
