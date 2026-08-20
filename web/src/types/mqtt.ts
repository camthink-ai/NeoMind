// ========== MQTT Broker Types ==========

export interface ExternalBroker {
  id: string
  name: string
  broker: string
  port: number
  tls: boolean
  username?: string
  password?: string
  ca_cert?: string
  client_cert?: string
  client_key?: string
  client_id?: string
  enabled: boolean
  connected?: boolean
  last_error?: string
  updated_at: number
  subscribe_topics?: string[]
  /** Payload field used as device identity when the topic can't uniquely identify the device */
  device_id_field?: string
}

// Data Source Types
export interface MqttStatus {
  connected: boolean
  listen_address: string
  subscriptions_count: number
  devices_count: number
  clients_count: number
  server_ip: string
  listen_port: number
  tls_enabled: boolean
  external_brokers?: ExternalBrokerConnection[]
}

export interface ExternalBrokerConnection {
  id: string
  name: string
  /** API may return "host" or "broker" depending on version */
  host?: string
  broker: string
  port: number
  tls: boolean
  connected: boolean
  enabled: boolean
  last_error?: string
  subscribe_topics?: string[]
  // TLS certificate fields
  ca_cert?: string
  client_cert?: string
  client_key?: string
  // MQTT client configuration
  client_id?: string
}
