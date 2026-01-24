/**
 * UIConfigSections Component
 *
 * Pre-built configuration sections for common UI patterns.
 * Provides consistent styling and validation for reusable config options.
 */

import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Switch } from '@/components/ui/switch'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'
import { ConfigSection } from './ConfigSection'

// ============================================================================
// Value Configuration Section
// ============================================================================

export interface ValueConfigProps {
  value?: number
  onChange?: (value: number) => void
  min?: number
  max?: number
  step?: number
  unit?: string
  showValue?: boolean
  label?: string
  readonly?: boolean
}

export function ValueConfig({
  value,
  onChange,
  min = 0,
  max = 100,
  step = 1,
  unit = '',
  showValue = true,
  label = 'Value',
  readonly = false,
}: ValueConfigProps) {
  return (
    <ConfigSection title={label} bordered>
      <div className="space-y-2">
        <Label>Static Value ({min} - {max})</Label>
        <Input
          type="number"
          min={min}
          max={max}
          step={step}
          value={value ?? 0}
          onChange={(e) => onChange?.(parseFloat(e.target.value) || 0)}
          disabled={readonly}
        />
      </div>
      {unit !== '' && (
        <div className="space-y-2">
          <Label>Unit</Label>
          <Input
            value={unit}
            onChange={() => {/* unit is passed as prop */}}
            placeholder="°C, %, kg..."
            disabled={readonly}
          />
        </div>
      )}
      <div className="flex items-center gap-2">
        <Switch
          id="show-value"
          checked={showValue}
          onCheckedChange={() => {/* readonly */}}
          disabled={readonly}
        />
        <Label htmlFor="show-value">Show Value</Label>
      </div>
    </ConfigSection>
  )
}

// ============================================================================
// Range Configuration Section (Min/Max/Step)
// ============================================================================

export interface RangeConfigProps {
  min?: number
  max?: number
  step?: number
  onChange?: (key: 'min' | 'max' | 'step', value: number) => void
  label?: string
  readonly?: boolean
}

export function RangeConfig({
  min = 0,
  max = 100,
  step = 1,
  onChange,
  label = 'Range',
  readonly = false,
}: RangeConfigProps) {
  return (
    <ConfigSection title={label} bordered>
      <div className="grid grid-cols-3 gap-2">
        <div className="space-y-2">
          <Label>Min</Label>
          <Input
            type="number"
            value={min}
            onChange={(e) => onChange?.('min', parseFloat(e.target.value) || 0)}
            disabled={readonly}
          />
        </div>
        <div className="space-y-2">
          <Label>Max</Label>
          <Input
            type="number"
            value={max}
            onChange={(e) => onChange?.('max', parseFloat(e.target.value) || 0)}
            disabled={readonly}
          />
        </div>
        <div className="space-y-2">
          <Label>Step</Label>
          <Input
            type="number"
            value={step}
            onChange={(e) => onChange?.('step', parseFloat(e.target.value) || 0)}
            disabled={readonly}
          />
        </div>
      </div>
    </ConfigSection>
  )
}

// ============================================================================
// Size Configuration Section
// ============================================================================

export type SizeOption = 'xs' | 'sm' | 'md' | 'lg' | 'xl' | '2xl'

export interface SizeConfigProps {
  size?: SizeOption
  onChange?: (size: SizeOption) => void
  options?: { value: SizeOption; label: string }[]
  label?: string
  readonly?: boolean
}

const defaultSizeOptions: { value: SizeOption; label: string }[] = [
  { value: 'xs', label: 'Extra Small' },
  { value: 'sm', label: 'Small' },
  { value: 'md', label: 'Medium' },
  { value: 'lg', label: 'Large' },
  { value: 'xl', label: 'Extra Large' },
  { value: '2xl', label: '2X Large' },
]

export function SizeConfig({
  size = 'md',
  onChange,
  options = defaultSizeOptions,
  label = 'Size',
  readonly = false,
}: SizeConfigProps) {
  return (
    <div className="space-y-2">
      <Label>{label}</Label>
      <Select value={size} onValueChange={(v: any) => onChange?.(v)} disabled={readonly}>
        <SelectTrigger>
          <SelectValue />
        </SelectTrigger>
        <SelectContent>
          {options.map((opt) => (
            <SelectItem key={opt.value} value={opt.value}>
              {opt.label}
            </SelectItem>
          ))}
        </SelectContent>
      </Select>
    </div>
  )
}

