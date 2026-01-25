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

// Generic components - Indicators
export { ValueCard } from './generic/ValueCard'
export { LEDIndicator, type LEDState } from './generic/LEDIndicator'
export { Sparkline } from './generic/Sparkline'
export { ProgressBar } from './generic/ProgressBar'

// Generic components - Charts
export { LineChart, AreaChart } from './generic/LineChart'
export { BarChart } from './generic/BarChart'
export { PieChart } from './generic/PieChart'

// Generic components - Controls
export { ToggleSwitch } from './generic/ToggleSwitch'
export { ButtonGroup } from './generic/ButtonGroup'
export { Slider } from './generic/Slider'

// Generic components - Tables & Lists
export { DataTable } from './generic/DataTable'
export { LogFeed } from './generic/LogFeed'
export { StatusList } from './generic/StatusList'

// Generic components - Display & Content
export { ImageDisplay } from './generic/ImageDisplay'
export { ImageHistory, type ImageHistoryProps, type ImageHistoryItem } from './generic/ImageHistory'
export { WebDisplay } from './generic/WebDisplay'
export { MarkdownDisplay } from './generic/MarkdownDisplay'

// Business components
export { AgentStatusCard } from './business/AgentStatusCard'
export { DecisionList } from './business/DecisionList'
export { DeviceControl } from './business/DeviceControl'
export { RuleStatusGrid } from './business/RuleStatusGrid'
export { TransformList } from './business/TransformList'

// Config system
export * from './config'
