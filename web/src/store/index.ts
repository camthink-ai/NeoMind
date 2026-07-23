/**
 * Zustand Store
 *
 * This is the main store entry point that combines all slices.
 * It uses Zustand's slice pattern for better code organization.
 */

import { createWithEqualityFn as create } from 'zustand/traditional'
import { devtools, persist, createJSONStorage, type StateStorage } from 'zustand/middleware'
import { onUnauthorized, tokenManager, getApiKey } from '@/lib/api'

// ============================================================================
// Storage Configuration
// ============================================================================

// Storage strategy: Do NOT persist messages to LocalStorage
// Messages are already persisted in the backend database.
// This prevents QuotaExceededError during long conversations.
/**
 * Custom storage that handles QuotaExceededError gracefully.
 * When storage is full, it clears old data and retries.
 */
const baseStorage: StateStorage = {
  getItem: (name: string): string | null => {
    return localStorage.getItem(name)
  },
  setItem: (name: string, value: string): void => {
    try {
      localStorage.setItem(name, value)
    } catch (error) {
      if (error instanceof DOMException && error.name === 'QuotaExceededError') {
        console.warn('[Storage] LocalStorage quota exceeded, clearing old data...')
        // Clear the store and try again
        localStorage.removeItem('neomind-store')
        try {
          localStorage.setItem(name, value)
        } catch (retryError) {
          console.error('[Storage] Failed to save even after clearing:', retryError)
        }
      } else {
        throw error
      }
    }
  },
  removeItem: (name: string): void => {
    localStorage.removeItem(name)
  },
}

// Create JSON storage wrapper for Zustand
const safeStorage = createJSONStorage(() => baseStorage)

// Import all slices
import { createAuthSlice } from './slices/authSlice'
import { createSessionSlice } from './slices/sessionSlice'
import { createUISlice } from './slices/uiSlice'
import { createAlertSlice } from './slices/alertSlice'
import { createDeviceSlice } from './slices/deviceSlice'
import { createSettingsSlice } from './slices/settingsSlice'
import { createExtensionSlice } from './slices/extensionSlice'
import { createLlmBackendSlice } from './slices/llmBackendSlice'
import { createDashboardCrudSlice } from './slices/dashboardCrudSlice'
import { createDashboardLayoutSlice } from './slices/dashboardLayoutSlice'
import { createDashboardConfigSlice } from './slices/dashboardConfigSlice'
import { createDashboardUISlice } from './slices/dashboardUISlice'
import { createUpdateSlice } from './slices/updateSlice'
import { createAiAnalystSlice } from './slices/aiAnalystSlice'
import { createInstanceSlice } from './slices/instanceSlice'
import { createFrontendComponentSlice } from './slices/frontendComponentSlice'
import { createDataPushSlice } from './slices/dataPushSlice'

// Import types
import type { AuthSlice } from './slices/authSlice'
import type { SessionSlice } from './slices/sessionSlice'
import type { UISlice } from './slices/uiSlice'
import type { AlertSlice } from './slices/alertSlice'
import type { DeviceSlice } from './slices/deviceSlice'
import type { SettingsSlice } from './slices/settingsSlice'
import type { ExtensionSlice } from './slices/extensionSlice'
import type { LlmBackendSlice } from './slices/llmBackendSlice'
import type { DashboardCrudSlice } from './slices/dashboardCrudSlice'
import type { DashboardLayoutSlice } from './slices/dashboardLayoutSlice'
import type { DashboardConfigSlice } from './slices/dashboardConfigSlice'
import type { DashboardUISlice } from './slices/dashboardUISlice'
import type { UpdateSlice } from './slices/updateSlice'
import type { AiAnalystSlice } from './slices/aiAnalystSlice'
import type { InstanceSlice } from './slices/instanceSlice'
import type { FrontendComponentSlice } from './slices/frontendComponentSlice'
import type { DataPushSlice } from './slices/dataPushSlice'

// ============================================================================
// Combined Store Type
// ============================================================================

