/**
 * ComponentConfigBuilder
 *
 * Fluent API for building component configuration panels.
 * Provides a standardized way to construct config dialogs with sections.
 */

import { useTranslation } from 'react-i18next'
import type { DataSource, DataSourceOrList } from '@/types/dashboard'
import type {
  SizeOption,
  BooleanOption,
  SelectOption,
  Orientation,
  DataMappingType,
} from './UIConfigSections'
import type { SingleValueMappingConfig } from '@/lib/dataMapping'

// ============================================================================
// Configuration Schema Types
// ============================================================================

export interface ComponentConfigSchema {
  title?: string
  // Separated sections for three-column layout UI
  dataSourceSections?: ConfigSection[]
  styleSections?: ConfigSection[]
  displaySections?: ConfigSection[]  // NEW: Display-specific configuration (labels, legends, axes, etc.)
  // Legacy support - all sections combined
  sections?: ConfigSection[]
}

export type ConfigSection =
  | DataSourceSection
  | ValueSection
  | RangeSection
  | SizeSection
  | ColorSection
  | MultiColorSection
  | LabelSection
  | BooleanSection
  | SelectSection
  | TextSection
  | OrientationSection
  | AnimationSection
  | DataMappingSection
  | CustomSection

export interface DataSourceSection {
  type: 'data-source'
  props: {
    dataSource?: DataSourceOrList
    onChange: (dataSource: DataSourceOrList | DataSource | undefined) => void
    // Optional: filter which source types to show
    allowedTypes?: Array<'device-metric' | 'device-command' | 'device-info' | 'device' | 'metric' | 'command' | 'system'>
    // Optional: enable multiple data source selection
    multiple?: boolean
    // Optional: max number of data sources (only used when multiple is true)
    maxSources?: number
  }
}

export interface ValueSection {
  type: 'value'
  props: {
    value?: number
    onChange?: (value: number) => void
    min?: number
    max?: number
    step?: number
    unit?: string
    showValue?: boolean
    label?: string
  }
}

export interface RangeSection {
  type: 'range'
  props: {
    min?: number
    max?: number
    step?: number
    onChange?: (key: 'min' | 'max' | 'step', value: number) => void
    label?: string
  }
}

export interface SizeSection {
  type: 'size'
  props: {
    size?: SizeOption
    onChange?: (size: SizeOption) => void
    label?: string
  }
}

export interface ColorSection {
  type: 'color'
  props: {
    color?: string
    onChange?: (color: string) => void
    label?: string
  }
}

export interface MultiColorSection {
  type: 'multi-color'
  props: {
    colors?: {
      primary?: string
      secondary?: string
      error?: string
      warning?: string
      success?: string
    }
    onChange?: (key: string, color: string) => void
    label?: string
  }
}

export interface LabelSection {
  type: 'label'
  props: {
    prefix?: string
    suffix?: string
    unit?: string
    onChange?: (key: 'prefix' | 'suffix' | 'unit', value: string) => void
    label?: string
  }
}

export interface BooleanSection {
  type: 'boolean'
  props: {
    options: BooleanOption[]
    onChange?: (key: string, value: boolean) => void
    label?: string
  }
}

export interface SelectSection {
  type: 'select'
  props: {
    value?: string
    options: SelectOption[]
    onChange?: (value: string) => void
    label: string
  }
}

export interface TextSection {
  type: 'text'
  props: {
    content?: string
    onChange?: (content: string) => void
    label?: string
    placeholder?: string
    rows?: number
  }
}

export interface OrientationSection {
  type: 'orientation'
  props: {
    orientation?: Orientation
    onChange?: (orientation: Orientation) => void
    label?: string
  }
}

export interface AnimationSection {
  type: 'animation'
  props: {
    duration?: number
    animated?: boolean
    onChange?: (key: 'duration' | 'animated', value: any) => void
    label?: string
  }
}

export interface DataMappingSection {
  type: 'data-mapping'
  props: {
    dataMapping?: SingleValueMappingConfig
    onChange?: (config: SingleValueMappingConfig | undefined) => void
    mappingType: DataMappingType
    label?: string
  }
}

