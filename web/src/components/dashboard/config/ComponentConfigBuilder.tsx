import type React from 'react'
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
  displaySections?: ConfigSection[]  // Display-specific configuration (labels, legends, axes, etc.)
  advancedSections?: ConfigSection[] // Advanced/component-specific configuration
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
    allowedTypes?: Array<'device-metric' | 'device-command' | 'device-info' | 'device' | 'metric' | 'command' | 'system' | 'extension' | 'extension-command' | 'transform' | 'ai'>
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
