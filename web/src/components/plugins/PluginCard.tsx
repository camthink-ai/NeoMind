import { useState } from 'react'
import {
  Card,
  CardContent,
  CardDescription,
  CardFooter,
  CardHeader,
  CardTitle,
} from '@/components/ui/card'
import { Button } from '@/components/ui/button'
import { Badge } from '@/components/ui/badge'
import { Switch } from '@/components/ui/switch'
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu'
import {
  MoreVertical,
  Settings,
  Play,
  Square,
  Trash2,
  RefreshCw,
  CheckCircle,
  XCircle,
  AlertCircle,
  Circle,
} from 'lucide-react'
import { cn } from '@/lib/utils'

export interface PluginCardProps {
  id: string
  name: string
  description?: string
  type: string
  enabled: boolean
  running: boolean
  version?: string
  // Optional stats
  deviceCount?: number
  stats?: {
    start_count?: number
    error_count?: number
    total_execution_ms?: number
    avg_response_time_ms?: number
  }
  // Optional actions
  onToggle?: (id: string, enabled: boolean) => Promise<boolean>
  onStart?: (id: string) => Promise<boolean>
  onStop?: (id: string) => Promise<boolean>
  onConfigure?: (id: string) => void
  onDelete?: (id: string) => Promise<boolean>
  onRefresh?: (id: string) => Promise<boolean>
  onViewDevices?: (id: string) => void
  // Optional badge
  badge?: string
}

