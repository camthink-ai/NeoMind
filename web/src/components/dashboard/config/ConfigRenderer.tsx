/**
 * ConfigRenderer Component
 *
 * Renders configuration sections following system UI standards.
 * Uses Field components and default input/label styles.
 */

import { Label } from '@/components/ui/label'
import { Input } from '@/components/ui/input'
import { Switch } from '@/components/ui/switch'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'
import { Field } from '@/components/ui/field'
import { ColorPicker, CompactColorPicker, COLOR_PRESETS } from '@/components/ui/color-picker'
import { cn } from '@/lib/utils'
import { DataMappingConfig } from './UIConfigSections'
import type {
  ConfigSection as ConfigSectionType,
  DataSourceSection,
  ValueSection,
  RangeSection,
  SizeSection,
  ColorSection,
  MultiColorSection,
  LabelSection,
  BooleanSection,
  SelectSection,
  TextSection,
  OrientationSection,
  AnimationSection,
  DataMappingSection,
  CustomSection,
} from './ComponentConfigBuilder'

interface ConfigRendererProps {
  sections: ConfigSectionType[]
}

export function ConfigRenderer({ sections }: ConfigRendererProps) {
  if (sections.length === 0) return null

  return (
    <div className="space-y-3">
      {sections.map((section, index) => (
        <ConfigSectionItem key={`${section.type}-${index}`} section={section} />
      ))}
    </div>
  )
}

