/**
 * ConfigRenderer Component
 *
 * Renders configuration sections from a ComponentConfigSchema.
 * Handles all section types and dispatches to appropriate UI components.
 */

import { Label } from '@/components/ui/label'
import { Input } from '@/components/ui/input'
import { Switch } from '@/components/ui/switch'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'
import { ConfigSection } from './ConfigSection'
import { DataSourceConfigSection } from './DataSourceConfigSection'
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
  CustomSection,
} from './ComponentConfigBuilder'

interface ConfigRendererProps {
  sections: ConfigSectionType[]
}

export function ConfigRenderer({ sections }: ConfigRendererProps) {
  return (
    <div className="space-y-4">
      {sections.map((section, index) => (
        <ConfigSectionItem key={index} section={section} />
      ))}
    </div>
  )
}

function ConfigSectionItem({ section }: { section: ConfigSectionType }) {
  switch (section.type) {
    case 'data-source':
      return <DataSourceConfigSection {...(section as DataSourceSection).props} />

    case 'value': {
      const props = (section as ValueSection).props
      return (
        <ConfigSection title={props.label || 'Value'} bordered>
          <div className="space-y-2">
            <Label>Static Value ({props.min ?? 0} - {props.max ?? 100})</Label>
            <Input
              type="number"
              min={props.min}
              max={props.max}
              step={props.step}
              value={props.value ?? 0}
              onChange={(e) => props.onChange?.(parseFloat(e.target.value) || 0)}
            />
          </div>
          {props.unit !== undefined && (
            <div className="space-y-2">
              <Label>Unit</Label>
              <Input
                value={props.unit}
                onChange={() => {
                  // Unit is typically handled separately
                }}
                placeholder="°C, %, kg..."
              />
            </div>
          )}
          {props.showValue !== undefined && (
            <div className="flex items-center gap-2">
              <Switch
                id="show-value"
                checked={props.showValue}
                onCheckedChange={() => {}}
              />
              <Label htmlFor="show-value">Show Value</Label>
            </div>
          )}
        </ConfigSection>
      )
    }

    case 'range': {
      const props = (section as RangeSection).props
      return (
        <ConfigSection title={props.label || 'Range'} bordered>
          <div className="grid grid-cols-3 gap-2">
            <div className="space-y-2">
              <Label>Min</Label>
              <Input
                type="number"
                value={props.min}
                onChange={(e) => props.onChange?.('min', parseFloat(e.target.value) || 0)}
              />
            </div>
            <div className="space-y-2">
              <Label>Max</Label>
              <Input
                type="number"
                value={props.max}
                onChange={(e) => props.onChange?.('max', parseFloat(e.target.value) || 0)}
              />
            </div>
            <div className="space-y-2">
              <Label>Step</Label>
              <Input
                type="number"
                value={props.step}
                onChange={(e) => props.onChange?.('step', parseFloat(e.target.value) || 0)}
              />
            </div>
          </div>
        </ConfigSection>
      )
    }

    case 'size': {
      const props = (section as SizeSection).props
      const sizeOptions = [
        { value: 'xs', label: 'Extra Small' },
        { value: 'sm', label: 'Small' },
        { value: 'md', label: 'Medium' },
        { value: 'lg', label: 'Large' },
        { value: 'xl', label: 'Extra Large' },
        { value: '2xl', label: '2X Large' },
      ]
      return (
        <div className="space-y-2">
          <Label>{props.label || 'Size'}</Label>
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
        </div>
      )
    }

    case 'color': {
      const props = (section as ColorSection).props
      return (
        <div className="space-y-2">
          <Label>{props.label || 'Color'}</Label>
          <div className="flex items-center gap-2">
            <Input
              type="color"
              value={props.color}
              onChange={(e) => props.onChange?.(e.target.value)}
              className="h-10 w-16 p-1"
            />
            <Input
              type="text"
              value={props.color}
              onChange={(e) => props.onChange?.(e.target.value)}
              placeholder="#3b82f6"
              className="flex-1 font-mono text-sm"
            />
          </div>
        </div>
      )
    }

    case 'multi-color': {
      const props = (section as MultiColorSection).props
      const colorFields: { key: string; label: string; defaultColor: string }[] = [
        { key: 'primary', label: 'Primary Color', defaultColor: '#3b82f6' },
        { key: 'secondary', label: 'Secondary Color', defaultColor: '#8b5cf6' },
        { key: 'error', label: 'Error Color', defaultColor: '#ef4444' },
        { key: 'warning', label: 'Warning Color', defaultColor: '#eab308' },
        { key: 'success', label: 'Success Color', defaultColor: '#22c55e' },
      ]
      return (
        <ConfigSection title={props.label || 'Colors'} bordered>
          <div className="space-y-3">
            {colorFields.map((field) => {
              const colorValue = props.colors?.[field.key as keyof typeof props.colors] || field.defaultColor
              return (
                <div key={field.key} className="space-y-2">
                  <Label>{field.label}</Label>
                  <div className="flex items-center gap-2">
                    <Input
                      type="color"
                      value={colorValue}
                      onChange={(e) => props.onChange?.(field.key, e.target.value)}
                      className="h-8 w-12 p-0.5"
                    />
                    <Input
                      type="text"
                      value={colorValue}
                      onChange={(e) => props.onChange?.(field.key, e.target.value)}
                      className="flex-1 font-mono text-sm h-8"
                    />
                  </div>
                </div>
              )
            })}
          </div>
        </ConfigSection>
      )
    }

    case 'label': {
      const props = (section as LabelSection).props
      return (
        <ConfigSection title={props.label || 'Labels'} bordered>
          <div className="grid grid-cols-2 gap-2">
            <div className="space-y-2">
              <Label>Prefix</Label>
              <Input
                value={props.prefix}
                onChange={(e) => props.onChange?.('prefix', e.target.value)}
                placeholder="$"
              />
            </div>
            <div className="space-y-2">
              <Label>Suffix</Label>
              <Input
                value={props.suffix}
                onChange={(e) => props.onChange?.('suffix', e.target.value)}
                placeholder="kg"
              />
            </div>
          </div>
          <div className="space-y-2">
            <Label>Unit</Label>
            <Input
              value={props.unit}
              onChange={(e) => props.onChange?.('unit', e.target.value)}
              placeholder="°C, %..."
            />
          </div>
        </ConfigSection>
      )
    }

    case 'boolean': {
      const props = (section as BooleanSection).props
      return (
        <ConfigSection title={props.label || 'Options'} bordered>
          <div className="space-y-3">
            {props.options.map((option) => (
              <div key={option.key} className="flex items-center justify-between">
                <div className="flex flex-col gap-0.5">
                  <Label htmlFor={option.key} className="cursor-pointer">
                    {option.label}
                  </Label>
                  {option.description && (
                    <span className="text-xs text-muted-foreground">
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
        </ConfigSection>
      )
    }

    case 'select': {
      const props = (section as SelectSection).props
      return (
        <div className="space-y-2">
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
        </div>
      )
    }

    case 'text': {
      const props = (section as TextSection).props
      return (
        <ConfigSection title={props.label} bordered>
          <div className="space-y-2">
            <textarea
              className="w-full min-h-[120px] p-2 border rounded-md text-sm font-mono"
              value={props.content}
              onChange={(e) => props.onChange?.(e.target.value)}
              placeholder={props.placeholder || 'Enter content...'}
              rows={props.rows || 4}
            />
          </div>
        </ConfigSection>
      )
    }

    case 'orientation': {
      const props = (section as OrientationSection).props
      return (
        <div className="space-y-2">
          <Label>{props.label || 'Orientation'}</Label>
          <Select value={props.orientation} onValueChange={props.onChange}>
            <SelectTrigger>
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              <SelectItem value="horizontal">Horizontal</SelectItem>
              <SelectItem value="vertical">Vertical</SelectItem>
            </SelectContent>
          </Select>
        </div>
      )
    }

    case 'animation': {
      const props = (section as AnimationSection).props
      return (
        <ConfigSection title={props.label || 'Animation'} bordered>
          <div className="flex items-center justify-between">
            <Label>Enable Animation</Label>
            <Switch
              checked={props.animated}
              onCheckedChange={(checked) => props.onChange?.('animated', checked)}
            />
          </div>
          <div className="space-y-2">
            <Label>Duration (ms)</Label>
            <Input
              type="number"
              min={0}
              max={10000}
              step={100}
              value={props.duration}
              onChange={(e) => props.onChange?.('duration', parseInt(e.target.value) || 0)}
            />
          </div>
        </ConfigSection>
      )
    }

    case 'custom':
      return <>{(section as CustomSection).render()}</>

    default:
      return null
  }
}
