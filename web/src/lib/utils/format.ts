/**
 * Format timestamp to human readable string
 * @param timestamp - Unix timestamp in seconds, ISO 8601 string, or undefined
 * @returns Formatted date string
 */
export function formatTimestamp(timestamp: string | number | undefined): string {
  if (!timestamp) return '-'

  // Handle ISO 8601 string from backend
  if (typeof timestamp === 'string') {
    const date = new Date(timestamp)
    const now = Date.now()
    const diff = now - date.getTime()

    // Less than 1 minute
    if (diff < 60 * 1000) {
      return '刚刚'
    }

    // Less than 1 hour
    if (diff < 60 * 60 * 1000) {
      const mins = Math.floor(diff / (60 * 1000))
      return `${mins}分钟前`
    }

    // Less than 1 day
    if (diff < 24 * 60 * 60 * 1000) {
      const hours = Math.floor(diff / (60 * 60 * 1000))
      return `${hours}小时前`
    }

    // Less than 7 days
    if (diff < 7 * 24 * 60 * 60 * 1000) {
      const days = Math.floor(diff / (24 * 60 * 60 * 1000))
      return `${days}天前`
    }

    // Format as date
    return date.toLocaleDateString('zh-CN', {
      year: 'numeric',
      month: '2-digit',
      day: '2-digit',
    })
  }

  // Handle Unix timestamp (number, in seconds)
  const now = Date.now()
  const ts = timestamp * 1000 // Convert to milliseconds
  const diff = now - ts

  // Less than 1 minute
  if (diff < 60 * 1000) {
    return '刚刚'
  }

  // Less than 1 hour
  if (diff < 60 * 60 * 1000) {
    const mins = Math.floor(diff / (60 * 1000))
    return `${mins}分钟前`
  }

  // Less than 1 day
  if (diff < 24 * 60 * 60 * 1000) {
    const hours = Math.floor(diff / (60 * 60 * 1000))
    return `${hours}小时前`
  }

  // Less than 7 days
  if (diff < 7 * 24 * 60 * 60 * 1000) {
    const days = Math.floor(diff / (24 * 60 * 60 * 1000))
    return `${days}天前`
  }

  // Format as date
  const date = new Date(ts)
  return date.toLocaleDateString('zh-CN', {
    year: 'numeric',
    month: '2-digit',
    day: '2-digit',
  })
}

/**
 * Format duration in seconds to human readable string
 * @param seconds - Duration in seconds
 * @returns Formatted duration string
 */
export function formatDuration(seconds: number): string {
  if (seconds < 60) {
    return `${seconds}秒`
  }

  if (seconds < 3600) {
    const mins = Math.floor(seconds / 60)
    return `${mins}分钟`
  }

  if (seconds < 86400) {
    const hours = Math.floor(seconds / 3600)
    const mins = Math.floor((seconds % 3600) / 60)
    return mins > 0 ? `${hours}小时${mins}分` : `${hours}小时`
  }

  const days = Math.floor(seconds / 86400)
  const hours = Math.floor((seconds % 86400) / 3600)
  return hours > 0 ? `${days}天${hours}h` : `${days}天`
}

/**
 * Format uptime to human readable string
 * @param seconds - Uptime in seconds
 * @returns Formatted uptime string
 */
export function formatUptime(seconds: number): string {
  const days = Math.floor(seconds / 86400)
  const hours = Math.floor((seconds % 86400) / 3600)
  if (days > 0) return `${days}天${hours}h`
  if (hours > 0) return `${hours}h`
  return '<1h'
}

/**
 * Format number with locale-specific formatting
 * @param num - Number to format
 * @returns Formatted number string
 */
export function formatNumber(num: number): string {
  if (num >= 1000000) {
    return `${(num / 1000000).toFixed(1)}M`
  }
  if (num >= 1000) {
    return `${(num / 1000).toFixed(1)}K`
  }
  return num.toString()
}

/**
 * Format percentage
 * @param value - Current value
 * @param total - Total value
 * @returns Formatted percentage string
 */
export function formatPercentage(value: number, total: number): string {
  if (total === 0) return '0%'
  return Math.round((value / total) * 100) + '%'
}

/**
 * Format bytes to human readable string
 * @param bytes - Number of bytes
 * @returns Formatted bytes string
 */
export function formatBytes(bytes: number): string {
  if (bytes === 0) return '0 B'
  const k = 1024
  const sizes = ['B', 'KB', 'MB', 'GB', 'TB']
  const i = Math.floor(Math.log(bytes) / Math.log(k))
  return parseFloat((bytes / Math.pow(k, i)).toFixed(2)) + ' ' + sizes[i]
}