// ============================================================================
// Color Configuration Section
// ============================================================================

export interface ColorConfigProps {
  color?: string
  onChange?: (color: string) => void
  label?: string
  readonly?: boolean
}

export function ColorConfig({
  color = '#3b82f6',
  onChange,
  label = 'Color',
  readonly = false,
}: ColorConfigProps) {
  return (
    <div className="space-y-2">
      <Label>{label}</Label>
      <div className="flex items-center gap-2">
        <Input
          type="color"
          value={color}
          onChange={(e) => onChange?.(e.target.value)}
          className="h-10 w-16 p-1"
          disabled={readonly}
        />
        <Input
          type="text"
          value={color}
          onChange={(e) => onChange?.(e.target.value)}
          placeholder="#3b82f6"
          className="flex-1 font-mono text-sm"
          disabled={readonly}
        />
      </div>
    </div>
  )
}

// ============================================================================
// Multi-Color Configuration Section
// ============================================================================

export interface MultiColorConfigProps {
  colors?: {
    primary?: string
    secondary?: string
    error?: string
    warning?: string
    success?: string
  }
  onChange?: (key: string, color: string) => void
  label?: string
  readonly?: boolean
}

export function MultiColorConfig({
  colors = {},
  onChange,
  label = 'Colors',
  readonly = false,
}: MultiColorConfigProps) {
  const colorFields: { key: keyof typeof colors; label: string; defaultColor: string }[] = [
    { key: 'primary', label: 'Primary Color', defaultColor: '#3b82f6' },
    { key: 'secondary', label: 'Secondary Color', defaultColor: '#8b5cf6' },
    { key: 'error', label: 'Error Color', defaultColor: '#ef4444' },
    { key: 'warning', label: 'Warning Color', defaultColor: '#eab308' },
    { key: 'success', label: 'Success Color', defaultColor: '#22c55e' },
  ]

  return (
    <ConfigSection title={label} bordered>
      <div className="space-y-3">
        {colorFields.map((field) => (
          <ColorConfig
            key={field.key}
            label={field.label}
            color={colors[field.key] || field.defaultColor}
            onChange={(color) => onChange?.(field.key, color)}
            readonly={readonly}
          />
        ))}
      </div>
    </ConfigSection>
  )
}

// ============================================================================
// Label Configuration Section
// ============================================================================

export interface LabelConfigProps {
  prefix?: string
  suffix?: string
  unit?: string
  onChange?: (key: 'prefix' | 'suffix' | 'unit', value: string) => void
  label?: string
  readonly?: boolean
}

export function LabelConfig({
  prefix = '',
  suffix = '',
  unit = '',
  onChange,
  label = 'Labels',
  readonly = false,
}: LabelConfigProps) {
  return (
    <ConfigSection title={label} bordered>
      <div className="grid grid-cols-2 gap-2">
        <div className="space-y-2">
          <Label>Prefix</Label>
          <Input
            value={prefix}
            onChange={(e) => onChange?.('prefix', e.target.value)}
            placeholder="$"
            disabled={readonly}
          />
        </div>
        <div className="space-y-2">
          <Label>Suffix</Label>
          <Input
            value={suffix}
            onChange={(e) => onChange?.('suffix', e.target.value)}
            placeholder="kg"
            disabled={readonly}
          />
        </div>
      </div>
      <div className="space-y-2">
        <Label>Unit</Label>
        <Input
          value={unit}
          onChange={(e) => onChange?.('unit', e.target.value)}
          placeholder="°C, %..."
          disabled={readonly}
        />
      </div>
    </ConfigSection>
  )
}

// ============================================================================
// Boolean Options Configuration
// ============================================================================

export interface BooleanOption {
  key: string
  label: string
  value?: boolean
  description?: string
}

export interface BooleanConfigProps {
  options: BooleanOption[]
  onChange?: (key: string, value: boolean) => void
  label?: string
  readonly?: boolean
}

