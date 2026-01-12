/**
 * Settings Slice
 *
 * Handles application settings (LLM, MQTT) and external brokers.
 */

import type { StateCreator } from 'zustand'
import type { SettingsState, BrokerState, HassDiscoveryState } from '../types'
import type { LlmSettings, MqttSettings, ExternalBroker, MqttStatus } from '@/types'
import type { HassDiscoveryRequest, HassDiscoveryResponse, HassProcessRequest, HassDiscoveredDevice } from '@/types'
import { api } from '@/lib/api'

export interface SettingsSlice extends SettingsState, BrokerState, HassDiscoveryState {
  // Dialog actions
  setSettingsDialogOpen: (open: boolean) => void

  // LLM Settings actions
  fetchLlmSettings: () => Promise<void>
  updateLlmSettings: (settings: LlmSettings) => Promise<boolean>
  testLlm: () => Promise<boolean>

  // MQTT Settings actions
  fetchMqttSettings: () => Promise<void>
  updateMqttSettings: (settings: MqttSettings) => Promise<boolean>
  getMqttStatus: () => Promise<MqttStatus | null>

  // External Broker actions
  fetchBrokers: () => Promise<void>
  createBroker: (broker: Omit<ExternalBroker, 'id' | 'updated_at' | 'connected' | 'last_error'> & { id?: string }) => Promise<boolean>
  updateBroker: (id: string, broker: Omit<ExternalBroker, 'id' | 'updated_at' | 'connected' | 'last_error'>) => Promise<boolean>
  deleteBroker: (id: string) => Promise<boolean>
  testBroker: (id: string) => Promise<boolean>
  toggleBroker: (id: string) => Promise<boolean>

  // HASS Discovery actions
  fetchHassDiscoveryStatus: () => Promise<void>
  fetchHassDiscoveredDevices: () => Promise<void>
  startHassDiscovery: (req: HassDiscoveryRequest) => Promise<HassDiscoveryResponse>
  stopHassDiscovery: () => Promise<{ stopped: boolean }>
  stopHassDiscoveryPolling: () => void
  processHassDiscovery: (req: HassProcessRequest) => Promise<HassDiscoveredDevice | null>
  registerHassDevice: (deviceId: string) => Promise<boolean>
  unregisterHassDevice: (deviceId: string) => Promise<boolean>
  clearHassDiscoveredDevices: () => void
}

export const createSettingsSlice: StateCreator<
  SettingsSlice,
  [],
  [],
  SettingsSlice
