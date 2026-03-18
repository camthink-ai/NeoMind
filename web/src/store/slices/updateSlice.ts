/**
 * Update Slice
 *
 * Manages application update state including available updates,
 * download progress, and installation status.
 */

import { StateCreator } from 'zustand'

export interface UpdateInfo {
  available: boolean
  version?: string
  body?: string
  date?: string
}

export interface UpdateProgress {
  total: number
  current: number
  progress: number
}

export type UpdateStatus = 'idle' | 'checking' | 'available' | 'downloading' | 'installing' | 'up-to-date' | 'error'

export interface UpdateState {
  // State
  updateStatus: UpdateStatus
  updateInfo: UpdateInfo | null
  downloadProgress: UpdateProgress | null
  lastCheckTime: number | null
  error: string | null
  updateDialogOpen: boolean

  // Actions
  setUpdateStatus: (status: UpdateStatus) => void
  setUpdateInfo: (info: UpdateInfo | null) => void
  setDownloadProgress: (progress: UpdateProgress | null) => void
  setError: (error: string | null) => void
  setLastCheckTime: (time: number) => void
  setUpdateDialogOpen: (open: boolean) => void
  resetUpdate: () => void
}

export const createUpdateSlice: StateCreator<
  UpdateSlice,
  [],
  [],
  UpdateSlice
> = (set) => ({
  // Initial state
  updateStatus: 'idle',
  updateInfo: null,
  downloadProgress: null,
  lastCheckTime: null,
  error: null,
  updateDialogOpen: false,

  // Actions
  setUpdateStatus: (status) =>
    set({ updateStatus: status, error: null }),

  setUpdateInfo: (info) =>
    set({ updateInfo: info }),

  setDownloadProgress: (progress) =>
    set({ downloadProgress: progress }),

  setError: (error) =>
    set({ error, updateStatus: 'error' }),

  setLastCheckTime: (time) =>
    set({ lastCheckTime: time }),

  setUpdateDialogOpen: (open) =>
    set({ updateDialogOpen: open }),

  resetUpdate: () =>
    set({
      updateStatus: 'idle',
      updateInfo: null,
      downloadProgress: null,
      error: null,
    }),
})

// Type for the full store with update slice
export interface UpdateSlice extends UpdateState {
  setUpdateStatus: (status: UpdateStatus) => void
  setUpdateInfo: (info: UpdateInfo | null) => void
  setDownloadProgress: (progress: UpdateProgress | null) => void
  setError: (error: string | null) => void
  setLastCheckTime: (time: number) => void
  setUpdateDialogOpen: (open: boolean) => void
  resetUpdate: () => void
}
