/**
 * Data Push Slice
 *
 * Manages push target state and operations.
 */

import type { StateCreator } from 'zustand'
import type { PushTarget, DeliveryLog, PushStats, CreatePushTargetRequest, UpdatePushTargetRequest } from '@/types'
import { api } from '@/lib/api'
import { logError } from '@/lib/errors'

export interface DataPushSlice {
  // State
  pushTargets: PushTarget[]
  pushTargetsLoading: boolean
  pushStats: PushStats | null
  pushStatsLoading: boolean
  pushTargetDialogOpen: boolean
  editingPushTarget: PushTarget | null
  deliveryLogs: DeliveryLog[]
  deliveryLogsTotal: number
  deliveryLogsLoading: boolean
  deliveryLogsTargetId: string | null

  // Actions
  fetchPushTargets: () => Promise<void>
  createPushTarget: (data: CreatePushTargetRequest) => Promise<boolean>
  updatePushTarget: (id: string, data: UpdatePushTargetRequest) => Promise<boolean>
  deletePushTarget: (id: string) => Promise<boolean>
  startPushTarget: (id: string) => Promise<boolean>
  stopPushTarget: (id: string) => Promise<boolean>
  testPushTarget: (id: string) => Promise<DeliveryLog | null>
  fetchPushStats: () => Promise<void>
  fetchDeliveryLogs: (targetId: string, limit?: number, offset?: number) => Promise<void>
  setPushTargetDialogOpen: (open: boolean) => void
  setEditingPushTarget: (target: PushTarget | null) => void
}

export const createDataPushSlice: StateCreator<
  DataPushSlice,
  [],
  [],
  DataPushSlice
> = (set, get) => ({
  // Initial state
  pushTargets: [],
  pushTargetsLoading: false,
  pushStats: null,
  pushStatsLoading: false,
  pushTargetDialogOpen: false,
  editingPushTarget: null,
  deliveryLogs: [],
  deliveryLogsTotal: 0,
  deliveryLogsLoading: false,
  deliveryLogsTargetId: null,

  // Actions
  fetchPushTargets: async () => {
    if (get().pushTargetsLoading) return
    set({ pushTargetsLoading: true })
    try {
      const response = await api.listPushTargets()
      set({ pushTargets: response.targets || [] })
    } catch (error) {
      logError(error, { operation: 'Fetch push targets' })
      set({ pushTargets: [] })
    } finally {
      set({ pushTargetsLoading: false })
    }
  },

  createPushTarget: async (data: CreatePushTargetRequest) => {
    try {
      await api.createPushTarget(data)
      await get().fetchPushTargets()
      return true
    } catch (error) {
      logError(error, { operation: 'Create push target' })
      return false
    }
  },

  updatePushTarget: async (id: string, data: UpdatePushTargetRequest) => {
    try {
      await api.updatePushTarget(id, data)
      await get().fetchPushTargets()
      return true
    } catch (error) {
      logError(error, { operation: 'Update push target' })
      return false
    }
  },

  deletePushTarget: async (id: string) => {
    try {
      await api.deletePushTarget(id)
      await get().fetchPushTargets()
      return true
    } catch (error) {
      logError(error, { operation: 'Delete push target' })
      return false
    }
  },

  startPushTarget: async (id: string) => {
    try {
      await api.startPushTarget(id)
      await get().fetchPushTargets()
      return true
    } catch (error) {
      logError(error, { operation: 'Start push target' })
      return false
    }
  },

  stopPushTarget: async (id: string) => {
    try {
      await api.stopPushTarget(id)
      await get().fetchPushTargets()
      return true
    } catch (error) {
      logError(error, { operation: 'Stop push target' })
      return false
    }
  },

  testPushTarget: async (id: string) => {
    try {
      const result = await api.testPushTarget(id)
      return result
    } catch (error) {
      logError(error, { operation: 'Test push target' })
      return null
    }
  },

  fetchPushStats: async () => {
    set({ pushStatsLoading: true })
    try {
      const stats = await api.getPushStats()
      set({ pushStats: stats })
    } catch (error) {
      logError(error, { operation: 'Fetch push stats' })
    } finally {
      set({ pushStatsLoading: false })
    }
  },

  fetchDeliveryLogs: async (targetId: string, limit?: number, offset?: number) => {
    set({ deliveryLogsLoading: true, deliveryLogsTargetId: targetId })
    try {
      const response = await api.listPushDeliveryLogs(targetId, limit, offset)
      set({ deliveryLogs: response.logs || [], deliveryLogsTotal: response.total || 0 })
    } catch (error) {
      logError(error, { operation: 'Fetch delivery logs' })
      set({ deliveryLogs: [], deliveryLogsTotal: 0 })
    } finally {
      set({ deliveryLogsLoading: false })
    }
  },

  setPushTargetDialogOpen: (open: boolean) => {
    set({ pushTargetDialogOpen: open })
    if (!open) {
      set({ editingPushTarget: null })
    }
  },

  setEditingPushTarget: (target: PushTarget | null) => {
    set({ editingPushTarget: target, pushTargetDialogOpen: target !== null })
  },
})
