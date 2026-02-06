/**
 * Device Adapter Types
 *
 * Static list of built-in device adapter types.
 * Previously fetched from /api/device-adapters/types, now hardcoded.
 */

import type { AdapterType } from '@/types'

/**
 * Built-in device adapter types
 */
export const ADAPTER_TYPES: AdapterType[] = [
  {
    id: 'mqtt',
    name: 'MQTT',
    description: 'MQTT broker connections (built-in + external)',
    icon: 'Server',
    icon_bg: 'bg-blue-100 text-blue-700 dark:bg-blue-900/20 dark:text-blue-400',
    mode: 'push',
    can_add_multiple: true,
    builtin: true,
  },
  {
    id: 'http',
    name: 'HTTP (Polling)',
    description: 'Poll data from device REST APIs on a schedule',
    icon: 'Radio',
    icon_bg: 'bg-orange-100 text-orange-700 dark:bg-orange-900/20 dark:text-orange-400',
    mode: 'pull',
    can_add_multiple: true,
    builtin: true,
  },
  {
    id: 'webhook',
    name: 'Webhook',
    description: 'Devices push data via HTTP POST to your server',
    icon: 'Webhook',
    icon_bg: 'bg-green-100 text-green-700 dark:bg-green-900/20 dark:text-green-400',
    mode: 'push',
    can_add_multiple: false,
    builtin: true,
  },
]

/**
 * Get adapter type by ID
 */
export const getAdapterType = (id: string): AdapterType | undefined => {
  return ADAPTER_TYPES.find((type) => type.id === id)
}

/**
 * Get adapter icon component name
 */
export const ADAPTER_ICONS: Record<string, string> = {
  mqtt: 'Server',
  http: 'Radio',
  webhook: 'Webhook',
}