export interface CustomSection {
  type: 'custom'
  render: () => React.ReactNode
}

// ============================================================================
// Configuration Schema Registry
// ============================================================================

const componentConfigSchemas: Record<string, () => ComponentConfigSchema> = {}

export function registerComponentConfig(
  type: string,
  schemaBuilder: () => ComponentConfigSchema
) {
  componentConfigSchemas[type] = schemaBuilder
}

export function getComponentConfig(type: string): ComponentConfigSchema | null {
  const builder = componentConfigSchemas[type]
  return builder ? builder() : null
}

// ============================================================================
// Default Config Schema Builders
// ============================================================================

/**
 * Create a standard schema for data-display components
 * (Counter, ValueCard, MetricCard, etc.)
 */
export function createDataDisplayConfig(config: {
  dataSource?: DataSourceOrList
  onDataSourceChange: (ds: DataSourceOrList | DataSource | undefined) => void
  value?: number
  onValueChange?: (value: number) => void
  unit?: string
  onUnitChange?: (unit: string) => void
  prefix?: string
  onPrefixChange?: (prefix: string) => void
  suffix?: string
  onSuffixChange?: (suffix: string) => void
  decimals?: number
  onDecimalsChange?: (decimals: number) => void
  size?: SizeOption
  onSizeChange?: (size: SizeOption) => void
  color?: string
  onColorChange?: (color: string) => void
  showTrend?: boolean
  onShowTrendChange?: (show: boolean) => void
  showChange?: boolean
  onShowChangeChange?: (show: boolean) => void
}): ComponentConfigSchema {
  // Data Source Sections
  const dataSourceSections: ConfigSection[] = [{
    type: 'data-source',
    props: {
      dataSource: config.dataSource,
      onChange: config.onDataSourceChange,
    },
  }]

  // Style Sections
  const styleSections: ConfigSection[] = []

  // Value Section (if change handler provided)
  if (config.onValueChange) {
    styleSections.push({
      type: 'value',
      props: {
        value: config.value,
        onChange: config.onValueChange,
        label: 'Static Value',
      },
    })
  }

  // Labels Section
  if (config.onPrefixChange || config.onSuffixChange || config.onUnitChange) {
    styleSections.push({
      type: 'label',
      props: {
        prefix: config.prefix,
        suffix: config.suffix,
        unit: config.unit,
        onChange: (key, value) => {
          if (key === 'prefix') config.onPrefixChange?.(value)
          if (key === 'suffix') config.onSuffixChange?.(value)
          if (key === 'unit') config.onUnitChange?.(value)
        },
      },
    })
  }

  // Decimals Section
  if (config.onDecimalsChange) {
    styleSections.push({
      type: 'custom',
      render: () => (
        <div className="space-y-2">
          <label className="text-sm font-medium">Decimal Places</label>
          <input
            type="number"
            min={0}
            max={6}
            value={config.decimals ?? 0}
            onChange={(e) => config.onDecimalsChange?.(parseInt(e.target.value) || 0)}
            className="w-full h-10 px-3 rounded-md border border-input bg-background"
          />
        </div>
      ),
    })
  }

  // Size Section
  if (config.onSizeChange) {
    styleSections.push({
      type: 'size',
      props: {
        size: config.size,
        onChange: config.onSizeChange,
      },
    })
  }

  // Color Section
  if (config.onColorChange) {
    styleSections.push({
      type: 'color',
      props: {
        color: config.color,
        onChange: config.onColorChange,
      },
    })
  }

  // Boolean Options
  if (config.onShowTrendChange || config.onShowChangeChange) {
    const options: BooleanOption[] = []
    if (config.onShowTrendChange) {
      options.push({ key: 'showTrend', label: 'Show Trend', value: config.showTrend })
    }
    if (config.onShowChangeChange) {
      options.push({ key: 'showChange', label: 'Show Change', value: config.showChange })
    }
    styleSections.push({
      type: 'boolean',
      props: {
        options,
        onChange: (key, value) => {
          if (key === 'showTrend') config.onShowTrendChange?.(value)
          if (key === 'showChange') config.onShowChangeChange?.(value)
        },
      },
    })
  }

  return { dataSourceSections, styleSections }
}

