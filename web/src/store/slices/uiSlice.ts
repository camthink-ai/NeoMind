/**
 * UI Slice
 *
 * Handles UI state like sidebar and WebSocket connection.
 */

import type { StateCreator } from 'zustand'
import type { UIState } from '../types'

export interface UISlice extends UIState {
  // Actions
  toggleSidebar: () => void
  setSidebarOpen: (open: boolean) => void
  setWsConnected: (connected: boolean) => void
}

export const createUISlice: StateCreator<
  UISlice,
  [],
  [],
  UISlice
> = (set) => ({
  // Initial state
  sidebarOpen: true,
  wsConnected: false,

  // Actions
  toggleSidebar: () => {
    set((state) => ({ sidebarOpen: !state.sidebarOpen }))
  },

  setSidebarOpen: (open: boolean) => {
    set({ sidebarOpen: open })
  },

  setWsConnected: (connected: boolean) => {
    set({ wsConnected: connected })
  },
})