export type NeoMindStore = AuthSlice
  & SessionSlice
  & UISlice
  & AlertSlice
  & DeviceSlice
  & SettingsSlice
  & ExtensionSlice
  & LlmBackendSlice
  & DashboardCrudSlice
  & DashboardLayoutSlice
  & DashboardConfigSlice
  & DashboardUISlice
  & UpdateSlice
  & AiAnalystSlice
  & InstanceSlice
  & FrontendComponentSlice
  & DataPushSlice

// ============================================================================
// Create Store
// ============================================================================

export const useStore = create<NeoMindStore>()(
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
        ...createDashboardCrudSlice(set, get, api),
        ...createDashboardLayoutSlice(set, get, api),
        ...createDashboardConfigSlice(set, get, api),
        ...createDashboardUISlice(set, get, api),
        ...createUpdateSlice(set, get, api),
        ...createAiAnalystSlice(set, get, api),
        ...createInstanceSlice(set, get, api),
        ...createFrontendComponentSlice(set, get, api),
        ...createDataPushSlice(set, get, api),
      }),
      {
        name: 'neomind-store',
        version: 3,
        storage: safeStorage,
        partialize: (state) => ({
          // NOTE: Do NOT persist messages to LocalStorage!
          // Messages are already persisted in the backend database.
          // This prevents QuotaExceededError during long conversations.
          // When the user switches sessions, messages are loaded from the backend.
          sessionId: state.sessionId,
          // Only persist essential UI state
          sidebarOpen: state.sidebarOpen,
          // NOTE: updateInfo / updateDialogOpen are intentionally NOT persisted.
          // They are transient per-check UI state. Persisting updateDialogOpen
          // meant a dialog left open (true) in the old version was read back
          // after an OTA update, so "New Version Available" re-appeared on the
          // freshly installed version. Restart-time re-check is driven by the
          // autoCheck in useUpdateCheck, not by persisted dialog state.
          // NOTE: currentInstanceId is NOT persisted here — it's managed
          // separately via localStorage('currentInstanceId') in instanceSlice
          // so that applyPendingSwitch() and the initial state stay in sync.
        }),
        migrate: (persisted, version) => {
          const old = persisted as Record<string, unknown>
          if (version < 2) {
            // v0/v1→v2: remove stale currentInstanceId that overrides
            // the one managed by instanceSlice's own localStorage key.
            // This fixes instances where the old persisted data caused
            // isRemoteInstance() to return true, skipping API calls.
            delete old.currentInstanceId
          }
          if (version < 3) {
            // v2→v3: drop transient update UI state (see partialize note).
            // Clears updateDialogOpen=true left over from earlier builds so
            // the update dialog does not re-appear after an OTA install.
            delete old.updateDialogOpen
            delete old.updateInfo
          }
          return old
        },
      }
    ),
    // Performance optimization: Disable Redux DevTools in production
    // Reduces state update overhead by ~15%
    { enabled: import.meta.env.DEV }
  )
)

// Re-export the store as useAppStore for convenience
export const useAppStore = useStore

// Performance optimization: Only expose store globally in development
if (typeof window !== 'undefined' && import.meta.env.DEV) {
  (window as any).neomindStore = useStore
}

// ============================================================================
// Re-export page titles
// ============================================================================

// ============================================================================
// Global 401 Handler Registration
// ============================================================================

// Register a global callback for all 401 errors
// When using API key auth, 401 on admin/JWT-only endpoints should NOT
// clear the entire session — the API key is still valid for all other routes.
// Only clear auth state when using JWT (token expired/invalid).
onUnauthorized(() => {
  const apiKey = getApiKey()
  if (apiKey) {
    // API key auth — don't clear session. A 401 likely means the endpoint
    // requires JWT (admin routes). The API key is still valid for other routes.
    return
  }
  // JWT auth — token is invalid, clear everything and redirect to login
  tokenManager.clearToken()
  useStore.setState({ token: null, user: null, isAuthenticated: false })
})