/**
 * Create a standard schema for progress-based components
 * (ProgressBar, ProgressRing, GaugeChart, etc.)
 */
export function createProgressConfig(config: {
  dataSource?: DataSourceOrList
  onDataSourceChange: (ds: DataSourceOrList | DataSource | undefined) => void
  label?: string
  onLabelChange?: (label: string) => void
  value?: number
  onValueChange?: (value: number) => void
  min?: number
  onMinChange?: (min: number) => void
  max?: number
  onMaxChange?: (max: number) => void
  size?: SizeOption
  onSizeChange?: (size: SizeOption) => void
  color?: string
  onColorChange?: (color: string) => void
  showValue?: boolean
  onShowValueChange?: (show: boolean) => void
  showPercentage?: boolean
  onShowPercentageChange?: (show: boolean) => void
}): ComponentConfigSchema {
  const sections: ConfigSection[] = []

  sections.push({
    type: 'data-source',
    props: { dataSource: config.dataSource, onChange: config.onDataSourceChange },
  })

  // Label input
  if (config.onLabelChange) {
    sections.push({
      type: 'custom',
      render: () => {
        const { t } = useTranslation('dashboardComponents')
        return (
          <div className="space-y-2 pt-4 border-t">
            <label className="text-sm font-medium">{t('common.label')}</label>
            <input
              type="text"
              value={config.label ?? ''}
              onChange={(e) => config.onLabelChange?.(e.target.value)}
              placeholder={t('placeholders.cpuUsage')}
              className="w-full h-10 px-3 rounded-md border border-input bg-background"
            />
          </div>
        )
      },
    })
  }

  if (config.onValueChange || config.onMinChange || config.onMaxChange) {
    sections.push({
      type: 'custom',
      render: () => (
        <div className="space-y-3 pt-4 border-t">
          {config.onValueChange && (
            <div className="space-y-2">
              <label className="text-sm font-medium">Value (0-{config.max ?? 100})</label>
              <input
                type="number"
                min={config.min ?? 0}
                max={config.max ?? 100}
                value={config.value ?? 0}
                onChange={(e) => config.onValueChange?.(parseFloat(e.target.value) || 0)}
                className="w-full h-10 px-3 rounded-md border border-input bg-background"
              />
            </div>
          )}
          {(config.onMinChange || config.onMaxChange) && (
            <div className="grid grid-cols-2 gap-2">
              {config.onMinChange && (
                <div className="space-y-2">
                  <label className="text-sm font-medium">Min</label>
                  <input
                    type="number"
                    value={config.min ?? 0}
                    onChange={(e) => config.onMinChange?.(parseFloat(e.target.value) || 0)}
                    className="w-full h-10 px-3 rounded-md border border-input bg-background"
                  />
                </div>
              )}
              {config.onMaxChange && (
                <div className="space-y-2">
                  <label className="text-sm font-medium">Max</label>
                  <input
                    type="number"
                    value={config.max ?? 100}
                    onChange={(e) => config.onMaxChange?.(parseFloat(e.target.value) || 0)}
                    className="w-full h-10 px-3 rounded-md border border-input bg-background"
                  />
                </div>
              )}
            </div>
          )}
        </div>
      ),
    })
  }

  if (config.onSizeChange) {
    sections.push({
      type: 'size',
      props: { size: config.size, onChange: config.onSizeChange },
    })
  }

  if (config.onColorChange) {
    sections.push({
      type: 'color',
      props: { color: config.color, onChange: config.onColorChange },
    })
  }

  const boolOptions: BooleanOption[] = []
  if (config.onShowValueChange) {
    boolOptions.push({ key: 'showValue', label: 'Show Value', value: config.showValue })
  }
  if (config.onShowPercentageChange) {
    boolOptions.push({ key: 'showPercentage', label: 'Show Percentage', value: config.showPercentage })
  }
  if (boolOptions.length > 0) {
    sections.push({
      type: 'boolean',
      props: {
        options: boolOptions,
        onChange: (key, value) => {
          if (key === 'showValue') config.onShowValueChange?.(value)
          if (key === 'showPercentage') config.onShowPercentageChange?.(value)
        },
      },
    })
  }

  return { sections }
}

