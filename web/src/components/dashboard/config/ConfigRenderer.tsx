/**
 * ConfigRenderer Component
 *
 * Renders configuration sections following system UI standards.
 * Uses Field components and default input/label styles.
 */

import { useTranslation } from 'react-i18next'
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
  const { t } = useTranslation('dashboardComponents')

  switch (section.type) {
    case 'data-source':
      return null // Handled separately by UnifiedDataSourceConfig

    case 'value': {
      const props = (section as ValueSection).props
      return (
        <div className="space-y-3">
          <Field>
            <Label>{props.label || t('configRenderer.value')} ({props.min ?? 0} - {props.max ?? 100})</Label>
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
              <Label>{t('configRenderer.unit')}</Label>
              <Input
                value={props.unit}
                onChange={() => {}}
                placeholder={t('configRenderer.unitPlaceholder')}
              />
            </Field>
          )}
          {props.showValue !== undefined && (
            <div className="flex items-center justify-between">
              <Label>{t('configRenderer.showValue')}</Label>
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
            <Label>{t('configRenderer.minValue')}</Label>
            <Input
              type="number"
              value={props.min}
              onChange={(e) => props.onChange?.('min', parseFloat(e.target.value) || 0)}
            />
          </Field>
          <Field>
            <Label>{t('configRenderer.maxValue')}</Label>
            <Input
              type="number"
              value={props.max}
              onChange={(e) => props.onChange?.('max', parseFloat(e.target.value) || 0)}
            />
          </Field>
          <Field>
            <Label>{t('configRenderer.step')}</Label>
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
        { value: 'xs', label: t('sizes.xs') },
        { value: 'sm', label: t('sizes.sm') },
        { value: 'md', label: t('sizes.md') },
        { value: 'lg', label: t('sizes.lg') },
        { value: 'xl', label: t('sizes.xl') },
      ]
      return (
        <Field>
          <Label>{props.label || t('configRenderer.size')}</Label>
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
          label={props.label || t('configRenderer.color')}
          presets="primary"
          disabled={false}
        />
      )
    }

    case 'multi-color': {
      const props = (section as MultiColorSection).props
      const colorFields: { key: string; labelKey: string; defaultColor: string; presetKey?: keyof typeof COLOR_PRESETS }[] = [
        { key: 'primary', labelKey: 'backgroundColor', defaultColor: '#3b82f6', presetKey: 'primary' },
        { key: 'secondary', labelKey: 'textColor', defaultColor: '#8b5cf6', presetKey: 'primary' },
        { key: 'error', labelKey: 'borderColor', defaultColor: '#ef4444', presetKey: 'semantic' },
        { key: 'warning', labelKey: 'color', defaultColor: '#eab308', presetKey: 'semantic' },
        { key: 'success', labelKey: 'dataMapping', defaultColor: '#22c55e', presetKey: 'semantic' },
      ]
      return (
        <div className="space-y-3">
          {colorFields.map((field) => {
            const colorValue = props.colors?.[field.key as keyof typeof props.colors] || field.defaultColor
            return (
              <Field key={field.key}>
                <Label>{t(`configRenderer.${field.labelKey}`)}</Label>
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
            <Label>{t('configRenderer.label')}</Label>
            <Input
              value={props.prefix}
              onChange={(e) => props.onChange?.('prefix', e.target.value)}
              placeholder="$"
            />
          </Field>
          <Field>
            <Label>{t('configRenderer.label')}</Label>
            <Input
              value={props.suffix}
              onChange={(e) => props.onChange?.('suffix', e.target.value)}
              placeholder="kg"
            />
          </Field>
          <Field>
            <Label>{t('configRenderer.unit')}</Label>
            <Input
              value={props.unit}
              onChange={(e) => props.onChange?.('unit', e.target.value)}
              placeholder={t('configRenderer.unitPlaceholder')}
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
          <Label>{props.label || t('configRenderer.orientation')}</Label>
          <Select value={props.orientation} onValueChange={props.onChange}>
            <SelectTrigger>
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              <SelectItem value="horizontal">{t('configRenderer.orientationHorizontal')}</SelectItem>
              <SelectItem value="vertical">{t('configRenderer.orientationVertical')}</SelectItem>
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
            <Label>{t('configRenderer.animation')}</Label>
            <Switch
              checked={props.animated}
              onCheckedChange={(checked) => props.onChange?.('animated', checked)}
            />
          </div>
          <Field>
            <Label>{t('configRenderer.size')}</Label>
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
