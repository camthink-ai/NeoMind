/**
 * Dashboard Config Components
 *
 * Unified configuration system for dashboard components.
 * Provides abstracted, reusable configuration sections.
 */

export { ConfigSection } from './ConfigSection'
export { DataSourceConfigSection } from './DataSourceConfigSection'
export {
  ValueConfig,
  RangeConfig,
  SizeConfig,
  ColorConfig,
  MultiColorConfig,
  LabelConfig,
  BooleanConfig,
  SelectConfig,
  TextContentConfig,
  OrientationConfig,
  AnimationConfig,
  type SizeOption,
  type BooleanOption,
  type SelectOption,
  type Orientation,
} from './UIConfigSections'
export { ConfigRenderer } from './ConfigRenderer'
export { ComponentConfigDialog } from './ComponentConfigDialog'
export type { ComponentConfigDialogProps } from './ComponentConfigDialog'
export { DataSourceSelectorContent } from './DataSourceSelectorContent'
export type { DataSourceSelectorContentProps } from './DataSourceSelectorContent'
export { DataSourceSelector } from './DataSourceSelector'
export type { DataSourceSelectorProps } from './DataSourceSelector'
export {
  registerComponentConfig,
  getComponentConfig,
  createDataDisplayConfig,
  createProgressConfig,
  createControlConfig,
  createIndicatorConfig,
  createContentConfig,
  createChartConfig,
  type ComponentConfigSchema,
  type DataSourceSection,
  type ValueSection,
  type RangeSection,
  type SizeSection,
  type ColorSection,
  type MultiColorSection,
  type LabelSection,
  type BooleanSection,
  type SelectSection,
  type TextSection,
  type OrientationSection,
  type AnimationSection,
  type CustomSection,
} from './ComponentConfigBuilder'