/**
 * Create a standard schema for control components
 * (Slider, ToggleSwitch, ButtonGroup, etc.)
 */
export function createControlConfig(config: {
  dataSource?: DataSourceOrList
  onDataSourceChange: (ds: DataSourceOrList | DataSource | undefined) => void
  value?: number | boolean
  onValueChange?: (value: number | boolean) => void
  min?: number
  onMinChange?: (min: number) => void
  max?: number
  onMaxChange?: (max: number) => void
  step?: number
  onStepChange?: (step: number) => void
  unit?: string
  onUnitChange?: (unit: string) => void
  size?: SizeOption
  onSizeChange?: (size: SizeOption) => void
  color?: string
  onColorChange?: (color: string) => void
  disabled?: boolean
  onDisabledChange?: (disabled: boolean) => void
}): ComponentConfigSchema {
  const sections: ConfigSection[] = []

  // Control components can use device telemetry and commands
  sections.push({
    type: 'data-source',
    props: {
      dataSource: config.dataSource,
      onChange: config.onDataSourceChange,
      allowedTypes: ['device', 'command'],
    },
  })

  if (config.onValueChange !== undefined) {
    sections.push({
      type: 'custom',
      render: () => (
        <div className="space-y-2 pt-4 border-t">
          <label className="text-sm font-medium">
            Default Value {typeof config.value === 'number' ? `(${config.min ?? 0} - ${config.max ?? 100})` : ''}
          </label>
          {typeof config.value === 'boolean' ? (
            <div className="flex items-center gap-2">
              <input
                type="checkbox"
                checked={config.value}
                onChange={(e) => config.onValueChange?.(e.target.checked)}
                className="rounded"
              />
              <label className="text-sm">Checked by default</label>
            </div>
          ) : (
            <input
              type="number"
              min={config.min ?? 0}
              max={config.max ?? 100}
              value={config.value ?? 0}
              onChange={(e) => config.onValueChange?.(parseFloat(e.target.value) || 0)}
              className="w-full h-10 px-3 rounded-md border border-input bg-background"
            />
          )}
        </div>
      ),
    })
  }

  if (config.onMinChange || config.onMaxChange || config.onStepChange) {
    sections.push({
      type: 'range',
      props: {
        min: config.min,
        max: config.max,
        step: config.step,
        onChange: (key, value) => {
          if (key === 'min') config.onMinChange?.(value)
          if (key === 'max') config.onMaxChange?.(value)
          if (key === 'step') config.onStepChange?.(value)
        },
      },
    })
  }

  if (config.onUnitChange) {
    sections.push({
      type: 'custom',
      render: () => {
        const { t } = useTranslation('dashboardComponents')
        return (
          <div className="space-y-2">
            <label className="text-sm font-medium">{t('common.unit')}</label>
            <input
              type="text"
              value={config.unit ?? ''}
              onChange={(e) => config.onUnitChange?.(e.target.value)}
              placeholder={t('placeholders.percentSymbol')}
              className="w-full h-10 px-3 rounded-md border border-input bg-background"
            />
          </div>
        )
      },
    })
  }

  if (config.onSizeChange) {
    sections.push({ type: 'size', props: { size: config.size, onChange: config.onSizeChange } })
  }

  if (config.onColorChange) {
    sections.push({ type: 'color', props: { color: config.color, onChange: config.onColorChange } })
  }

  if (config.onDisabledChange) {
    sections.push({
      type: 'boolean',
      props: {
        options: [{ key: 'disabled', label: 'Disabled', value: config.disabled }],
        onChange: (_, value) => config.onDisabledChange?.(value),
      },
    })
  }

  return { sections }
}

/**
 * Create a standard schema for indicator components
 * (LEDIndicator, StatusBadge, etc.)
 */
