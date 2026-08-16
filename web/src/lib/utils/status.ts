/**
 * Get color variant for a given status
 * @param status - Status string
 * @returns Tailwind color class
 */
export function getStatusColor(status: string): 'success' | 'warning' | 'error' | 'info' | 'muted' {
  const s = status.toLowerCase()

  // Success statuses
  if (['online', 'active', 'enabled', 'connected', 'completed', 'success', 'approved', 'executed', 'running'].includes(s)) {
    return 'success'
  }

  // Warning statuses
  if (['offline', 'pending', 'waiting', 'buffering', 'warning', 'retry'].includes(s)) {
    return 'warning'
  }

  // Error statuses
  if (['inactive', 'disabled', 'failed', 'error', 'rejected', 'timeout', 'critical'].includes(s)) {
    return 'error'
  }

  // Info statuses
  if (['idle', 'paused', 'stopped', 'info', 'disconnected'].includes(s)) {
    return 'info'
  }

  return 'muted'
}

/**
 * Get localized label for a given status
 * @param status - Status string
 * @returns Localized status label
 */

