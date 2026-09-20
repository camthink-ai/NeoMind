// Shared types for the agent editor — split from AgentEditorFullScreen.tsx

export interface MetricInfo {
  name: string
  display_name: string
  unit?: string
  data_type?: string
  source: 'device' | 'extension'
  extensionId?: string
}

export interface CommandInfo {
  name: string
  display_name: string
  description?: string
  source: 'device' | 'extension'
  extensionId?: string
  parameters?: Record<string, unknown>
}

export interface DataCollectionConfig {
  time_range_minutes: number
  include_history: boolean
  include_trend: boolean
  include_baseline: boolean
}

export interface SelectedResource {
  id: string
  name: string
  type: 'device' | 'extension'
  deviceType?: string
  // All available metrics/commands
  allMetrics: MetricInfo[]
  allCommands: CommandInfo[]
  // Selected metric/command names
  selectedMetrics: Set<string>
  selectedCommands: Set<string>
  // Data collection config for Focused Mode
  config?: {
    data_collection?: DataCollectionConfig
  }
}

export interface ResourceRecommendation {
  id: string
  name: string
  type: 'device' | 'extension'
  reason: string
  metrics?: MetricInfo[]
  commands?: CommandInfo[]
}

export type ScheduleType = 'timer' | 'reactive' | 'on-demand'
export type TimerSubType = 'interval' | 'daily' | 'weekly'
export interface AvailableResource {
  id: string
  name: string
  type: 'device' | 'extension'
  deviceType?: string
  metrics: MetricInfo[]
  commands: CommandInfo[]
}