export function createIndicatorConfig(config: {
  dataSource?: DataSourceOrList
  onDataSourceChange: (ds: DataSourceOrList | DataSource | undefined) => void
  state?: string
  onStateChange?: (state: string) => void
  size?: SizeOption
  onSizeChange?: (size: SizeOption) => void
  colors?: {
    on?: string
    off?: string
    error?: string
    warning?: string
  }
  onColorChange?: (key: string, color: string) => void
  blink?: boolean
  onBlinkChange?: (blink: boolean) => void
}): ComponentConfigSchema {
  const sections: ConfigSection[] = []

  sections.push({
    type: 'data-source',
    props: { dataSource: config.dataSource, onChange: config.onDataSourceChange },
  })

  if (config.onStateChange) {
    sections.push({
      type: 'select',
      props: {
        value: config.state ?? 'on',
        options: [
          { value: 'on', label: 'On' },
          { value: 'off', label: 'Off' },
          { value: 'blinking', label: 'Blinking' },
          { value: 'error', label: 'Error' },
          { value: 'warning', label: 'Warning' },
        ],
        onChange: config.onStateChange,
        label: 'Default State',
      },
    })
  }

  if (config.onSizeChange) {
    sections.push({ type: 'size', props: { size: config.size, onChange: config.onSizeChange } })
  }

  if (config.onColorChange && config.colors) {
    sections.push({
      type: 'multi-color',
      props: {
        colors: {
          primary: config.colors.on,
          error: config.colors.error,
          warning: config.colors.warning,
        },
        onChange: (key, color) => {
          const colorKey = key === 'primary' ? 'on' : key
          config.onColorChange?.(colorKey, color)
        },
      },
    })
  }

  if (config.onBlinkChange) {
    sections.push({
      type: 'boolean',
      props: {
        options: [{ key: 'blink', label: 'Blinking', value: config.blink }],
        onChange: (_, value) => config.onBlinkChange?.(value),
      },
    })
  }

  return { sections }
}

/**
 * Create a standard schema for content components
 * (TextBlock, CodeBlock, Image, Video, etc.)
 */
