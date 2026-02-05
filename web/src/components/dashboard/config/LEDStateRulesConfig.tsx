/**
 * LED State Rules Configuration
 *
 * Simplified configuration for LED indicator state mapping.
 * Each rule combines a match condition with state/label/color result.
 */

import { useState, useMemo } from 'react'
import { useTranslation } from 'react-i18next'
import { Plus, Trash2, ChevronDown, ChevronUp, GripVertical } from 'lucide-react'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'
import { CompactColorPicker, COLOR_PRESETS } from '@/components/ui/color-picker'
import { cn } from '@/lib/utils'
import type { StateRule, LEDState } from '@/components/dashboard/generic/LEDIndicator'

export interface LEDStateRulesConfigProps {
  rules: StateRule[]
  onChange: (rules: StateRule[]) => void
  readonly?: boolean
}

// State options with translations
function getStateOptions(t: (key: string) => string) {
  return [
    { value: 'on' as LEDState, label: t('stateMapping.on'), color: '#22c55e', icon: '●' },
    { value: 'off' as LEDState, label: t('stateMapping.off'), color: '#64748b', icon: '○' },
    { value: 'error' as LEDState, label: t('stateMapping.error'), color: '#ef4444', icon: '●' },
    { value: 'warning' as LEDState, label: t('stateMapping.warning'), color: '#f59e0b', icon: '●' },
    { value: 'unknown' as LEDState, label: t('stateMapping.unknown'), color: '#64748b', icon: '○' },
  ]
}

// Condition type
type ConditionType = 'threshold' | 'values' | 'pattern' | 'always'

function getConditionType(rule: StateRule): ConditionType {
  if (rule.threshold) return 'threshold'
  if (rule.pattern) return 'pattern'
  if (rule.values) return 'values'
  return 'always'
}

// Operator options
const OPERATOR_OPTIONS = [
  { value: '>', label: '>' },
  { value: '>=', label: '≥' },
  { value: '<', label: '<' },
  { value: '<=', label: '≤' },
  { value: '==', label: '=' },
  { value: '!=', label: '≠' },
]

