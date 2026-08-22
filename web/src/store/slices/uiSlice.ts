/**
 * UI Slice
 *
 * WebSocket connection state + per-domain data version counters.
 * (The app sidebar is fixed-width — no state.)
 */

import type { StateCreator } from 'zustand'
import type { UIState } from '../types'

export interface UISlice extends UIState {
  // Actions
  setWsConnected: (connected: boolean) => void
  /** Bump a domain's data version (triggers version-gated page refetches). */
  bumpDataVersion: (domain: string) => void
  /** Open the global chat side panel (counter-based request — see UIState). */
  openChatPanel: () => void
}

export const createUISlice: StateCreator<
  UISlice,
  [],
  [],
  UISlice
> = (set) => ({
  wsConnected: false,
  dataVersions: {},
  chatPanelRequest: 0,

  setWsConnected: (connected: boolean) => {
    set({ wsConnected: connected })
  },

  bumpDataVersion: (domain: string) => {
    set((state) => ({
      dataVersions: { ...state.dataVersions, [domain]: (state.dataVersions[domain] ?? 0) + 1 },
    }))
  },

  openChatPanel: () => {
    set((state) => ({ chatPanelRequest: state.chatPanelRequest + 1 }))
  },
})
