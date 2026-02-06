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
  DiscoveredDevice,
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
}

// ============================================================================
// Device State
// ============================================================================

export interface DeviceState {
  devices: Device[]
  deviceTypes: DeviceType[]
  selectedDevice: Device | null
  selectedDeviceId: string | null
  deviceDetails: Device | null
  deviceTypeDetails: DeviceType | null
  discovering: boolean
  discoveredDevices: DiscoveredDevice[]
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

export interface SettingsState {
  settingsDialogOpen: boolean
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

/**
 * Get page title with internationalization support.
 * This function should be used within React components where useTranslation is available.
 *
 * Usage:
 *   const { t } = useTranslation('nav')
 *   const title = getPageTitle('dashboard', t)
 */
export function getPageTitle(page: PageName, t: (key: string) => string): string {
  const titles: Record<PageName, string> = {
    dashboard: t('nav.dashboard'),
    'visual-dashboard': t('nav.visual-dashboard'),
    devices: t('nav.devices'),
    alerts: t('nav.alerts'),
    automation: t('nav.automation'),
    commands: t('nav.commands'),
    decisions: t('nav.decisions'),
    plugins: t('nav.plugins'),
    settings: t('nav.settings'),
    events: t('nav.events'),
    agents: t('nav.agents'),
  }
  return titles[page] || page
}

