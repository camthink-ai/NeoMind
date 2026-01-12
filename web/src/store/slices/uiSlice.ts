/**
 * UI Slice
 *
 * Handles UI state like navigation, sidebar, and WebSocket connection.
 */

import type { StateCreator } from 'zustand'
import type { UIState } from '../types'

export interface UISlice extends UIState {
  // Actions
  setCurrentPage: (page: UIState['currentPage']) => void
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
  currentPage: 'dashboard',
  sidebarOpen: true,
  wsConnected: false,

  // Actions
  setCurrentPage: (page) => {
    set({ currentPage: page })
  },

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
