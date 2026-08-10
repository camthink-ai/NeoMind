/**
 * ValueMap Editor Component
 *
 * Allows users to map data values to LED states.
 * Used in LED Indicator configuration.
 */

import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { Plus, Trash2, GripVertical } from 'lucide-react'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'
import { Field } from '@/components/ui/field'
import { CompactColorPicker, COLOR_PRESETS } from '@/components/ui/color-picker'
import { cn } from '@/lib/utils'

export type LEDState = 'on' | 'off' | 'error' | 'warning' | 'unknown'

export interface ValueStateMapping {
  id: string
  values: string          // Comma-separated values, e.g., "1,true,yes"
  pattern?: string        // Regex pattern for matching
  state: LEDState
  label?: string          // Custom label for this state
  color?: string          // Custom color for this state
}

interface ValueMapEditorProps {
  valueMap: ValueStateMapping[]
  onChange: (valueMap: ValueStateMapping[]) => void
}

// State options factory (uses translations)
function getStateOptions(t: (key: string) => string) {
  return [
    { value: 'on' as LEDState, label: t('valueMap.state.on'), color: 'text-success' },
    { value: 'off' as LEDState, label: t('valueMap.state.off'), color: 'text-muted-foreground' },
    { value: 'error' as LEDState, label: t('valueMap.state.error'), color: 'text-error' },
    { value: 'warning' as LEDState, label: t('valueMap.state.warning'), color: 'text-warning' },
    { value: 'unknown' as LEDState, label: t('valueMap.state.unknown'), color: 'text-muted-foreground' },
  ]
}

export function ValueMapEditor({ valueMap, onChange }: ValueMapEditorProps) {
  const { t } = useTranslation('dashboardComponents')

  const stateOptions = getStateOptions(t)

  const [newMapping, setNewMapping] = useState<ValueStateMapping>({
    id: '',
    values: '',
    state: 'unknown',
  })

  const addMapping = () => {
    if (!newMapping.values.trim()) return

    const mapping: ValueStateMapping = {
      id: Date.now().toString(),
      values: newMapping.values,
      state: newMapping.state,
      label: newMapping.label,
      color: newMapping.color,
    }

    onChange([...valueMap, mapping])
    setNewMapping({ id: '', values: '', state: 'unknown' })
  }

  const removeMapping = (id: string) => {
    onChange(valueMap.filter(m => m.id !== id))
  }

  const updateMapping = (id: string, updates: Partial<ValueStateMapping>) => {
    onChange(valueMap.map(m => m.id === id ? { ...m, ...updates } : m))
  }

  return (
    <div className="space-y-3">
      <div className="flex items-center justify-between">
        <Label className="text-sm font-medium">{t('valueMap.title')}</Label>
        <span className="text-xs text-muted-foreground">
          {valueMap.length} {t('valueMap.rules')}
        </span>
      </div>

      {/* Existing mappings */}
      <div className="space-y-2">
        {valueMap.length === 0 ? (
          <div className="text-center py-4 px-3 rounded-md border border-dashed text-muted-foreground text-sm">
            {t('valueMap.noRules')}
          </div>
        ) : (
          valueMap.map((mapping, index) => {
            const stateInfo = stateOptions.find(s => s.value === mapping.state)
            return (
              <div
                key={mapping.id}
                className="flex items-start gap-2 p-3 rounded-md border bg-background group"
              >
                <div className="flex items-center gap-2 flex-1 min-w-0">
                  <GripVertical className="h-4 w-4 text-muted-foreground shrink-0 cursor-grab" />
                  <div className="flex-1 min-w-0 space-y-2">
                    {/* Values */}
                    <div className="flex items-center gap-2">
                      <span className="text-xs text-muted-foreground shrink-0">{t('valueMap.values')}:</span>
                      <Input
                        value={mapping.values}
                        onChange={(e) => updateMapping(mapping.id, { values: e.target.value })}
                        placeholder={t('valueMap.valuesPlaceholder')}
                        className="h-8 text-sm"
                      />
                    </div>

                    {/* State */}
                    <div className="flex items-center gap-2">
                      <span className="text-xs text-muted-foreground shrink-0">{t('valueMap.state')}:</span>
                      <Select
                        value={mapping.state}
                        onValueChange={(val) => updateMapping(mapping.id, { state: val as LEDState })}
                      >
                        <SelectTrigger className="h-8 text-sm">
                          <SelectValue />
                        </SelectTrigger>
                        <SelectContent>
                          {stateOptions.map(opt => (
                            <SelectItem key={opt.value} value={opt.value}>
                              <span className={cn(opt.color)}>{opt.label}</span>
                            </SelectItem>
                          ))}
                        </SelectContent>
                      </Select>
                    </div>

                    {/* Optional: Label */}
                    <div className="flex items-center gap-2">
                      <span className="text-xs text-muted-foreground shrink-0">{t('valueMap.label')}:</span>
                      <Input
                        value={mapping.label || ''}
                        onChange={(e) => updateMapping(mapping.id, { label: e.target.value || undefined })}
                        placeholder={t('valueMap.labelPlaceholder')}
                        className="h-8 text-sm"
                      />
                    </div>

                    {/* Optional: Color */}
                    <div className="flex items-center gap-1">
                      <span className="text-xs text-muted-foreground shrink-0">{t('valueMap.color')}:</span>
                      <CompactColorPicker
                        // Theme-foreground fallback (black in light / white in dark)
                        // instead of a hardcoded '#000000' — pure black is
                        // indistinguishable from "unset" and clips in dark mode.
                        value={mapping.color || 'hsl(var(--foreground))'}
                        onChange={(color) => updateMapping(mapping.id, { color: color || undefined })}
                        presets={COLOR_PRESETS.semantic}
                        className="flex-1"
                      />
                    </div>
                  </div>
                </div>

                <Button
                  variant="ghost"
                  size="sm"
                  onClick={() => removeMapping(mapping.id)}
                  className="shrink-0 h-8 w-8 p-0 text-muted-foreground hover:text-error"
                >
                  <Trash2 className="h-4 w-4" />
                </Button>
              </div>
            )
          })
        )}
      </div>

      {/* Add new mapping */}
      <div className="pt-2 border-t">
        <div className="flex items-center gap-2 mb-2">
          <Input
            value={newMapping.values}
            onChange={(e) => setNewMapping({ ...newMapping, values: e.target.value })}
            placeholder={t('valueMap.valuePlaceholder')}
            className="flex-1 h-9"
          />
          <Select
            value={newMapping.state}
            onValueChange={(val) => setNewMapping({ ...newMapping, state: val as LEDState })}
          >
            <SelectTrigger className="w-28 h-9">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              {stateOptions.map(opt => (
                <SelectItem key={opt.value} value={opt.value}>
                  {opt.label}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
          <Button
            onClick={addMapping}
            disabled={!newMapping.values.trim()}
            size="sm"
          >
            <Plus className="h-4 w-4" />
          </Button>
        </div>
        <p className="text-xs text-muted-foreground">
          {t('valueMap.hint')}
        </p>
      </div>
    </div>
  )
}
