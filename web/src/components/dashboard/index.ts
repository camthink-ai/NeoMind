/**
 * Dashboard Components Index
 *
 * Exports all dashboard-related components.
 */

// Registry (New - centralized component metadata and rendering)
export * from './registry'

// Wrapper
export { DashboardComponentWrapper } from './DashboardComponentWrapper'
export type { DashboardComponentWrapperProps } from './DashboardComponentWrapper'

// Layout
export { DashboardGrid } from './DashboardGrid'
export { DashboardListSidebar } from './DashboardListSidebar'
export type { DashboardListSidebarProps } from './DashboardListSidebar'

// Mobile components
export { MobileEditBar, CompactMobileEditBar, FloatingEditButton } from './MobileEditBar'

// Generic components - Indicators
export { ValueCard } from './generic/ValueCard'
export { LEDIndicator, type LEDState } from './generic/LEDIndicator'
export { Sparkline } from './generic/Sparkline'
export { ProgressBar } from './generic/ProgressBar'

// Agent-related components
export { AgentStatusCard } from './generic/AgentStatusCard'
export { AgentMonitorWidget } from './generic/AgentMonitorWidget'

// Generic components - Charts
export { LineChart, AreaChart } from './generic/LineChart'
export { BarChart } from './generic/BarChart'
export { PieChart } from './generic/PieChart'

// Generic components - Controls
export { ToggleSwitch } from './generic/ToggleSwitch'

// Generic components - Display & Content
export { ImageDisplay } from './generic/ImageDisplay'
export { ImageHistory, type ImageHistoryProps, type ImageHistoryItem } from './generic/ImageHistory'
export { WebDisplay } from './generic/WebDisplay'
export { MarkdownDisplay } from './generic/MarkdownDisplay'

// Generic components - Spatial & Media
export { MapDisplay, type MapDisplayProps, type MapMarker } from './generic/MapDisplay'
export { VideoDisplay, type VideoDisplayProps, type VideoSourceType } from './generic/VideoDisplay'
export { CustomLayer, type CustomLayerProps, type LayerItem, LayerEditorDialog, type LayerBinding, type LayerBindingType } from './generic/CustomLayer'
export { MapEditorDialog, type MapBinding, type MapBindingType } from './generic/MapEditorDialog'
export { CenterPickerDialog } from './generic/CenterPickerDialog'

// Config system
export * from './config'
