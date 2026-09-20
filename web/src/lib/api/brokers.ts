/** brokers api — split from api.ts 2026-09. */
import type { MqttStatus, ExternalBroker } from '@/types'
import { fetchAPI, getApiBase, getApiKey, tokenManager } from "./client"

export const brokersApi = {
  // ========== MQTT / Brokers API ==========
  // Used by UnifiedDeviceConnectionsTab to display connection status

  getMqttStatus: () => fetchAPI<{ status: MqttStatus }>('/mqtt/status'),

  getBrokers: () => fetchAPI<{ brokers: ExternalBroker[]; count: number }>('/brokers'),
  getBroker: (id: string) => fetchAPI<{ broker: ExternalBroker }>(`/brokers/${id}`),
  createBroker: (broker: Omit<ExternalBroker, 'id' | 'updated_at' | 'connected' | 'last_error'> & { id?: string }) =>
    fetchAPI<{ broker: ExternalBroker; message?: string }>('/brokers', {
      method: 'POST',
      body: JSON.stringify(broker),
    }),
  updateBroker: (id: string, broker: Omit<ExternalBroker, 'id' | 'updated_at' | 'connected' | 'last_error'>) =>
    fetchAPI<{ broker: ExternalBroker; message?: string }>(`/brokers/${id}`, {
      method: 'PUT',
      body: JSON.stringify(broker),
    }),
  deleteBroker: (id: string) =>
    fetchAPI<{ message?: string }>(`/brokers/${id}`, {
      method: 'DELETE',
    }),
  testBroker: (id: string) =>
    fetchAPI<{ success: boolean; message?: string; broker_url?: string; broker?: ExternalBroker }>(`/brokers/${id}/test`, {
      method: 'POST',
    }),

  // Embedded Broker Config
  getEmbeddedBrokerConfig: () =>
    fetchAPI<{
      config: {
        listen: string
        port: number
        max_connections: number
        auth_enabled: boolean
        credentials: { username: string; password: string }[]
        tls_enabled: boolean
        tls_cert_path: string | null
        tls_key_path: string | null
        tls_ca_path: string | null
        device_id_field?: string | null
      }
    }>('/mqtt/broker-config').then((res) => res.config),

  updateEmbeddedBrokerConfig: (config: {
    listen?: string
    port?: number
    auth_enabled?: boolean
    tls_enabled?: boolean
    device_id_field?: string | null
  }) => fetchAPI<{ message: string; restart_required?: boolean }>('/mqtt/broker-config', {
    method: 'PUT',
    body: JSON.stringify(config),
  }),

  addMqttCredential: (username: string, password: string) =>
    fetchAPI<{ message: string }>('/mqtt/broker-config/credentials', {
      method: 'POST',
      body: JSON.stringify({ username, password }),
    }),

  deleteMqttCredential: (username: string) =>
    fetchAPI<{ message: string }>('/mqtt/broker-config/credentials/delete', {
      method: 'POST',
      body: JSON.stringify({ username }),
    }),

  uploadMqttTlsCert: (certPem: string, keyPem: string, caPem?: string) =>
    fetchAPI<{ message: string }>('/mqtt/broker-config/tls', {
      method: 'PUT',
      body: JSON.stringify({ cert_pem: certPem, key_pem: keyPem, ca_pem: caPem }),
    }),

  generateMqttTlsCert: () =>
    fetchAPI<{ message: string; ca_path: string }>('/mqtt/broker-config/tls/generate', {
      method: 'POST',
    }),

  downloadMqttCaCert: async () => {
    const base = getApiBase()
    const headers: Record<string, string> = {}
    const token = tokenManager.getToken()
    if (token) {
      headers['Authorization'] = `Bearer ${token}`
    }
    const apiKey = getApiKey()
    if (apiKey) {
      headers['X-API-Key'] = apiKey
    }
    const response = await fetch(`${base}/mqtt/broker-config/tls/ca-cert`, { headers })
    if (!response.ok) {
      // Parse structured error from response body
      try {
        const body = await response.json()
        const message = body?.error?.message || body?.message || `Download failed (${response.status})`
        throw new Error(message)
      } catch (e) {
        if (e instanceof Error) throw e
        throw new Error(`Failed to download CA certificate (${response.status})`)
      }
    }
    const blob = await response.blob()
    const url = window.URL.createObjectURL(blob)
    const a = document.createElement('a')
    a.href = url
    a.download = 'mqtt-ca.crt'
    document.body.appendChild(a)
    a.click()
    document.body.removeChild(a)
    window.URL.revokeObjectURL(url)
  },

}
