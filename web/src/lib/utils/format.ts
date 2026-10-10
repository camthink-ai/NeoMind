/**
 * Format timestamp to human readable string
 * @param timestamp - Unix timestamp in seconds, ISO 8601 string, or undefined
 * @param includeSeconds - Whether to include seconds (default: true)
 * @returns Formatted date string in format "YYYY-MM-DD HH:mm:ss"
 */
export function formatTimestamp(timestamp: string | number | undefined, includeSeconds: boolean = true): string {
  if (!timestamp) return '-'

  let date: Date

  // Handle ISO 8601 string from backend
  if (typeof timestamp === 'string') {
    date = new Date(timestamp)
  } else if (typeof timestamp === 'number') {
    // Handle Unix timestamp (in seconds or milliseconds)
    // If timestamp is less than 10000000000, it's in seconds
    date = new Date(timestamp < 10000000000 ? timestamp * 1000 : timestamp)
  } else {
    return '-'
  }

  // Check if date is valid
  if (isNaN(date.getTime())) return '-'

  const year = date.getFullYear()
  const month = String(date.getMonth() + 1).padStart(2, '0')
  const day = String(date.getDate()).padStart(2, '0')
  const hours = String(date.getHours()).padStart(2, '0')
  const minutes = String(date.getMinutes()).padStart(2, '0')
  const seconds = String(date.getSeconds()).padStart(2, '0')

  if (includeSeconds) {
    return `${year}-${month}-${day} ${hours}:${minutes}:${seconds}`
  }
  return `${year}-${month}-${day} ${hours}:${minutes}`
}

/**
 * Format timestamp to relative time (for places where relative time is preferred)
 * @param timestamp - Unix timestamp in seconds, ISO 8601 string, or undefined
 * @returns Relative time string like "刚刚", "5分钟前", etc.
 */
export function formatRelativeTime(timestamp: string | number | undefined): string {
  if (!timestamp) return '-'

  let date: Date

  if (typeof timestamp === 'string') {
    date = new Date(timestamp)
  } else if (typeof timestamp === 'number') {
    date = new Date(timestamp < 10000000000 ? timestamp * 1000 : timestamp)
  } else {
    return '-'
  }

  if (isNaN(date.getTime())) return '-'

  const now = Date.now()
  const diff = now - date.getTime()

  // Future time
  if (diff < 0) {
    return formatTimestamp(typeof timestamp === 'string' ? timestamp : timestamp)
  }

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

  // Older than 7 days - show absolute date
  return formatTimestamp(timestamp, false)
}

/**
 * Format date only (without time)
 * @param timestamp - Unix timestamp in seconds, ISO 8601 string, or undefined
 * @returns Formatted date string in format "YYYY-MM-DD"
 */
export function formatDate(timestamp: string | number | undefined): string {
  if (!timestamp) return '-'

  let date: Date

  if (typeof timestamp === 'string') {
    date = new Date(timestamp)
  } else if (typeof timestamp === 'number') {
    date = new Date(timestamp < 10000000000 ? timestamp * 1000 : timestamp)
  } else {
    return '-'
  }

  if (isNaN(date.getTime())) return '-'

  const year = date.getFullYear()
  const month = String(date.getMonth() + 1).padStart(2, '0')
  const day = String(date.getDate()).padStart(2, '0')

  return `${year}-${month}-${day}`
}

/**
 * Format time only (without date)
 * @param timestamp - Unix timestamp in seconds, ISO 8601 string, or undefined
 * @param includeSeconds - Whether to include seconds (default: true)
 * @returns Formatted time string in format "HH:mm:ss" or "HH:mm"
 */
export function formatTime(timestamp: string | number | undefined, includeSeconds: boolean = true): string {
  if (!timestamp) return '-'

  let date: Date

  if (typeof timestamp === 'string') {
    date = new Date(timestamp)
  } else if (typeof timestamp === 'number') {
    date = new Date(timestamp < 10000000000 ? timestamp * 1000 : timestamp)
  } else {
    return '-'
  }

  if (isNaN(date.getTime())) return '-'

  const hours = String(date.getHours()).padStart(2, '0')
  const minutes = String(date.getMinutes()).padStart(2, '0')
  const seconds = String(date.getSeconds()).padStart(2, '0')

  if (includeSeconds) {
    return `${hours}:${minutes}:${seconds}`
  }
  return `${hours}:${minutes}`
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
 * Format an elapsed/generation time (ms) with adaptive units — chat reply
 * stats, tool durations. Unit-styled and locale-independent (unlike
 * formatDuration's Chinese prose): 850ms, 12.3s, 2m 05s, 1h 30m.
 */
export function formatElapsedMs(ms: number): string {
  if (!Number.isFinite(ms) || ms <= 0) return '-'
  if (ms < 1000) return `${Math.round(ms)}ms`
  const secs = ms / 1000
  // 59.95s boundary: toFixed(1) would otherwise print "60.0s"
  if (secs < 59.95) return `${secs.toFixed(1)}s`
  const totalSecs = Math.round(secs)
  if (totalSecs < 3600) {
    const mins = Math.floor(totalSecs / 60)
    return `${mins}m ${String(totalSecs % 60).padStart(2, '0')}s`
  }
  const hours = Math.floor(totalSecs / 3600)
  const mins = Math.floor((totalSecs % 3600) / 60)
  return mins > 0 ? `${hours}h ${mins}m` : `${hours}h`
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