export function LEDStateRulesConfig({
  rules,
  onChange,
  readonly = false,
}: LEDStateRulesConfigProps) {
  const { t } = useTranslation('dashboardComponents')
  const stateOptions = getStateOptions(t)

  // Track collapsed state for each rule
  const [collapsedIds, setCollapsedIds] = useState<Set<string>>(new Set())

  const toggleCollapse = (id: string) => {
    setCollapsedIds(prev => {
      const next = new Set(prev)
      if (next.has(id)) {
        next.delete(id)
      } else {
        next.add(id)
      }
      return next
    })
  }

  const addRule = (type: ConditionType) => {
    const newRule: StateRule = {
      state: 'on',
    }

    if (type === 'threshold') {
      newRule.threshold = { operator: '>', value: 50 }
    } else if (type === 'values') {
      newRule.values = ''
    } else if (type === 'pattern') {
      newRule.pattern = ''
    }
    // 'always' has no condition

    // Auto-expand new rule (don't add to collapsedIds)
    onChange([...rules, newRule])
  }

  const removeRule = (index: number) => {
    const updated = rules.filter((_, i) => i !== index)
    onChange(updated)
  }

  const updateRule = (index: number, updates: Partial<StateRule>) => {
    const updated = rules.map((r, i) => i === index ? { ...r, ...updates } : r)
    onChange(updated)
  }

  const stateInfo = useMemo(() => {
    const map = new Map<LEDState, typeof stateOptions[0]>()
    for (const opt of stateOptions) {
      map.set(opt.value, opt)
    }
    return map
  }, [stateOptions])

  return (
    <div className="space-y-3">
      {/* Header */}
      <div className="flex items-center justify-between">
        <Label className="text-sm font-medium">{t('stateMapping.title')}</Label>
        <div className="flex items-center gap-2">
          <span className="text-xs text-muted-foreground">
            {rules.length} {t('stateMapping.rules')}
          </span>
          <div className="flex gap-1">
            <Button
              variant="outline"
              size="sm"
              onClick={() => addRule('threshold')}
              disabled={readonly}
              className="h-7 text-xs"
            >
              <Plus className="h-3 w-3 mr-1" />
              {t('stateMapping.addThreshold')}
            </Button>
            <Button
              variant="outline"
              size="sm"
              onClick={() => addRule('values')}
              disabled={readonly}
              className="h-7 text-xs"
            >
              <Plus className="h-3 w-3 mr-1" />
              {t('stateMapping.addString')}
            </Button>
          </div>
        </div>
      </div>

      {/* Rules list */}
      <div className="space-y-2">
        {rules.length === 0 ? (
          <div className="text-center py-6 px-3 rounded-md border border-dashed text-muted-foreground text-sm">
            {t('stateMapping.noRules')}
            <p className="text-xs mt-1 opacity-70">{t('stateMapping.noRulesHint')}</p>
          </div>
        ) : (
          rules.map((rule, index) => {
            const conditionType = getConditionType(rule)
            const info = stateInfo.get(rule.state)
            const isCollapsed = collapsedIds.has(String(index))

            return (
              <div
                key={index}
                className="rounded-md border bg-background overflow-hidden"
              >
                {/* Header */}
                <div
                  className={cn(
                    "flex items-center gap-2 px-3 py-2 cursor-pointer transition-colors",
                    !isCollapsed && "bg-muted/30"
                  )}
                  onClick={() => toggleCollapse(String(index))}
                >
                  {/* Drag handle */}
                  <GripVertical className="h-4 w-4 text-muted-foreground/50 shrink-0" />

                  {/* Condition type badge */}
                  <span className={cn(
                    "text-xs font-medium px-1.5 py-0.5 rounded",
                    conditionType === 'threshold' && "bg-blue-500/10 text-blue-600",
                    conditionType === 'pattern' && "bg-purple-500/10 text-purple-600",
                    conditionType === 'values' && "bg-amber-500/10 text-amber-600",
                    conditionType === 'always' && "bg-gray-500/10 text-gray-600"
                  )}>
                    {conditionType === 'threshold' && t('stateMapping.thresholdLabel')}
                    {conditionType === 'pattern' && t('stateMapping.patternLabel')}
                    {conditionType === 'values' && t('stateMapping.stringLabel')}
                    {conditionType === 'always' && t('stateMapping.alwaysLabel')}
                  </span>

                  {/* Condition summary */}
                  <span className="text-xs text-muted-foreground flex-1 truncate">
                    {conditionType === 'threshold' && (
                      <>
                        {rule.threshold?.operator} {rule.threshold?.value}
                      </>
                    )}
                    {conditionType === 'values' && (
                      <>{rule.values || t('stateMapping.untitled')}</>
                    )}
                    {conditionType === 'pattern' && (
                      <>/{rule.pattern || ''}/</>
                    )}
                    {conditionType === 'always' && (
                      <>{t('stateMapping.always')}</>
                    )}
                  </span>

                  {/* State indicator with custom label/color indicators */}
                  <div className="flex items-center gap-2">
                    {rule.label && (
                      <span className="text-xs text-muted-foreground" title={t('stateMapping.customLabel')}>
                        🏷️
                      </span>
                    )}
                    {rule.color && (
                      <span
                        className="w-3 h-3 rounded-full border"
                        style={{ backgroundColor: rule.color }}
                        title={t('stateMapping.customColor')}
                      />
                    )}
                    <span className={cn("text-sm font-medium", info?.color)}>
                      {info?.icon} {info?.label}
                    </span>
                  </div>

                  {/* Expand/Collapse */}
                  {isCollapsed ? (
                    <ChevronDown className="h-4 w-4 text-muted-foreground shrink-0" />
                  ) : (
                    <ChevronUp className="h-4 w-4 text-muted-foreground shrink-0" />
                  )}

                  {/* Delete */}
                  <Button
                    variant="ghost"
                    size="sm"
                    onClick={(e) => {
                      e.stopPropagation()
                      removeRule(index)
                    }}
                    disabled={readonly}
                    className="h-7 w-7 p-0 shrink-0 text-muted-foreground hover:text-destructive"
                  >
                    <Trash2 className="h-3.5 w-3.5" />
                  </Button>
                </div>

                {/* Expanded content */}
                {!isCollapsed && (
                  <div className="px-3 py-3 border-t space-y-3">
                    {/* Condition input */}
                    <div className="space-y-1.5">
                      <span className="text-xs text-muted-foreground">
                        {conditionType === 'threshold' && t('stateMapping.thresholdLabel')}
                        {conditionType === 'values' && t('stateMapping.stringLabel')}
                        {conditionType === 'pattern' && t('stateMapping.patternLabel')}
                        {conditionType === 'always' && t('stateMapping.condition')}
                      </span>

                      {conditionType === 'threshold' ? (
                        <div className="flex items-center gap-2">
                          <Select
                            value={rule.threshold?.operator}
                            onValueChange={(val) => updateRule(index, {
                              threshold: { ...rule.threshold!, operator: val as any, value: rule.threshold!.value }
                            })}
                            disabled={readonly}
                          >
                            <SelectTrigger className="w-20 h-9 text-sm">
                              <SelectValue />
                            </SelectTrigger>
                            <SelectContent>
                              {OPERATOR_OPTIONS.map(opt => (
                                <SelectItem key={opt.value} value={opt.value}>
                                  {opt.label}
                                </SelectItem>
                              ))}
                            </SelectContent>
                          </Select>
                          <Input
                            type="number"
                            value={rule.threshold?.value ?? 0}
                            onChange={(e) => updateRule(index, {
                              threshold: { ...rule.threshold!, operator: rule.threshold!.operator, value: parseFloat(e.target.value) || 0 }
                            })}
                            disabled={readonly}
                            className="flex-1 h-9 text-sm"
                          />
                        </div>
                      ) : conditionType === 'pattern' ? (
                        <div className="flex items-center gap-2">
                          <span className="text-xs text-muted-foreground font-mono">/</span>
                          <Input
                            value={rule.pattern || ''}
                            onChange={(e) => updateRule(index, { pattern: e.target.value || undefined })}
                            placeholder={t('stateMapping.patternPlaceholder')}
                            disabled={readonly}
                            className="flex-1 h-9 text-sm font-mono"
                          />
                          <span className="text-xs text-muted-foreground font-mono">/</span>
                        </div>
                      ) : (
                        <Input
                          value={rule.values || ''}
                          onChange={(e) => updateRule(index, { values: e.target.value || undefined })}
                          placeholder={t('stateMapping.valuesPlaceholder')}
                          disabled={readonly}
                          className="h-9 text-sm"
                        />
                      )}
                    </div>

                    {/* State selector */}
                    <div className="space-y-1.5">
                      <span className="text-xs text-muted-foreground">{t('stateMapping.state')}</span>
                      <Select
                        value={rule.state}
                        onValueChange={(val) => updateRule(index, { state: val as LEDState })}
                        disabled={readonly}
                      >
                        <SelectTrigger className="h-9 w-full">
                          <SelectValue />
                        </SelectTrigger>
                        <SelectContent>
                          {stateOptions.map(opt => (
                            <SelectItem key={opt.value} value={opt.value}>
                              <span className={cn("flex items-center gap-2", opt.color)}>
                                {opt.icon} {opt.label}
                              </span>
                            </SelectItem>
                          ))}
                        </SelectContent>
                      </Select>
                    </div>

                    {/* Color picker */}
                    <div className="space-y-1.5">
                      <span className="text-xs text-muted-foreground">{t('stateMapping.customColor')} <span className="opacity-50">({t('common.optional')})</span></span>
                      <div className="flex items-center gap-2">
                        <CompactColorPicker
                          value={rule.color || stateInfo.get(rule.state)?.color || '#000000'}
                          onChange={(color) => updateRule(index, { color: color || undefined })}
                          presets={COLOR_PRESETS.semantic}
                        />
                        {rule.color && (
                          <span className="text-xs text-muted-foreground font-mono">{rule.color}</span>
                        )}
                      </div>
                    </div>

                    {/* Custom label */}
                    <div className="space-y-1.5">
                      <span className="text-xs text-muted-foreground">{t('stateMapping.customLabel')} <span className="opacity-50">({t('common.optional')})</span></span>
                      <Input
                        value={rule.label || ''}
                        onChange={(e) => updateRule(index, { label: e.target.value || undefined })}
                        placeholder={t('stateMapping.labelPlaceholder')}
                        disabled={readonly}
                        className="h-9 text-sm"
                      />
                    </div>
                  </div>
                )}
              </div>
            )
          })
        )}
      </div>

      {/* Hint */}
      <p className="text-xs text-muted-foreground">
        {t('stateMapping.hint')}
      </p>
    </div>
  )
}
