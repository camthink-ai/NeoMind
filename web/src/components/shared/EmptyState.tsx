import { Button } from '@/components/ui/button'
import { cn } from '@/lib/utils'
import {
  Server,
  FileJson,
  Zap,
  Workflow,
  Bell,
  Terminal,
  Home,
  Package,
  Users,
  Settings,
  AlertCircle,
  Inbox,
  Search,
} from 'lucide-react'

export type EmptyStateIconType =
  | 'device'
  | 'deviceType'
  | 'rule'
  | 'workflow'
  | 'alert'
  | 'command'
  | 'scenario'
  | 'plugin'
  | 'user'
  | 'settings'
  | 'search'
  | 'inbox'
  | 'custom'

const iconMap: Record<Exclude<EmptyStateIconType, 'custom'>, React.ReactNode> = {
  device: <Server className="h-12 w-12" />,
  deviceType: <FileJson className="h-12 w-12" />,
  rule: <Zap className="h-12 w-12" />,
  workflow: <Workflow className="h-12 w-12" />,
  alert: <Bell className="h-12 w-12" />,
  command: <Terminal className="h-12 w-12" />,
  scenario: <Home className="h-12 w-12" />,
  plugin: <Package className="h-12 w-12" />,
  user: <Users className="h-12 w-12" />,
  settings: <Settings className="h-12 w-12" />,
  search: <Search className="h-12 w-12" />,
  inbox: <Inbox className="h-12 w-12" />,
}

export interface EmptyStateProps {
  icon?: React.ReactNode | EmptyStateIconType
  title: string
  description?: string
  action?: {
    label: string
    onClick: () => void
    variant?: 'default' | 'outline' | 'secondary' | 'ghost' | 'destructive'
    icon?: React.ReactNode
  }
  className?: string
}

/**
 * Empty state component for when there's no data to display
 *
 * @example
 * <EmptyState
 *   icon="device"
 *   title="暂无设备"
 *   description="点击下方按钮添加您的第一个设备"
 *   action={{ label: '添加设备', onClick: handleAdd }}
 * />
 */
export function EmptyState({ icon, title, description, action, className }: EmptyStateProps) {
  const iconContent = typeof icon === 'string' ? iconMap[icon] : icon

  return (
    <div className={cn('flex flex-col items-center justify-center py-12 px-4 text-center', className)}>
      {iconContent && (
        <div className="mb-4 flex h-16 w-16 items-center justify-center rounded-full bg-muted/50 text-muted-foreground">
          {iconContent}
        </div>
      )}
      <h3 className="text-lg font-semibold">{title}</h3>
      {description && (
        <p className="mt-2 text-sm text-muted-foreground max-w-sm">{description}</p>
      )}
      {action && (
        <Button
          onClick={action.onClick}
          variant={action.variant || 'default'}
          className="mt-6"
        >
          {action.icon && <span className="mr-2">{action.icon}</span>}
          {action.label}
        </Button>
      )}
    </div>
  )
}

/**
 * Compact empty state for smaller spaces
 */
export function EmptyStateCompact({
  title,
  description,
  icon,
}: {
  title: string
  description?: string
  icon?: React.ReactNode
}) {
  return (
    <div className="flex flex-col items-center justify-center py-6 text-center">
      {icon && (
        <div className="mb-2 flex h-10 w-10 items-center justify-center rounded-full bg-muted/50 text-muted-foreground">
          {icon}
        </div>
      )}
      <p className="font-medium text-muted-foreground">{title}</p>
      {description && (
        <p className="mt-1 text-sm text-muted-foreground/70">{description}</p>
      )}
    </div>
  )
}

/**
 * Inline empty state for tables and lists
 */
export function EmptyStateInline({
  title,
  colSpan = 1,
  className,
}: {
  title: string
  colSpan?: number
  className?: string
}) {
  return (
    <tr>
      <td colSpan={colSpan} className={cn('py-8', className)}>
        <div className="flex flex-col items-center justify-center text-center">
          <Inbox className="h-10 w-10 text-muted-foreground/50 mb-2" />
          <p className="text-sm text-muted-foreground">{title}</p>
        </div>
      </td>
    </tr>
  )
}