export function PluginCard({
  id,
  name,
  description,
  type,
  enabled,
  running,
  version,
  deviceCount,
  stats,
  onToggle,
  onStart,
  onStop,
  onConfigure,
  onDelete,
  onRefresh,
  onViewDevices,
  badge,
}: PluginCardProps) {
  const [toggling, setToggling] = useState(false)
  const [starting, setStarting] = useState(false)
  const [stopping, setStopping] = useState(false)
  const [refreshing, setRefreshing] = useState(false)
  const [deleting, setDeleting] = useState(false)

  const handleToggle = async () => {
    if (!onToggle) return
    setToggling(true)
    try {
      await onToggle(id, !enabled)
    } finally {
      setToggling(false)
    }
  }

  const handleStart = async () => {
    if (!onStart) return
    setStarting(true)
    try {
      await onStart(id)
    } finally {
      setStarting(false)
    }
  }

  const handleStop = async () => {
    if (!onStop) return
    setStopping(true)
    try {
      await onStop(id)
    } finally {
      setStopping(false)
    }
  }

  const handleRefresh = async () => {
    if (!onRefresh) return
    setRefreshing(true)
    try {
      await onRefresh(id)
    } finally {
      setRefreshing(false)
    }
  }

  const handleDelete = async () => {
    if (!onDelete) return
    if (!confirm(`确定要删除插件 "${name}" 吗？`)) return
    setDeleting(true)
    try {
      await onDelete(id)
    } finally {
      setDeleting(false)
    }
  }

  const getStatusIcon = () => {
    if (running) return <CheckCircle className="h-4 w-4 text-green-500" />
    if (enabled) return <AlertCircle className="h-4 w-4 text-yellow-500" />
    return <XCircle className="h-4 w-4 text-muted-foreground" />
  }

  const getStatusText = () => {
    if (running) return '运行中'
    if (enabled) return '已启用'
    return '已禁用'
  }

  const getTypeBadgeColor = () => {
    switch (type.toLowerCase()) {
      case 'llm_backend':
        return 'bg-purple-100 text-purple-800 dark:bg-purple-900 dark:text-purple-300'
      case 'device_adapter':
      case 'mqtt':
      case 'modbus':
      case 'hass':
        return 'bg-blue-100 text-blue-800 dark:bg-blue-900 dark:text-blue-300'
      case 'tool':
        return 'bg-green-100 text-green-800 dark:bg-green-900 dark:text-green-300'
      case 'integration':
        return 'bg-orange-100 text-orange-800 dark:bg-orange-900 dark:text-orange-300'
      default:
        return 'bg-gray-100 text-gray-800 dark:bg-gray-800 dark:text-gray-300'
    }
  }

  return (
    <Card className={cn(
      'transition-all duration-200',
      running && 'border-green-500 dark:border-green-700',
      !running && enabled && 'border-yellow-500 dark:border-yellow-700',
    )}>
      <CardHeader className="pb-3">
        <div className="flex items-start justify-between">
          <div className="flex-1 min-w-0">
            <div className="flex items-center gap-2 mb-1">
              <CardTitle className="text-lg truncate">{name}</CardTitle>
              {badge && (
                <Badge variant="secondary" className="text-xs">
                  {badge}
                </Badge>
              )}
            </div>
            <CardDescription className="flex items-center gap-2">
              <span className={cn('text-xs px-2 py-0.5 rounded', getTypeBadgeColor())}>
                {type}
              </span>
              {version && <span className="text-xs">v{version}</span>}
            </CardDescription>
          </div>

          <div className="flex items-center gap-2">
            {getStatusIcon()}
            <DropdownMenu>
              <DropdownMenuTrigger asChild>
                <Button variant="ghost" size="sm" className="h-8 w-8 p-0">
                  <MoreVertical className="h-4 w-4" />
                </Button>
              </DropdownMenuTrigger>
              <DropdownMenuContent align="end">
                {onConfigure && (
                  <DropdownMenuItem onClick={() => onConfigure(id)}>
                    <Settings className="mr-2 h-4 w-4" />
                    配置
                  </DropdownMenuItem>
                )}
                {onRefresh && (
                  <DropdownMenuItem onClick={handleRefresh} disabled={refreshing}>
                    <RefreshCw className={cn('mr-2 h-4 w-4', refreshing && 'animate-spin')} />
                    刷新状态
                  </DropdownMenuItem>
                )}
                {onViewDevices && deviceCount !== undefined && (
                  <DropdownMenuItem onClick={() => onViewDevices(id)}>
                    <Circle className="mr-2 h-4 w-4" />
                    查看设备 ({deviceCount})
                  </DropdownMenuItem>
                )}
                <DropdownMenuSeparator />
                {onDelete && (
                  <DropdownMenuItem
                    onClick={handleDelete}
                    disabled={deleting || running}
                    className="text-destructive focus:text-destructive"
                  >
                    <Trash2 className="mr-2 h-4 w-4" />
                    {deleting ? '删除中...' : '删除'}
                  </DropdownMenuItem>
                )}
              </DropdownMenuContent>
            </DropdownMenu>
          </div>
        </div>

        {description && (
          <p className="text-sm text-muted-foreground mt-2 line-clamp-2">
            {description}
          </p>
        )}
      </CardHeader>

      <CardContent className="pb-3">
        <div className="flex items-center justify-between text-sm">
          <span className="text-muted-foreground">状态:</span>
          <span className="font-medium">{getStatusText()}</span>
        </div>

        {deviceCount !== undefined && (
          <div className="flex items-center justify-between text-sm mt-1">
            <span className="text-muted-foreground">设备数:</span>
            <span className="font-medium">{deviceCount}</span>
          </div>
        )}

        {stats && (
          <div className="flex gap-4 mt-3 text-xs text-muted-foreground">
            {stats.start_count !== undefined && (
              <span>启动: {stats.start_count}</span>
            )}
            {stats.error_count !== undefined && stats.error_count > 0 && (
              <span className="text-red-500">错误: {stats.error_count}</span>
            )}
            {stats.avg_response_time_ms !== undefined && (
              <span>平均: {stats.avg_response_time_ms.toFixed(0)}ms</span>
            )}
          </div>
        )}
      </CardContent>

      <CardFooter className="pt-3 border-t flex justify-between">
        <div className="flex items-center gap-2">
          {onToggle && (
            <>
              <Switch
                checked={enabled}
                onCheckedChange={handleToggle}
                disabled={toggling || running}
              />
              <span className="text-sm text-muted-foreground">
                {enabled ? '已启用' : '已禁用'}
              </span>
            </>
          )}
        </div>

        <div className="flex gap-2">
          {!running && enabled && onStart && (
            <Button
              size="sm"
              variant="outline"
              onClick={handleStart}
              disabled={starting}
            >
              {starting ? (
                <>
                  <RefreshCw className="mr-2 h-4 w-4 animate-spin" />
                  启动中
                </>
              ) : (
                <>
                  <Play className="mr-2 h-4 w-4" />
                  启动
                </>
              )}
            </Button>
          )}
          {running && onStop && (
            <Button
              size="sm"
              variant="outline"
              onClick={handleStop}
              disabled={stopping}
            >
              {stopping ? (
                <>
                  <RefreshCw className="mr-2 h-4 w-4 animate-spin" />
                  停止中
                </>
              ) : (
                <>
                  <Square className="mr-2 h-4 w-4" />
                  停止
                </>
              )}
            </Button>
          )}
        </div>
      </CardFooter>
    </Card>
  )
}