export function BooleanConfig({
  options,
  onChange,
  label = 'Options',
  readonly = false,
}: BooleanConfigProps) {
  return (
    <ConfigSection title={label} bordered>
      <div className="space-y-3">
        {options.map((option) => (
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
              onCheckedChange={(checked) => onChange?.(option.key, checked)}
              disabled={readonly}
            />
          </div>
        ))}
      </div>
    </ConfigSection>
  )
}

// ============================================================================
// Select Configuration Section
// ============================================================================

export interface SelectOption {
  value: string
  label: string
  description?: string
}

export interface SelectConfigProps {
  value?: string
  options: SelectOption[]
  onChange?: (value: string) => void
  label: string
  readonly?: boolean
}

export function SelectConfig({
  value,
  options,
  onChange,
  label,
  readonly = false,
}: SelectConfigProps) {
  return (
    <div className="space-y-2">
      <Label>{label}</Label>
      <Select value={value} onValueChange={onChange} disabled={readonly}>
        <SelectTrigger>
          <SelectValue />
        </SelectTrigger>
        <SelectContent>
          {options.map((opt) => (
            <SelectItem key={opt.value} value={opt.value}>
              {opt.label}
            </SelectItem>
          ))}
        </SelectContent>
      </Select>
    </div>
  )
}

// ============================================================================
// Text Content Configuration Section
// ============================================================================

export interface TextContentConfigProps {
  content?: string
  onChange?: (content: string) => void
  label?: string
  placeholder?: string
  rows?: number
  readonly?: boolean
  maxLength?: number
}

export function TextContentConfig({
  content = '',
  onChange,
  label = 'Content',
  placeholder = 'Enter content...',
  rows = 4,
  readonly = false,
  maxLength,
}: TextContentConfigProps) {
  return (
    <ConfigSection title={label} bordered>
      <div className="space-y-2">
        <textarea
          className="w-full min-h-[120px] p-2 border rounded-md text-sm"
          value={content}
          onChange={(e) => onChange?.(e.target.value)}
          placeholder={placeholder}
          rows={rows}
          disabled={readonly}
          maxLength={maxLength}
        />
        {maxLength && (
          <p className="text-xs text-muted-foreground text-right">
            {content?.length || 0} / {maxLength}
          </p>
        )}
      </div>
    </ConfigSection>
  )
}

// ============================================================================
// Orientation Configuration
// ============================================================================

export type Orientation = 'horizontal' | 'vertical'

export interface OrientationConfigProps {
  orientation?: Orientation
  onChange?: (orientation: Orientation) => void
  label?: string
  readonly?: boolean
}

export function OrientationConfig({
  orientation = 'horizontal',
  onChange,
  label = 'Orientation',
  readonly = false,
}: OrientationConfigProps) {
  return (
    <SelectConfig
      value={orientation}
      options={[
        { value: 'horizontal', label: 'Horizontal' },
        { value: 'vertical', label: 'Vertical' },
      ]}
      onChange={(v) => onChange?.(v as Orientation)}
      label={label}
      readonly={readonly}
    />
  )
}

// ============================================================================
// Animation Configuration
// ============================================================================

export interface AnimationConfigProps {
  duration?: number
  animated?: boolean
  onChange?: (key: 'duration' | 'animated', value: any) => void
  label?: string
  readonly?: boolean
}

export function AnimationConfig({
  duration = 1000,
  animated = true,
  onChange,
  label = 'Animation',
  readonly = false,
}: AnimationConfigProps) {
  return (
    <ConfigSection title={label} bordered>
      <BooleanConfig
        options={[
          {
            key: 'animated',
            label: 'Enable Animation',
            value: animated,
          },
        ]}
        onChange={(key, value) => onChange?.(key as 'animated', value)}
        readonly={readonly}
      />
      <div className="space-y-2">
        <Label>Duration (ms)</Label>
        <Input
          type="number"
          min={0}
          max={10000}
          step={100}
          value={duration}
          onChange={(e) => onChange?.('duration', parseInt(e.target.value) || 0)}
          disabled={readonly}
        />
      </div>
    </ConfigSection>
  )
}