> = (set, get) => ({
  // Initial state
  llmSettings: null,
  mqttSettings: null,
  settingsDialogOpen: false,
  externalBrokers: [],
  brokersLoading: false,
  hassDiscovering: false,
  hassDiscoveredDevices: [],
  hassDiscoveryStatus: null,

  // Dialog actions
  setSettingsDialogOpen: (open) => set({ settingsDialogOpen: open }),

  // LLM Settings
  fetchLlmSettings: async () => {
    try {
      const settings = await api.getLlmSettings()
      set({ llmSettings: settings })
    } catch (error) {
      console.error('Failed to fetch LLM settings:', error)
    }
  },

  updateLlmSettings: async (settings) => {
    try {
      await api.updateLlmSettings(settings)
      set({ llmSettings: settings })
      return true
    } catch (error) {
      console.error('Failed to update LLM settings:', error)
      return false
    }
  },

  testLlm: async () => {
    try {
      const settings = get().llmSettings
      if (!settings) {
        return false
      }
      await api.testLlm(settings)
      return true
    } catch (error) {
      console.error('Failed to test LLM:', error)
      return false
    }
  },

  // MQTT Settings
  fetchMqttSettings: async () => {
    try {
      const result = await api.getMqttSettings()
      set({ mqttSettings: result.settings })
    } catch (error) {
      console.error('Failed to fetch MQTT settings:', error)
    }
  },

  updateMqttSettings: async (settings) => {
    try {
      const result = await api.updateMqttSettings(settings)
      set({ mqttSettings: result.settings })
      return true
    } catch (error) {
      console.error('Failed to update MQTT settings:', error)
      return false
    }
  },

  getMqttStatus: async () => {
    try {
      const result = await api.getMqttStatus()
      return result.status
    } catch (error) {
      console.error('Failed to get MQTT status:', error)
      return null
    }
  },

  // External Brokers
  fetchBrokers: async () => {
    set({ brokersLoading: true })
    try {
      const result = await api.getBrokers()
      set({ externalBrokers: result.brokers || [] })
    } catch (error) {
      console.error('Failed to fetch external brokers:', error)
      set({ externalBrokers: [] })
    } finally {
      set({ brokersLoading: false })
    }
  },

  createBroker: async (broker) => {
    try {
      await api.createBroker(broker)
      await get().fetchBrokers()
      return true
    } catch (error) {
      console.error('Failed to create broker:', error)
      return false
    }
  },

  updateBroker: async (id, broker) => {
    try {
      await api.updateBroker(id, broker)
      await get().fetchBrokers()
      return true
    } catch (error) {
      console.error('Failed to update broker:', error)
      return false
    }
  },

  deleteBroker: async (id) => {
    try {
      await api.deleteBroker(id)
      await get().fetchBrokers()
      return true
    } catch (error) {
      console.error('Failed to delete broker:', error)
      return false
    }
  },

  testBroker: async (id) => {
    try {
      const result = await api.testBroker(id)
      // If the response includes the updated broker, use it to update the state
      if (result.broker) {
        set((state) => ({
          externalBrokers: state.externalBrokers.map((b) =>
            b.id === id ? result.broker! : b
          ),
        }))
      } else {
        // Fallback: refresh all brokers
        await get().fetchBrokers()
      }
      return result.success === true
    } catch (error) {
      console.error('Failed to test broker:', error)
      return false
    }
  },

  toggleBroker: async (id) => {
    try {
      const brokers = get().externalBrokers
      const broker = brokers.find(b => b.id === id)
      if (!broker) return false

      await api.updateBroker(id, {
        name: broker.name,
        broker: broker.broker,
        port: broker.port,
        tls: broker.tls,
        username: broker.username,
        password: broker.password || '',
        enabled: !broker.enabled,
      })

      await get().fetchBrokers()
      return true
    } catch (error) {
      console.error('Failed to toggle broker:', error)
      return false
    }
  },

  // HASS Discovery
  fetchHassDiscoveryStatus: async () => {
    try {
      const response = await api.getHassDiscoveryStatus()
      set({ hassDiscoveryStatus: response })
    } catch (error) {
      console.error('Failed to fetch HASS discovery status:', error)
      set({ hassDiscoveryStatus: null })
    }
  },

  fetchHassDiscoveredDevices: async () => {
    try {
      const response = await api.getHassDiscoveredDevices()
      set({ hassDiscoveredDevices: response.devices || [] })
    } catch (error) {
      console.error('Failed to fetch HASS discovered devices:', error)
      set({ hassDiscoveredDevices: [] })
    }
  },

  startHassDiscovery: async (req) => {
    set({ hassDiscovering: true })
    try {
      const result = await api.startHassDiscovery(req)

      // Clear any existing polling interval before starting a new one
      get().stopHassDiscoveryPolling()

      // Start polling for discovered devices
      const pollInterval = setInterval(async () => {
        await get().fetchHassDiscoveredDevices()
      }, 2000)

      // Store interval ID for cleanup
      ;(window as any).__hassDiscoveryPollInterval = pollInterval

      return result
    } catch (error) {
      console.error('Failed to start HASS discovery:', error)
      throw error
    } finally {
      set({ hassDiscovering: false })
    }
  },

  stopHassDiscovery: async () => {
    try {
      const result = await api.stopHassDiscovery()
      get().stopHassDiscoveryPolling()
      await get().fetchHassDiscoveryStatus()
      return result
    } catch (error) {
      console.error('Failed to stop HASS discovery:', error)
      throw error
    }
  },

  stopHassDiscoveryPolling: () => {
    const interval = (window as any).__hassDiscoveryPollInterval
    if (interval) {
      clearInterval(interval)
      delete (window as any).__hassDiscoveryPollInterval
    }
  },

  processHassDiscovery: async (req) => {
    try {
      const result = await api.processHassDiscovery(req)
      set((state) => {
        const exists = state.hassDiscoveredDevices.some(
          (d) => d.device_id === result.device?.device_id
        )
        if (exists) {
          return state
        }
        return {
          hassDiscoveredDevices: [...state.hassDiscoveredDevices, result.device],
        }
      })
      return result.device
    } catch (error) {
      console.error('Failed to process HASS discovery:', error)
      return null
    }
  },

  registerHassDevice: async (deviceId) => {
    try {
      await api.registerAggregatedHassDevice(deviceId)
      set((state) => ({
        hassDiscoveredDevices: state.hassDiscoveredDevices.map((d) =>
          d.device_id === deviceId
            ? { ...d, already_registered: true }
            : d
        ),
      }))
      // Note: fetchDevices would need to be called from device slice
      return true
    } catch (error) {
      console.error('Failed to register HASS device:', error)
      return false
    }
  },

  unregisterHassDevice: async (deviceId) => {
    try {
      await api.unregisterHassDevice(deviceId)
      set((state) => ({
        hassDiscoveredDevices: state.hassDiscoveredDevices.map((d) =>
          d.device_id === deviceId
            ? { ...d, already_registered: false }
            : d
        ),
      }))
      return true
    } catch (error) {
      console.error('Failed to unregister HASS device:', error)
      return false
    }
  },

  clearHassDiscoveredDevices: () => set({ hassDiscoveredDevices: [] }),
})