export function createContentConfig(config: {
  content?: string
  onContentChange?: (content: string) => void
  src?: string
  onSrcChange?: (src: string) => void
  alt?: string
  onAltChange?: (alt: string) => void
  language?: string
  onLanguageChange?: (language: string) => void
  variant?: string
  onVariantChange?: (variant: string) => void
  align?: string
  onAlignChange?: (align: string) => void
  color?: string
  onColorChange?: (color: string) => void
  showLineNumbers?: boolean
  onShowLineNumbersChange?: (show: boolean) => void
  autoplay?: boolean
  onAutoplayChange?: (autoplay: boolean) => void
  loop?: boolean
  onLoopChange?: (loop: boolean) => void
  muted?: boolean
  onMutedChange?: (muted: boolean) => void
  rounded?: boolean
  onRoundedChange?: (rounded: boolean) => void
}): ComponentConfigSchema {
  const sections: ConfigSection[] = []

  if (config.onContentChange) {
    sections.push({
      type: 'text',
      props: {
        content: config.content,
        onChange: config.onContentChange,
        label: 'Content',
        rows: 6,
      },
    })
  }

  if (config.onSrcChange) {
    sections.push({
      type: 'custom',
      render: () => (
        <div className="space-y-2 pt-4 border-t">
          <label className="text-sm font-medium">URL</label>
          <input
            type="text"
            value={config.src ?? ''}
            onChange={(e) => config.onSrcChange?.(e.target.value)}
            placeholder="https://example.com/file.png"
            className="w-full h-10 px-3 rounded-md border border-input bg-background"
          />
        </div>
      ),
    })
  }

  if (config.onAltChange) {
    sections.push({
      type: 'custom',
      render: () => {
        const { t } = useTranslation('dashboardComponents')
        return (
          <div className="space-y-2">
            <label className="text-sm font-medium">{t('codeConfig.altTextDescription', 'Alt Text / Description')}</label>
            <input
              type="text"
              value={config.alt ?? ''}
              onChange={(e) => config.onAltChange?.(e.target.value)}
              placeholder={t('placeholders.description')}
              className="w-full h-10 px-3 rounded-md border border-input bg-background"
            />
          </div>
        )
      },
    })
  }

  if (config.onLanguageChange) {
    sections.push({
      type: 'select',
      props: {
        value: config.language ?? 'typescript',
        options: [
          { value: 'javascript', label: 'JavaScript' },
          { value: 'typescript', label: 'TypeScript' },
          { value: 'python', label: 'Python' },
          { value: 'rust', label: 'Rust' },
          { value: 'go', label: 'Go' },
          { value: 'java', label: 'Java' },
          { value: 'cpp', label: 'C++' },
          { value: 'csharp', label: 'C#' },
          { value: 'html', label: 'HTML' },
          { value: 'css', label: 'CSS' },
          { value: 'json', label: 'JSON' },
          { value: 'yaml', label: 'YAML' },
          { value: 'bash', label: 'Bash' },
          { value: 'sql', label: 'SQL' },
        ],
        onChange: config.onLanguageChange,
        label: 'Language',
      },
    })
  }

  if (config.onVariantChange || config.onAlignChange) {
    sections.push({
      type: 'custom',
      render: () => (
        <div className="grid grid-cols-2 gap-2">
          {config.onVariantChange && (
            <div className="space-y-2">
              <label className="text-sm font-medium">Variant</label>
              <select
                value={config.variant ?? 'body'}
                onChange={(e) => config.onVariantChange?.(e.target.value)}
                className="w-full h-10 px-3 rounded-md border border-input bg-background"
              >
                <option value="h1">Heading 1</option>
                <option value="h2">Heading 2</option>
                <option value="h3">Heading 3</option>
                <option value="body">Body</option>
                <option value="small">Small</option>
                <option value="muted">Muted</option>
              </select>
            </div>
          )}
          {config.onAlignChange && (
            <div className="space-y-2">
              <label className="text-sm font-medium">Alignment</label>
              <select
                value={config.align ?? 'left'}
                onChange={(e) => config.onAlignChange?.(e.target.value)}
                className="w-full h-10 px-3 rounded-md border border-input bg-background"
              >
                <option value="left">Left</option>
                <option value="center">Center</option>
                <option value="right">Right</option>
                <option value="justify">Justify</option>
              </select>
            </div>
          )}
        </div>
      ),
    })
  }

  if (config.onColorChange) {
    sections.push({
      type: 'color',
      props: { color: config.color, onChange: config.onColorChange },
    })
  }

  // Boolean options
  const boolOptions: BooleanOption[] = []
  if (config.onShowLineNumbersChange) {
    boolOptions.push({ key: 'showLineNumbers', label: 'Show Line Numbers', value: config.showLineNumbers })
  }
  if (config.onAutoplayChange) {
    boolOptions.push({ key: 'autoplay', label: 'Autoplay', value: config.autoplay })
  }
  if (config.onLoopChange) {
    boolOptions.push({ key: 'loop', label: 'Loop', value: config.loop })
  }
  if (config.onMutedChange) {
    boolOptions.push({ key: 'muted', label: 'Muted', value: config.muted })
  }
  if (config.onRoundedChange) {
    boolOptions.push({ key: 'rounded', label: 'Rounded Corners', value: config.rounded })
  }

  if (boolOptions.length > 0) {
    sections.push({
      type: 'boolean',
      props: {
        options: boolOptions,
        onChange: (key, value) => {
          if (key === 'showLineNumbers') config.onShowLineNumbersChange?.(value)
          if (key === 'autoplay') config.onAutoplayChange?.(value)
          if (key === 'loop') config.onLoopChange?.(value)
          if (key === 'muted') config.onMutedChange?.(value)
          if (key === 'rounded') config.onRoundedChange?.(value)
        },
      },
    })
  }

  return { sections }
}

/**
 * Create a standard schema for chart components
 * (LineChart, BarChart, PieChart, etc.)
 */
