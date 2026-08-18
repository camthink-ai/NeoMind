/**
 * UI Slice
 *
 * WebSocket connection state. (The app sidebar is fixed-width — no state.)
 */

import type { StateCreator } from 'zustand'
import type { UIState } from '../types'

export interface UISlice extends UIState {
  // Actions
  setWsConnected: (connected: boolean) => void
}

export const createUISlice: StateCreator<
  UISlice,
  [],
  [],
  UISlice
> = (set) => ({
  wsConnected: false,

  setWsConnected: (connected: boolean) => {
    set({ wsConnected: connected })
  },
})
