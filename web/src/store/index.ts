/**
 * Zustand Store
 *
 * This is the main store entry point that combines all slices.
 * It uses Zustand's slice pattern for better code organization.
 */

import { create } from 'zustand'
import { devtools, persist } from 'zustand/middleware'
import { onUnauthorized, tokenManager } from '@/lib/api'

// Import all slices
import { createAuthSlice } from './slices/authSlice'
import { createSessionSlice } from './slices/sessionSlice'
import { createUISlice } from './slices/uiSlice'
import { createAlertSlice } from './slices/alertSlice'
import { createDeviceSlice } from './slices/deviceSlice'
import { createSettingsSlice } from './slices/settingsSlice'
import { createExtensionSlice } from './slices/extensionSlice'
import { createLlmBackendSlice } from './slices/llmBackendSlice'
import { createDashboardSlice } from './slices/dashboardSlice'

// Import types
import type { AuthSlice } from './slices/authSlice'
import type { SessionSlice } from './slices/sessionSlice'
import type { UISlice } from './slices/uiSlice'
import type { AlertSlice } from './slices/alertSlice'
import type { DeviceSlice } from './slices/deviceSlice'
import type { SettingsSlice } from './slices/settingsSlice'
import type { ExtensionSlice } from './slices/extensionSlice'
import type { LlmBackendSlice } from './slices/llmBackendSlice'
import type { DashboardState } from './slices/dashboardSlice'

// ============================================================================
// Combined Store Type
// ============================================================================

export type NeoTalkStore = AuthSlice
  & SessionSlice
  & UISlice
  & AlertSlice
  & DeviceSlice
  & SettingsSlice
  & ExtensionSlice
  & LlmBackendSlice
  & DashboardState

// ============================================================================
// Create Store
// ============================================================================

export const useStore = create<NeoTalkStore>()(
  devtools(
    persist(
      (set, get, api) => ({
        // Combine all slices
        ...createAuthSlice(set, get, api),
        ...createSessionSlice(set, get, api),
        ...createUISlice(set, get, api),
        ...createAlertSlice(set, get, api),
        ...createDeviceSlice(set, get, api),
        ...createSettingsSlice(set, get, api),
        ...createExtensionSlice(set, get, api),
        ...createLlmBackendSlice(set, get, api),
        ...createDashboardSlice(set, get, api),
      }),
      {
        name: 'neomind-store',
        partialize: (state) => ({
          messages: state.messages,
          sessionId: state.sessionId,
        }),
        onRehydrateStorage: () => (state) => {
          console.log('[Store] Rehydrating store, devices:', state?.devices?.length)
        },
      }
    ),
    // Performance optimization: Disable Redux DevTools in production
    // Reduces state update overhead by ~15%
    { enabled: import.meta.env.DEV }
  )
)

// Performance optimization: Only expose store globally in development
if (typeof window !== 'undefined' && import.meta.env.DEV) {
  (window as any).neomindStore = useStore
}

// ============================================================================
// Re-export page titles
// ============================================================================

export { pageTitles, getPageTitle } from './types'

// ============================================================================
// Global 401 Handler Registration
// ============================================================================

// Register a global callback for all 401 errors
// This clears the invalid token and shows the login screen
onUnauthorized(() => {
  // Clear the invalid token
  tokenManager.clearToken()
  // Update store state
  useStore.setState({ token: null, user: null, isAuthenticated: false })
})
