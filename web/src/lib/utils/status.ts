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
  if (['pending', 'waiting', 'buffering', 'warning', 'disconnected', 'retry'].includes(s)) {
    return 'warning'
  }

  // Error statuses
  if (['offline', 'inactive', 'disabled', 'failed', 'error', 'rejected', 'timeout', 'critical'].includes(s)) {
    return 'error'
  }

  // Info statuses
  if (['idle', 'paused', 'stopped', 'info'].includes(s)) {
    return 'info'
  }

  return 'muted'
}

/**
 * Get localized label for a given status
 * @param status - Status string
 * @returns Localized status label
 */
export function getStatusLabel(status: string): string {
  const labels: Record<string, string> = {
    // Device/Connection status
    online: '在线',
    offline: '离线',
    connected: '已连接',
    disconnected: '已断开',

    // Execution status
    active: '运行中',
    inactive: '未激活',
    enabled: '已启用',
    disabled: '已禁用',
    running: '运行中',
    stopped: '已停止',
    paused: '已暂停',
    idle: '空闲',

    // Task status
    pending: '待处理',
    waiting: '等待中',
    completed: '已完成',
    failed: '失败',
    timeout: '超时',
    retry: '重试中',
    buffering: '缓冲中',

    // Decision status
    approved: '已批准',
    rejected: '已拒绝',
    executed: '已执行',

    // Alert levels
    critical: '严重',
    warning: '警告',
    info: '信息',

    // General
    success: '成功',
    error: '错误',
    unknown: '未知',
  }

  return labels[status.toLowerCase()] || status
}

/**
 * Get button variant for a given status
 * @param status - Status string
 * @returns Button variant
 */
export function getStatusVariant(status: string): 'default' | 'outline' | 'destructive' | 'secondary' {
  const s = status.toLowerCase()

  if (['failed', 'error', 'rejected', 'critical', 'timeout'].includes(s)) {
    return 'destructive'
  }

  if (['online', 'active', 'enabled', 'connected', 'completed', 'success', 'approved', 'executed'].includes(s)) {
    return 'default'
  }

  return 'secondary'
}

/**
 * Get alert level color
 * @param level - Alert level (critical, warning, info)
 * @returns CSS color class
 */
export function getAlertLevelColor(level: string): string {
  const l = level.toLowerCase()

  if (l === 'critical') {
    return 'text-error bg-error/10 border-error/20'
  }
  if (l === 'warning') {
    return 'text-warning bg-warning/10 border-warning/20'
  }
  if (l === 'info') {
    return 'text-info bg-info/10 border-info/20'
  }

  return 'text-muted-foreground bg-muted/50'
}

/**
 * Get alert level label
 * @param level - Alert level
 * @returns Localized level label
 */
export function getAlertLevelLabel(level: string): string {
  const labels: Record<string, string> = {
    critical: '严重',
    warning: '警告',
    info: '信息',
  }

  return labels[level.toLowerCase()] || level
}
