/**
 * Alert Slice
 *
 * Handles alert state and operations.
 * Note: Now uses the unified messages API internally for backward compatibility.
 */

import type { StateCreator } from 'zustand'
import type { AlertState } from '../types'
import { api } from '@/lib/api'

// Get auth token
const getToken = (): string | null => {
  return localStorage.getItem('neomind_token') || sessionStorage.getItem('neomind_token_session')
}

export interface AlertSlice extends AlertState {
  // Actions
  fetchAlerts: () => Promise<void>
  acknowledgeAlert: (id: string) => Promise<boolean>
  createAlert: (alert: { title: string; message: string; severity?: string; source?: string }) => Promise<boolean>
}

export const createAlertSlice: StateCreator<
  AlertSlice,
  [],
  [],
  AlertSlice
> = (set, get) => ({
  // Initial state
  alerts: [],
  alertsLoading: false,

  // Actions
  fetchAlerts: async () => {
    set({ alertsLoading: true })
    try {
      const token = getToken()
      // Use correct API base for Tauri environment
      const apiBase = (window as any).__TAURI__ ? 'http://localhost:3000/api' : '/api'
      // Use the messages API instead of alerts
      const response = await fetch(`${apiBase}/messages`, {
        headers: {
          'Authorization': token ? `Bearer ${token}` : '',
        },
      })
      const rawData = await response.json()

      // Handle different response formats from messages endpoint
      let messagesArray: any[] = []
      if (Array.isArray(rawData)) {
        messagesArray = rawData
      } else if (rawData?.data?.messages && Array.isArray(rawData.data.messages)) {
        messagesArray = rawData.data.messages
      } else if (rawData?.messages && Array.isArray(rawData.messages)) {
        messagesArray = rawData.messages
      } else if (rawData?.data && Array.isArray(rawData.data)) {
        messagesArray = rawData.data
      }

      // Convert messages to alert format for backward compatibility
      const alertsArray = messagesArray.map((msg: any) => ({
        id: msg.id,
        title: msg.title,
        message: msg.message,
        severity: msg.severity,
        status: msg.status,
        source: msg.source,
        acknowledged: msg.status !== 'active',
        timestamp: msg.timestamp || msg.created_at,
      }))

      // Sort by timestamp descending (newest first)
      alertsArray.sort((a: any, b: any) => {
        const aTime = new Date(a.timestamp).getTime()
        const bTime = new Date(b.timestamp).getTime()
        // If either timestamp is invalid, treat it as oldest
        if (isNaN(aTime)) return 1
        if (isNaN(bTime)) return -1
        return bTime - aTime
      })

      set({ alerts: alertsArray as any })
    } catch (error) {
      console.error('Failed to fetch alerts:', error)
      set({ alerts: [] })
    } finally {
      set({ alertsLoading: false })
    }
  },

  acknowledgeAlert: async (id: string) => {
    try {
      const result = await api.acknowledgeMessage(id)
      if (result.acknowledged) {
        // Update the alert in the list
        set((state) => ({
          alerts: state.alerts.map((alert) =>
            alert.id === id
              ? { ...alert, acknowledged: true, status: 'acknowledged' as const }
              : alert
          ),
        }))
        return true
      }
      return false
    } catch (error) {
      console.error('Failed to acknowledge alert:', error)
      return false
    }
  },

  createAlert: async (alert) => {
    try {
      // Map alert to message format
      await api.createMessage({
        category: 'alert',
        severity: (alert.severity || 'info') as any,
        title: alert.title,
        message: alert.message,
        source: alert.source || 'manual',
      })
      // Refresh the alerts list after creating
      await get().fetchAlerts()
      return true
    } catch (error) {
      console.error('Failed to create alert:', error)
      return false
    }
  },
})
