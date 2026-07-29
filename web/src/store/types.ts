/**
 * Store state type definitions
 *
 * This file contains all the state interfaces for the Zustand store slices.
 */

import type {
  Device,
  DeviceType,
  Alert,
  Message,
  TelemetryDataResponse,
  TelemetrySummaryResponse,
  DeviceCurrentStateResponse,
  CommandHistoryResponse,
  ChatSession,
  UserInfo,
} from '@/types'

// ============================================================================
// Auth State
// ============================================================================

export interface AuthState {
  apiKey: string | null
  isAuthenticated: boolean
  // User authentication (JWT)
  user: UserInfo | null
  token: string | null
}

// ============================================================================
// Session State
// ============================================================================

export interface SessionState {
  sessionId: string | null
  messages: Message[]
  sessions: ChatSession[]
  // Pagination state for sessions
  sessionsPage: number
  sessionsHasMore: boolean
  sessionsLoading: boolean
  // Loading state for switching sessions
  isLoadingSession: boolean
}

// ============================================================================
// Device State
// ============================================================================

export interface DeviceState {
  devices: Device[]
  /** Real-time telemetry split from devices array to avoid cascading re-renders.
   *  Keyed by device ID, value is a flat map of metric→value. */
  deviceTelemetry: Record<string, Record<string, unknown>>
  deviceTypes: DeviceType[]
  selectedDevice: Device | null
  selectedDeviceId: string | null
  deviceDetails: Device | null
  deviceTypeDetails: DeviceType | null
  devicesLoading: boolean
  deviceTypesLoading: boolean

  // Dialog states
  addDeviceDialogOpen: boolean
  addDeviceTypeDialogOpen: boolean
  deviceDetailsDialogOpen: boolean
}

// ============================================================================
// Telemetry State
// ============================================================================

export interface TelemetryState {
  telemetryData: TelemetryDataResponse | null
  telemetrySummary: TelemetrySummaryResponse | null
  deviceCurrentState: DeviceCurrentStateResponse | null  // Unified device + metrics data
  commandHistory: CommandHistoryResponse | null
  telemetryLoading: boolean
}

// ============================================================================
// Alert State
// ============================================================================

export interface AlertState {
  alerts: Alert[]
  alertsLoading: boolean
}

// ============================================================================
// Settings State
// ============================================================================
// LLM/MQTT/Device settings are now managed via the Plugin system.
// Only general system settings remain here.

export type SettingsSection = "llm" | "connections" | "preferences" | "about"

export interface SettingsState {
  settingsDialogOpen: boolean
  settingsSection: SettingsSection
}

// ============================================================================
// UI State
// ============================================================================

export type PageName =
  | 'dashboard'
  | 'visual-dashboard'
  | 'devices'
  | 'alerts'
  | 'automation'
  | 'commands'
  | 'decisions'
  | 'plugins'
  | 'settings'
  | 'events'
  | 'agents'

export interface UIState {
  sidebarOpen: boolean
  wsConnected: boolean
}

// ============================================================================
// WebSocket State
// ============================================================================

export interface WebSocketState {
  wsConnected: boolean
}

// ============================================================================
// Combined Root State
// ============================================================================

export interface RootState
  extends AuthState,
    SessionState,
    DeviceState,
    TelemetryState,
    AlertState,
    SettingsState,
    UIState {}

// ============================================================================
// Page Titles Mapping
// ============================================================================