export function createChartConfig(config: {
  dataSource?: DataSourceOrList
  onDataSourceChange: (ds: DataSourceOrList | DataSource | undefined) => void
  label?: string
  onLabelChange?: (label: string) => void
  height?: number
  onHeightChange?: (height: number) => void
  size?: SizeOption
  onSizeChange?: (size: SizeOption) => void
  showPoints?: boolean
  onShowPointsChange?: (show: boolean) => void
  stacked?: boolean
  onStackedChange?: (stacked: boolean) => void
  showPercentage?: boolean
  onShowPercentageChange?: (show: boolean) => void
  showLabels?: boolean
  onShowLabelsChange?: (show: boolean) => void
  // Telemetry options for historical data
  // Note: timeRange is deprecated - use timeWindow in DataSource instead
  limit?: number
  onLimitChange?: (limit: number) => void
  // Multiple data source support
  multiple?: boolean
  maxSources?: number
}): ComponentConfigSchema {
  const sections: ConfigSection[] = []

  sections.push({
    type: 'data-source',
    props: {
      dataSource: config.dataSource,
      onChange: config.onDataSourceChange,
      allowedTypes: ['device-metric', 'system'],  // Charts support device metrics and system metrics
      multiple: config.multiple ?? true,
      maxSources: config.maxSources ?? 10
    },
  })

  // Data points limit option (shown when configured)
  // Note: Time range is now configured via DataTransformConfig component with timeWindow enum
  if (config.onLimitChange) {
    sections.push({
      type: 'custom',
      render: () => (
        <div className="space-y-2 pt-4 border-t">
          <label className="text-sm font-medium">Data Points</label>
          <input
            type="number"
            value={config.limit ?? 50}
            onChange={(e) => config.onLimitChange?.(Number(e.target.value))}
            min={1}
            max={500}
            className="w-full h-10 px-3 rounded-md border border-input bg-background text-sm"
          />
          <p className="text-xs text-muted-foreground">Max data points to fetch (for raw data)</p>
        </div>
      ),
    })
  }

  // Label input
  if (config.onLabelChange) {
    sections.push({
      type: 'custom',
      render: () => {
        const { t } = useTranslation('dashboardComponents')
        return (
          <div className="space-y-2 pt-4 border-t">
            <label className="text-sm font-medium">{t('common.label')}</label>
            <input
              type="text"
              value={config.label ?? ''}
              onChange={(e) => config.onLabelChange?.(e.target.value)}
              placeholder={t('placeholders.temperatureTrend')}
              className="w-full h-10 px-3 rounded-md border border-input bg-background"
            />
          </div>
        )
      },
    })
  }

  if (config.onHeightChange) {
    sections.push({
      type: 'custom',
      render: () => (
        <div className="space-y-2 pt-4 border-t">
          <label className="text-sm font-medium">Chart Height</label>
          <input
            type="number"
            min={100}
            max={800}
            step={50}
            value={config.height ?? 200}
            onChange={(e) => config.onHeightChange?.(parseInt(e.target.value) || 200)}
            className="w-full h-10 px-3 rounded-md border border-input bg-background"
          />
        </div>
      ),
    })
  }

  if (config.onSizeChange) {
    sections.push({ type: 'size', props: { size: config.size, onChange: config.onSizeChange } })
  }

  const boolOptions: BooleanOption[] = []
  if (config.onShowPointsChange) {
    boolOptions.push({ key: 'showPoints', label: 'Show Points', value: config.showPoints })
  }
  if (config.onStackedChange) {
    boolOptions.push({ key: 'stacked', label: 'Stacked', value: config.stacked })
  }
  if (config.onShowPercentageChange) {
    boolOptions.push({ key: 'showPercentage', label: 'Show Percentage', value: config.showPercentage })
  }
  if (config.onShowLabelsChange) {
    boolOptions.push({ key: 'showLabels', label: 'Show Labels', value: config.showLabels })
  }

  if (boolOptions.length > 0) {
    sections.push({
      type: 'boolean',
      props: {
        options: boolOptions,
        onChange: (key, value) => {
          if (key === 'showPoints') config.onShowPointsChange?.(value)
          if (key === 'stacked') config.onStackedChange?.(value)
          if (key === 'showPercentage') config.onShowPercentageChange?.(value)
          if (key === 'showLabels') config.onShowLabelsChange?.(value)
        },
      },
    })
  }

  return { sections }
}
