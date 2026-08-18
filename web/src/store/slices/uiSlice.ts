/**
 * UI Slice
 *
 * Handles UI state like the app sidebar collapse and WebSocket connection.
 */

import type { StateCreator } from 'zustand'
import type { UIState } from '../types'

export interface UISlice extends UIState {
  // Actions
  toggleAppSidebar: () => void
  setAppSidebarCollapsed: (collapsed: boolean) => void
  setWsConnected: (connected: boolean) => void
}

export const createUISlice: StateCreator<
  UISlice,
  [],
  [],
  UISlice
> = (set) => ({
  // Initial state
  appSidebarCollapsed: false,
  wsConnected: false,

  // Actions
  toggleAppSidebar: () => {
    set((state) => ({ appSidebarCollapsed: !state.appSidebarCollapsed }))
  },

  setAppSidebarCollapsed: (collapsed: boolean) => {
    set({ appSidebarCollapsed: collapsed })
  },

  setWsConnected: (connected: boolean) => {
    set({ wsConnected: connected })
  },
})