// ConfigSectionItem - NOT memoized to ensure updates are properly reflected
const ConfigSectionItem = function ConfigSectionItem({ section }: { section: ConfigSectionType }) {
  switch (section.type) {
    case 'data-source':
      return null // Handled separately by UnifiedDataSourceConfig

    case 'value': {
      const props = (section as ValueSection).props
      return (
        <div className="space-y-3">
          <Field>
            <Label>{props.label || '数值'} ({props.min ?? 0} - {props.max ?? 100})</Label>
            <Input
              type="number"
              min={props.min}
              max={props.max}
              step={props.step}
              value={props.value ?? 0}
              onChange={(e) => props.onChange?.(parseFloat(e.target.value) || 0)}
            />
          </Field>
          {props.unit !== undefined && (
            <Field>
              <Label>单位</Label>
              <Input
                value={props.unit}
                onChange={() => {}}
                placeholder="°C, %, kg..."
              />
            </Field>
          )}
          {props.showValue !== undefined && (
            <div className="flex items-center justify-between">
              <Label>显示数值</Label>
              <Switch
                checked={props.showValue}
                onCheckedChange={() => {}}
              />
            </div>
          )}
        </div>
      )
    }

    case 'range': {
      const props = (section as RangeSection).props
      return (
        <div className="space-y-3">
          <Field>
            <Label>最小值</Label>
            <Input
              type="number"
              value={props.min}
              onChange={(e) => props.onChange?.('min', parseFloat(e.target.value) || 0)}
            />
          </Field>
          <Field>
            <Label>最大值</Label>
            <Input
              type="number"
              value={props.max}
              onChange={(e) => props.onChange?.('max', parseFloat(e.target.value) || 0)}
            />
          </Field>
          <Field>
            <Label>步长</Label>
            <Input
              type="number"
              value={props.step}
              onChange={(e) => props.onChange?.('step', parseFloat(e.target.value) || 0)}
            />
          </Field>
        </div>
      )
    }

    case 'size': {
      const props = (section as SizeSection).props
      const sizeOptions = [
        { value: 'xs', label: '极小' },
        { value: 'sm', label: '小' },
        { value: 'md', label: '中' },
        { value: 'lg', label: '大' },
        { value: 'xl', label: '极大' },
      ]
      return (
        <Field>
          <Label>{props.label || '尺寸'}</Label>
          <Select value={props.size} onValueChange={props.onChange}>
            <SelectTrigger>
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              {sizeOptions.map((opt) => (
                <SelectItem key={opt.value} value={opt.value}>
                  {opt.label}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        </Field>
      )
    }

    case 'color': {
      const props = (section as ColorSection).props
      return (
        <ColorPicker
          value={props.color}
          onChange={props.onChange}
          label={props.label || '颜色'}
          presets="primary"
          disabled={false}
        />
      )
    }

    case 'multi-color': {
      const props = (section as MultiColorSection).props
      const colorFields: { key: string; label: string; defaultColor: string; presetKey?: keyof typeof COLOR_PRESETS }[] = [
        { key: 'primary', label: '主色', defaultColor: '#3b82f6', presetKey: 'primary' },
        { key: 'secondary', label: '辅色', defaultColor: '#8b5cf6', presetKey: 'primary' },
        { key: 'error', label: '错误色', defaultColor: '#ef4444', presetKey: 'semantic' },
        { key: 'warning', label: '警告色', defaultColor: '#eab308', presetKey: 'semantic' },
        { key: 'success', label: '成功色', defaultColor: '#22c55e', presetKey: 'semantic' },
      ]
      return (
        <div className="space-y-3">
          {colorFields.map((field) => {
            const colorValue = props.colors?.[field.key as keyof typeof props.colors] || field.defaultColor
            return (
              <Field key={field.key}>
                <Label>{field.label}</Label>
                <CompactColorPicker
                  value={colorValue}
                  onChange={(color) => props.onChange?.(field.key, color)}
                  presets={field.presetKey ? COLOR_PRESETS[field.presetKey] : COLOR_PRESETS.primary}
                  disabled={false}
                />
              </Field>
            )
          })}
        </div>
      )
    }

    case 'label': {
      const props = (section as LabelSection).props
      return (
        <div className="space-y-3">
          <Field>
            <Label>前缀</Label>
            <Input
              value={props.prefix}
              onChange={(e) => props.onChange?.('prefix', e.target.value)}
              placeholder="$"
            />
          </Field>
          <Field>
            <Label>后缀</Label>
            <Input
              value={props.suffix}
              onChange={(e) => props.onChange?.('suffix', e.target.value)}
              placeholder="kg"
            />
          </Field>
          <Field>
            <Label>单位</Label>
            <Input
              value={props.unit}
              onChange={(e) => props.onChange?.('unit', e.target.value)}
              placeholder="°C, %..."
            />
          </Field>
        </div>
      )
    }

    case 'boolean': {
      const props = (section as BooleanSection).props
      return (
        <div className="space-y-3">
          {props.options.map((option) => (
            <div key={option.key} className="flex items-center justify-between">
              <div className="flex flex-col gap-1">
                <Label htmlFor={option.key} className="cursor-pointer">
                  {option.label}
                </Label>
                {option.description && (
                  <span className="text-sm text-muted-foreground">
                    {option.description}
                  </span>
                )}
              </div>
              <Switch
                id={option.key}
                checked={option.value ?? false}
                onCheckedChange={(checked) => props.onChange?.(option.key, checked)}
              />
            </div>
          ))}
        </div>
      )
    }

    case 'select': {
      const props = (section as SelectSection).props
      return (
        <Field>
          <Label>{props.label}</Label>
          <Select value={props.value} onValueChange={props.onChange}>
            <SelectTrigger>
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              {props.options.map((opt) => (
                <SelectItem key={opt.value} value={opt.value}>
                  {opt.label}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        </Field>
      )
    }

    case 'text': {
      const props = (section as TextSection).props
      return (
        <Field>
          <Label>{props.label}</Label>
          <textarea
            className={cn(
              "flex min-h-[80px] w-full rounded-md border border-input bg-background px-3 py-2 text-sm",
              "ring-offset-background placeholder:text-muted-foreground",
              "focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2",
              "disabled:cursor-not-allowed disabled:opacity-50",
              "resize-none"
            )}
            value={props.content}
            onChange={(e) => props.onChange?.(e.target.value)}
            placeholder={props.placeholder || '输入内容...'}
            rows={props.rows || 3}
          />
        </Field>
      )
    }

    case 'orientation': {
      const props = (section as OrientationSection).props
      return (
        <Field>
          <Label>{props.label || '方向'}</Label>
          <Select value={props.orientation} onValueChange={props.onChange}>
            <SelectTrigger>
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              <SelectItem value="horizontal">水平</SelectItem>
              <SelectItem value="vertical">垂直</SelectItem>
            </SelectContent>
          </Select>
        </Field>
      )
    }

    case 'animation': {
      const props = (section as AnimationSection).props
      return (
        <div className="space-y-3">
          <div className="flex items-center justify-between">
            <Label>启用动画</Label>
            <Switch
              checked={props.animated}
              onCheckedChange={(checked) => props.onChange?.('animated', checked)}
            />
          </div>
          <Field>
            <Label>时长 (毫秒)</Label>
            <Input
              type="number"
              min={0}
              max={10000}
              step={100}
              value={props.duration}
              onChange={(e) => props.onChange?.('duration', parseInt(e.target.value) || 0)}
            />
          </Field>
        </div>
      )
    }

    case 'data-mapping': {
      const props = (section as DataMappingSection).props
      return (
        <DataMappingConfig
          dataMapping={props.dataMapping}
          onChange={props.onChange}
          mappingType={props.mappingType}
          label={props.label}
          readonly={false}
        />
      )
    }

    case 'custom':
      return <>{(section as CustomSection).render()}</>

    default:
      return null
  }
}
